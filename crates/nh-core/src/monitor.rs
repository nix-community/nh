//! Runs Nix builds, optionally with rom rendering their progress.

use std::{
  env,
  fs::File,
  io::{self, BufWriter, ErrorKind, IsTerminal, Read, Write},
  sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
  thread,
  time::{Duration, Instant},
};

use misstep::{OptionExt, Result, ResultExt};
use rom::{
  EngineConfig,
  FilesystemResolver,
  Output,
  RenderConfig,
  StreamEngine,
  cache::BuildReportCache,
  display::format_log,
  state::current_time,
  terminal::{Admission, LiveTerminal},
};
use serde::Serialize;
use subprocess::{Exec, ExitStatus, Redirection};
use tracing::{debug, warn};

const FRAME_INTERVAL: Duration = Duration::from_millis(50);
const TIMER_INTERVAL: Duration = Duration::from_secs(1);

/// Runs `command`, returning its exit status and stdout.
///
/// With `rom`, rom renders the internal-JSON logs `command` writes to stderr.
/// If monitoring fails, `command` is killed, since nothing would drain its
/// logs anymore. Without `rom`, stderr is inherited.
///
/// # Errors
///
/// Returns an error if `command` cannot be started or its output cannot be
/// read, decoded, or presented.
pub fn run(command: Exec, rom: bool) -> Result<(ExitStatus, Vec<u8>)> {
  if !rom {
    let command = command.stdout(Redirection::Pipe);
    debug!(?command);
    let mut job = command.start().context("Failed to start build")?;
    let mut stdout = Vec::new();
    job
      .stdout
      .take()
      .context("Failed to capture build output")?
      .read_to_end(&mut stdout)?;
    return Ok((job.wait()?, stdout));
  }

  let mut monitor = Monitor::new()?;
  let command = command
    .args(["--log-format", "internal-json", "--verbose"])
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Pipe);
  debug!(?command);
  let mut job = command.start().context("Failed to start build")?;

  // Nix blocks writing a full pipe, so neither pipe may wait on the other or
  // on rendering.
  let (sender, chunks) = mpsc::sync_channel(64);
  let stdout = job
    .stdout
    .take()
    .context("Failed to capture build output")?;
  let stderr = job.stderr.take().context("Failed to capture build logs")?;
  spawn_reader(stdout, &sender, Chunk::Stdout);
  spawn_reader(stderr, &sender, Chunk::Stderr);
  drop(sender);

  let mut stdout = Vec::new();
  let result = monitor.drive(&chunks, &mut stdout);
  drop(chunks);

  if let Err(error) = result {
    if let Err(error) = job.kill() {
      warn!("Failed to stop monitored build: {error}");
    }
    job.wait().context("Failed to reap monitored build")?;
    return Err(error).context("rom failed to monitor the Nix build");
  }

  let status = job.wait()?;
  // Nix owns the build result; a presentation error must not mask it.
  if let Err(error) = monitor.finish(status.success()) {
    warn!("rom failed to present the Nix build: {error}");
  }
  Ok((status, stdout))
}

enum Chunk {
  Stdout(Vec<u8>),
  Stderr(Vec<u8>),
}

fn spawn_reader(
  mut pipe: File,
  sender: &SyncSender<io::Result<Chunk>>,
  chunk: fn(Vec<u8>) -> Chunk,
) {
  let sender = sender.clone();
  thread::spawn(move || {
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
      let read = match pipe.read(&mut buffer) {
        Ok(0) => return,
        Ok(count) => Ok(chunk(buffer[..count].to_vec())),
        Err(error) if error.kind() == ErrorKind::Interrupted => continue,
        Err(error) => Err(error),
      };
      if sender.send(read).is_err() {
        return;
      }
    }
  });
}

struct Monitor {
  stream:   StreamEngine,
  terminal: LiveTerminal<io::Stderr>,
  render:   RenderConfig,
  history:  BuildReportCache,
  recorder: Option<Recorder>,
  dirty:    bool,
}

impl Monitor {
  fn new() -> Result<Self> {
    let history =
      BuildReportCache::new(BuildReportCache::default_cache_path()?);
    let mut stream = StreamEngine::new(EngineConfig::default());
    stream
      .engine_mut()
      .set_resolver(FilesystemResolver::default());
    stream.engine_mut().load_history(&history);

    let dumb = env::var("TERM").as_deref() == Ok("dumb");
    let render = RenderConfig {
      ansi: io::stderr().is_terminal()
        && !dumb
        && env::var_os("NO_COLOR").is_none(),
      ..RenderConfig::default()
    };

    // A retired terminal only appends, for dumb terminals, multiplexers, and
    // redirected output.
    let mut terminal = LiveTerminal::new(io::stderr());
    if dumb || rom::terminal::admission() != Admission::Live {
      terminal.retire()?;
    }

    Ok(Self {
      stream,
      terminal,
      render,
      history,
      recorder: Recorder::from_env()?,
      dirty: false,
    })
  }

  fn drive(
    &mut self,
    chunks: &Receiver<io::Result<Chunk>>,
    stdout: &mut Vec<u8>,
  ) -> Result<()> {
    self.render(false)?;
    let mut last_frame = Instant::now();

    loop {
      let wait = if self.dirty {
        FRAME_INTERVAL.saturating_sub(last_frame.elapsed())
      } else {
        FRAME_INTERVAL
      };
      match chunks.recv_timeout(wait) {
        Ok(Ok(Chunk::Stdout(bytes))) => stdout.extend_from_slice(&bytes),
        Ok(Ok(Chunk::Stderr(bytes))) => self.push(&bytes)?,
        Ok(Err(error)) => {
          return Err(error).context("Failed to read Nix output").erase();
        },
        // Lookups may finish while Nix is quiet.
        Err(RecvTimeoutError::Timeout) => {
          self.dirty |= self.stream.engine_mut().poll_lookups(current_time());
        },
        Err(RecvTimeoutError::Disconnected) => return Ok(()),
      }

      let since_frame = last_frame.elapsed();
      if (self.dirty && since_frame >= FRAME_INTERVAL)
        || since_frame >= TIMER_INTERVAL
      {
        self.render(false)?;
        last_frame = Instant::now();
      }
    }
  }

  fn push(&mut self, bytes: &[u8]) -> Result<()> {
    if let Some(recorder) = &mut self.recorder {
      recorder.write(bytes)?;
    }
    let processed = self.stream.push_at(bytes, current_time())?;
    self.dirty |= processed.changed;
    self.output(processed.output)
  }

  fn output(&mut self, output: Vec<Output>) -> Result<()> {
    self.dirty |= !output.is_empty();
    for item in output {
      match item {
        Output::Passthrough(bytes) => {
          self.terminal.write_passthrough(&bytes)?;
        },
        Output::Log(line) => {
          let line = format_log(&line, &self.render);
          self.terminal.write_passthrough(line.as_bytes())?;
          self.terminal.write_passthrough(b"\n")?;
        },
      }
    }
    Ok(())
  }

  fn render(&mut self, final_render: bool) -> Result<()> {
    self.terminal.render(
      self.stream.engine().state(),
      &self.render,
      current_time(),
      final_render,
    )?;
    self.dirty = false;
    Ok(())
  }

  fn finish(&mut self, succeeded: bool) -> Result<()> {
    let processed = self.stream.finish_at(current_time())?;
    self.output(processed.output)?;
    if succeeded {
      self.stream.engine_mut().mark_planned_available();
    }
    self.render(true)?;
    if self.terminal.is_retired() {
      self.terminal.append_final(
        self.stream.engine().state(),
        &self.render,
        current_time(),
      )?;
    }
    self.terminal.finish()?;

    if let Err(error) = self.stream.engine().save_history(&self.history) {
      debug!(%error, "failed to save rom build history");
    }
    Ok(())
  }
}

/// Records the stream to `NH_ROM_LOG` as JSON lines, `{"t": <seconds>,
/// "line": <line>}`, so it can be replayed through rom.
struct Recorder {
  file:    BufWriter<File>,
  start:   Instant,
  partial: Vec<u8>,
}

#[derive(Serialize)]
struct RecordedLine<'line> {
  t:    f64,
  line: &'line str,
}

impl Recorder {
  fn from_env() -> Result<Option<Self>> {
    let Some(path) = env::var_os("NH_ROM_LOG") else {
      return Ok(None);
    };
    let file = File::create(&path).with_context(|| {
      format!("Failed to create {}", path.to_string_lossy())
    })?;
    Ok(Some(Self {
      file:    BufWriter::new(file),
      start:   Instant::now(),
      partial: Vec::new(),
    }))
  }

  fn write(&mut self, bytes: &[u8]) -> Result<()> {
    self.partial.extend_from_slice(bytes);
    let Some(end) = self.partial.iter().rposition(|&byte| byte == b'\n') else {
      return Ok(());
    };
    let t = self.start.elapsed().as_secs_f64();
    for line in String::from_utf8_lossy(&self.partial[..end]).split('\n') {
      serde_json::to_writer(&mut self.file, &RecordedLine { t, line })?;
      self.file.write_all(b"\n")?;
    }
    self.partial.drain(..=end);
    self.file.flush()?;
    Ok(())
  }
}
