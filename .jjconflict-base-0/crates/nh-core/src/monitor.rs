//! In-process ROM adapter for Nix internal-JSON streams.

use std::{
  io::{self, IsTerminal, Read, Write},
  sync::mpsc::{self, Receiver, RecvTimeoutError},
  thread,
  time::{Duration, Instant},
};

use color_eyre::{
  Result,
  eyre::{Context, eyre},
};
use rom::{
  DisplayFormat,
  EngineConfig,
  FilesystemResolver,
  LegendStyle,
  Output,
  Processed,
  RenderConfig,
  StreamEngine,
  SummaryStyle,
  cache::BuildReportCache,
  display::{format_log, render_frame, write_final},
  state::current_time,
  terminal::{Admission, LiveTerminal},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(50);
const TIMER_INTERVAL: Duration = Duration::from_secs(1);

enum Presenter {
  Live(LiveTerminal<io::Stderr>),
  Fallback {
    writer:            io::Stderr,
    presented_initial: bool,
  },
  Append(io::Stderr),
}

impl Presenter {
  fn new(silent: bool) -> Self {
    match (silent, rom::terminal::admission()) {
      (false, Admission::Live) => Self::Live(LiveTerminal::new(io::stderr())),
      (false, Admission::Multiplexer) => {
        Self::Fallback {
          writer:            io::stderr(),
          presented_initial: false,
        }
      },
      (true, _) | (false, Admission::NotATerminal) => {
        Self::Append(io::stderr())
      },
    }
  }

  fn output(
    &mut self,
    output: Vec<Output>,
    render_config: &RenderConfig,
  ) -> io::Result<bool> {
    let changed = !output.is_empty();
    for item in output {
      match item {
        Output::Passthrough(bytes) => self.write(&bytes)?,
        Output::Log(line) => {
          self.write(format_log(&line, render_config).as_bytes())?;
          self.write(b"\n")?;
        },
      }
    }
    Ok(changed)
  }

  fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
    match self {
      Self::Live(terminal) => terminal.write_passthrough(bytes),
      Self::Fallback { writer, .. } | Self::Append(writer) => {
        writer.write_all(bytes)?;
        writer.flush()
      },
    }
  }

  fn render(
    &mut self,
    stream: &StreamEngine,
    config: &RenderConfig,
    now: f64,
    final_render: bool,
  ) -> io::Result<bool> {
    match self {
      Self::Live(terminal) => {
        terminal.render(stream.engine().state(), config, now, final_render)
      },
      Self::Fallback {
        writer,
        presented_initial,
      } => {
        if final_render || *presented_initial {
          return Ok(false);
        }
        let width = config.width.unwrap_or(100).max(2) - 1;
        let height = config.height.unwrap_or(100).max(1);
        let frame = render_frame(
          stream.engine().state(),
          config,
          now,
          width,
          height,
          false,
        );
        if config.ansi {
          writeln!(writer, "{}", frame.ansi_text())?;
        } else {
          writeln!(writer, "{}", frame.text())?;
        }
        writer.flush()?;
        *presented_initial = true;
        Ok(true)
      },
      Self::Append(_) => Ok(false),
    }
  }

  fn render_initial(
    &mut self,
    stream: &StreamEngine,
    config: &RenderConfig,
    now: f64,
  ) -> io::Result<bool> {
    match self {
      Self::Live(terminal) => {
        terminal.render(stream.engine().state(), config, now, false)
      },
      Self::Fallback { .. } | Self::Append(_) => Ok(false),
    }
  }

  fn finish(
    &mut self,
    stream: &StreamEngine,
    config: &RenderConfig,
    silent: bool,
    now: f64,
  ) -> io::Result<()> {
    if silent {
      return Ok(());
    }

    match self {
      Self::Live(terminal) if !terminal.is_retired() => {
        let _ = terminal.render(stream.engine().state(), config, now, true)?;
        terminal.finish()
      },
      Self::Live(terminal) => {
        terminal.append_final(stream.engine().state(), config, now)
      },
      Self::Fallback { writer, .. } | Self::Append(writer) => {
        write_final(writer, stream.engine().state(), config, now)
      },
    }
  }
}

/// Monitor a Nix internal-JSON stream with ROM while the caller retains
/// ownership of the producer process and its exit status.
///
/// Direct terminals receive ROM's synchronized live presentation. Redirected
/// output and terminal multiplexers receive append-only logs plus a final
/// presentation.
///
/// # Errors
///
/// Returns an error if the stream cannot be read, decoded, or presented.
pub fn run<R: Read + Send + 'static>(reader: R) -> Result<()> {
  let engine = EngineConfig::default();
  let render_config = RenderConfig {
    ansi: io::stderr().is_terminal(),
    format: DisplayFormat::Tree,
    legend_style: LegendStyle::Table,
    summary_style: SummaryStyle::Concise,
    ..RenderConfig::default()
  };
  let silent = engine.silent;
  let history = BuildReportCache::new(BuildReportCache::default_cache_path());
  let mut stream = StreamEngine::new(engine);
  stream.engine_mut().set_resolver(FilesystemResolver);
  stream.engine_mut().load_history(&history);
  let mut presenter = Presenter::new(silent);
  let (receiver, reader_thread) = byte_reader(reader);
  let result = drive(&receiver, &mut stream, &mut presenter, &render_config);
  drop(receiver);

  reader_thread
    .join()
    .map_err(|_| eyre!("ROM input reader panicked"))?
    .wrap_err("ROM failed to read Nix output")?;
  result?;

  if let Err(error) = stream.engine().save_history(&history) {
    tracing::debug!(%error, "failed to save ROM build history");
  }
  Ok(())
}

fn byte_reader<R: Read + Send + 'static>(
  mut reader: R,
) -> (
  Receiver<io::Result<Vec<u8>>>,
  thread::JoinHandle<io::Result<()>>,
) {
  let (sender, receiver) = mpsc::sync_channel(128);
  let thread = thread::spawn(move || {
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
      match reader.read(&mut buffer) {
        Ok(0) => break,
        Ok(count) => {
          if sender.send(Ok(buffer[..count].to_vec())).is_err() {
            break;
          }
        },
        Err(error) => {
          let _ = sender.send(Err(error));
          break;
        },
      }
    }
    Ok(())
  });
  (receiver, thread)
}

fn drive(
  receiver: &Receiver<io::Result<Vec<u8>>>,
  stream: &mut StreamEngine,
  presenter: &mut Presenter,
  render_config: &RenderConfig,
) -> Result<()> {
  let mut dirty = false;
  let now = Instant::now();
  let mut last_frame = now.checked_sub(TIMER_INTERVAL).unwrap_or(now);
  let mut last_timer = Instant::now();
  let _ = presenter.render_initial(stream, render_config, current_time())?;

  loop {
    match receiver.recv_timeout(Duration::from_millis(25)) {
      Ok(Ok(bytes)) => {
        let Processed { changed, output } =
          stream.push_at(&bytes, current_time())?;
        let had_output = presenter.output(output, render_config)?;
        dirty |= changed || had_output;
      },
      Ok(Err(error)) => return Err(error.into()),
      Err(RecvTimeoutError::Disconnected) => break,
      Err(RecvTimeoutError::Timeout) => {},
    }

    let timer_due = last_timer.elapsed() >= TIMER_INTERVAL;
    if !stream.engine().config().silent
      && (dirty || timer_due)
      && last_frame.elapsed() >= FRAME_INTERVAL
    {
      let _ = presenter.render(stream, render_config, current_time(), false)?;
      dirty = false;
      last_frame = Instant::now();
      if timer_due {
        last_timer = Instant::now();
      }
    }
  }

  let final_output = stream.finish_at(current_time())?;
  let _ = presenter.output(final_output.output, render_config)?;
  presenter.finish(
    stream,
    render_config,
    stream.engine().config().silent,
    current_time(),
  )?;
  Ok(())
}
