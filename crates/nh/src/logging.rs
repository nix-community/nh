use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
  EnvFilter,
  filter::LevelFilter,
  fmt::{self, FormatEvent, FormatFields},
  prelude::*,
  registry::LookupSpan,
};
use yansi::{Color, Paint};

use crate::{Result, interface::Verbosity};

struct InfoFormatter;

impl<S, N> FormatEvent<S, N> for InfoFormatter
where
  S: Subscriber + for<'a> LookupSpan<'a>,
  N: for<'a> FormatFields<'a> + 'static,
{
  fn format_event(
    &self,
    ctx: &fmt::FmtContext<'_, S, N>,
    mut writer: fmt::format::Writer,
    event: &Event,
  ) -> std::fmt::Result {
    // Based on https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/trait.FormatEvent.html#examples
    // Without the unused parts
    let metadata = event.metadata();
    let level = metadata.level();

    match *level {
      Level::ERROR => {
        write!(writer, "{} ", Paint::new("ERROR").fg(Color::Red))?;
      },
      Level::WARN => write!(writer, "{} ", Paint::new("!").fg(Color::Yellow))?,
      Level::INFO => write!(writer, "{} ", Paint::new(">").fg(Color::Green))?,
      Level::DEBUG => {
        write!(writer, "{} ", Paint::new("DEBUG").fg(Color::Blue))?;
      },
      Level::TRACE => {
        write!(writer, "{} ", Paint::new("TRACE").fg(Color::Cyan))?;
      },
    }

    ctx.field_format().format_fields(writer.by_ref(), event)?;

    if *level != Level::INFO
      && let (Some(file), Some(line)) = (metadata.file(), metadata.line())
    {
      write!(writer, " (nh/{file}:{line})")?;
    }

    writeln!(writer)?;
    Ok(())
  }
}

/// Configure error reporting and tracing output.
///
/// # Errors
///
/// Returns an error if tracing filter directives cannot be parsed.
pub fn setup_logging(verbosity: Verbosity) -> Result<()> {
  misstep::install_panic_hook();
  let panic_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |info| {
    panic_hook(info);
    eprintln!(
      "Please report the bug at https://github.com/nix-community/nh/issues"
    );
  }));

  let fallback_level =
    match 3 + i64::from(verbosity.verbose) - i64::from(verbosity.quiet) {
      ..=1 => LevelFilter::ERROR,
      2 => LevelFilter::WARN,
      3 => LevelFilter::INFO,
      4 => LevelFilter::DEBUG,
      5.. => LevelFilter::TRACE,
    };

  let layer = fmt::layer()
    .with_writer(std::io::stderr)
    .without_time()
    .compact()
    .with_line_number(true)
    .event_format(InfoFormatter)
    .with_filter(
      EnvFilter::from_env("NH_LOG")
        .add_directive(fallback_level.into())
        .add_directive("dix=WARN".parse()?),
    );

  tracing_subscriber::registry().with(layer).init();

  tracing::trace!("Logging OK");

  Ok(())
}
