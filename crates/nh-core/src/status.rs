//! Build-scoped terminal status reporting.
//!
//! Publishes monitored-build progress to stderr as OSC 9;4. Whether the
//! terminal supports it is decided by `anstyle-progress`; this module only
//! applies the output-TTY gate and the user's mode. Nothing here queries or
//! reads the terminal.

use std::{
  io::{self, IsTerminal, Write},
  time::{Duration, Instant},
};

use anstyle_progress::{TermProgress, supports_term_progress};
use tracing::debug;

use crate::args::StatusMode;

/// Shortest gap between two emitted working frames. Clears are never limited.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// Whether a build reports, given whether stderr is a terminal.
///
/// Reports never leave a non-terminal stderr. `Auto` defers the support
/// decision entirely to `anstyle-progress`.
fn selected(mode: StatusMode, stderr_tty: bool) -> bool {
  match mode {
    StatusMode::Auto => supports_term_progress(stderr_tty),
    StatusMode::Always => stderr_tty,
    StatusMode::Never => false,
  }
}

/// A validated percentage for a counted build, if one is meaningful.
///
/// Returns `None` for unknown or zero totals, and for a complete build, so the
/// caller keeps reporting indeterminate rather than a synthetic 100%.
fn counted_percent(done: u64, expected: u64) -> Option<u8> {
  if expected == 0 || done >= expected {
    return None;
  }
  // 128-bit math keeps `done * 100` from overflowing for large counts. The
  // quotient is below 100 because `done < expected`.
  u8::try_from(u128::from(done) * 100 / u128::from(expected)).ok()
}

/// Build-scoped status reporter.
///
/// Owns the writer and the selection. Working frames are deduplicated and rate
/// limited; clears are not. `finish` and drop clear the indicator only when
/// this reporter has written a working frame, so a reporter that never
/// reported, or whose writes failed, never clears anything.
pub struct BuildStatus<W: Write> {
  writer:             W,
  enabled:            bool,
  /// Percentage and time of the last working frame. `None` until one is
  /// written, which is also when nothing is owned and nothing needs clearing.
  last:               Option<(Option<u8>, Instant)>,
  min_frame_interval: Duration,
  finished:           bool,
}

impl<W: Write> BuildStatus<W> {
  /// Selects reporting from the mode and whether stderr is a terminal.
  ///
  /// Selection writes nothing. The first report happens at the first render,
  /// which runs after the monitored child has been spawned.
  pub(crate) fn detect(writer: W, mode: StatusMode) -> Self {
    let enabled = selected(mode, io::stderr().is_terminal());
    debug!(?mode, enabled, "terminal status selection");
    Self::new(writer, enabled)
  }

  /// Builds a reporter with an explicit selection.
  const fn new(writer: W, enabled: bool) -> Self {
    Self {
      writer,
      enabled,
      last: None,
      min_frame_interval: MIN_FRAME_INTERVAL,
      finished: false,
    }
  }

  /// Reports the monitored build as working, with a percentage only when it is
  /// both known and still in progress.
  pub(crate) fn build_progress(&mut self, done: u64, expected: u64) {
    self.working(counted_percent(done, expected));
  }

  /// Stops reporting and clears owned state, retrying a failed clear on drop
  /// or a later call. A successful clear is never repeated.
  pub(crate) fn finish(&mut self) {
    self.finished = true;
    if self.last.is_some() && self.write(TermProgress::remove()) {
      self.last = None;
    }
  }

  /// Writes a working frame unless it repeats the last one or arrives within
  /// the rate window.
  fn working(&mut self, percent: Option<u8>) {
    let repeated = self.last.is_some_and(|(last, at)| {
      last == percent || at.elapsed() < self.min_frame_interval
    });
    if self.finished || !self.enabled || repeated {
      return;
    }
    let progress = percent.map_or_else(TermProgress::start, |percent| {
      TermProgress::start().percent(percent)
    });
    if self.write(progress) {
      self.last = Some((percent, Instant::now()));
    }
  }

  /// Writes one sequence, reporting whether the bytes were accepted.
  fn write(&mut self, progress: TermProgress) -> bool {
    let bytes = progress.to_string();
    if self.writer.write_all(bytes.as_bytes()).is_err() {
      return false;
    }
    // A failed flush cannot retract bytes the terminal has already received.
    let _ = self.writer.flush();
    true
  }
}

impl<W: Write> Drop for BuildStatus<W> {
  fn drop(&mut self) {
    self.finish();
  }
}

#[cfg(test)]
mod tests {
  #![allow(clippy::expect_used, clippy::unwrap_used, reason = "Fine in tests")]

  use std::{
    cell::{Cell, RefCell},
    rc::Rc,
  };

  use super::*;

  /// Writer that shares its captured bytes with the test.
  #[derive(Clone, Default)]
  struct SharedWriter {
    bytes:           Rc<RefCell<Vec<u8>>>,
    fail_next_write: Rc<Cell<bool>>,
  }

  impl SharedWriter {
    fn text(&self) -> String {
      String::from_utf8(self.bytes.borrow().clone()).expect("utf-8")
    }
  }

  impl Write for SharedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
      if self.fail_next_write.replace(false) {
        return Err(io::ErrorKind::WouldBlock.into());
      }
      self.bytes.borrow_mut().extend_from_slice(buffer);
      Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  /// Writer that rejects every write.
  struct FailingWriter;

  impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
      Err(io::Error::other("terminal closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
  }

  #[test]
  fn counted_percent_is_safe_at_edges() {
    assert_eq!(counted_percent(0, 0), None);
    assert_eq!(counted_percent(5, 0), None);
    assert_eq!(counted_percent(0, 5), Some(0));
    assert_eq!(counted_percent(4, 5), Some(80));
    // A finished build must not report a synthetic 100%.
    assert_eq!(counted_percent(5, 5), None);
    assert_eq!(counted_percent(6, 5), None);
    // No overflow for counts near u64::MAX.
    assert_eq!(counted_percent(u64::MAX - 1, u64::MAX), Some(99));
  }

  #[test]
  fn selection_never_reports_to_a_non_terminal_or_when_disabled() {
    assert!(selected(StatusMode::Always, true));
    assert!(!selected(StatusMode::Always, false));
    assert!(!selected(StatusMode::Never, true));
    // `Auto` on a non-terminal returns before any environment is read.
    assert!(!selected(StatusMode::Auto, false));
  }

  #[test]
  fn working_frames_encode_progress_and_clear() {
    let writer = SharedWriter::default();
    let mut status = BuildStatus::new(writer.clone(), true);
    status.min_frame_interval = Duration::ZERO;

    status.build_progress(0, 0);
    assert_eq!(writer.text(), "\x1b]9;4;3;\x1b\\");

    status.build_progress(1, 2);
    assert!(writer.text().ends_with("\x1b]9;4;1;50\x1b\\"));

    // A completed count stays busy rather than reporting a synthetic 100%.
    status.build_progress(2, 2);
    assert!(writer.text().ends_with("\x1b]9;4;3;\x1b\\"));

    status.finish();
    assert!(writer.text().ends_with("\x1b]9;4;0;\x1b\\"));
  }

  #[test]
  fn working_frames_are_deduplicated_and_rate_limited() {
    let writer = SharedWriter::default();
    let mut status = BuildStatus::new(writer.clone(), true);
    status.min_frame_interval = Duration::from_secs(3600);

    status.build_progress(0, 0);
    // A busy-to-counted transition inside the window is deferred, not dropped.
    status.build_progress(1, 4);
    assert_eq!(occurrences(&writer.text(), "9;4;3;"), 1);
    assert_eq!(occurrences(&writer.text(), "9;4;1;"), 0);

    status.min_frame_interval = Duration::ZERO;
    status.build_progress(1, 4);
    assert_eq!(occurrences(&writer.text(), "9;4;1;25"), 1);
    // An unchanged repeat is deduplicated.
    status.build_progress(1, 4);
    assert_eq!(occurrences(&writer.text(), "9;4;1;25"), 1);
  }

  #[test]
  fn finish_is_idempotent_and_drop_clears_once() {
    let writer = SharedWriter::default();
    {
      let mut status = BuildStatus::new(writer.clone(), true);
      status.build_progress(1, 2);
      status.finish();
      status.finish();
      status.build_progress(1, 2);
    }
    assert_eq!(occurrences(&writer.text(), "9;4;0;"), 1);

    let writer = SharedWriter::default();
    {
      let mut status = BuildStatus::new(writer.clone(), true);
      status.build_progress(1, 2);
    }
    assert_eq!(occurrences(&writer.text(), "9;4;0;"), 1);
  }

  #[test]
  fn failed_clear_is_retried_without_resuming_progress() {
    for retry_on_drop in [false, true] {
      let writer = SharedWriter::default();
      {
        let mut status = BuildStatus::new(writer.clone(), true);
        status.min_frame_interval = Duration::ZERO;
        status.build_progress(1, 2);
        writer.fail_next_write.set(true);

        status.finish();
        assert!(status.finished);
        assert!(status.last.is_some());
        assert_eq!(occurrences(&writer.text(), "9;4;0;"), 0);

        status.build_progress(0, 0);
        assert_eq!(writer.text(), "\x1b]9;4;1;50\x1b\\");

        if !retry_on_drop {
          status.finish();
          assert!(status.last.is_none());
          status.finish();
        }
      }
      assert_eq!(occurrences(&writer.text(), "9;4;0;"), 1);
    }
  }

  #[test]
  fn reporter_that_never_wrote_clears_nothing() {
    let writer = SharedWriter::default();
    {
      let _status = BuildStatus::new(writer.clone(), true);
    }
    assert_eq!(writer.text(), "");
  }

  #[test]
  fn failed_writes_are_never_treated_as_owned() {
    let mut status = BuildStatus::new(FailingWriter, true);
    status.build_progress(1, 2);
    assert!(status.last.is_none());
    status.finish();
  }

  #[test]
  fn disabled_reporter_emits_nothing() {
    let writer = SharedWriter::default();
    {
      let mut status = BuildStatus::new(writer.clone(), false);
      status.build_progress(1, 2);
      status.finish();
    }
    assert_eq!(writer.text(), "");
  }
}
