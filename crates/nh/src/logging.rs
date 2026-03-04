use clap_verbosity_flag::InfoLevel;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::{
  EnvFilter,
  filter::LevelFilter,
  fmt::{self, FormatEvent, FormatFields},
  prelude::*,
  registry::LookupSpan,
};
use yansi::{Color, Paint};

use crate::Result;

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

    if *level != Level::INFO {
      if let (Some(file), Some(line)) = (metadata.file(), metadata.line()) {
        write!(writer, " (nh/{file}:{line})")?;
      }
    }

    writeln!(writer)?;
    Ok(())
  }
}

pub fn setup_logging(
  verbosity: clap_verbosity_flag::Verbosity<InfoLevel>,
) -> Result<()> {
  color_eyre::config::HookBuilder::default()
    .display_location_section(true)
    .panic_section(
      "Please report the bug at https://github.com/nix-community/nh/issues",
    )
    .display_env_section(false)
    .install()?;

  let fallback_level =
    verbosity.log_level().map_or(LevelFilter::WARN, |level| {
      match level {
        clap_verbosity_flag::log::Level::Error => LevelFilter::ERROR,
        clap_verbosity_flag::log::Level::Warn => LevelFilter::WARN,
        clap_verbosity_flag::log::Level::Info => LevelFilter::INFO,
        clap_verbosity_flag::log::Level::Debug => LevelFilter::DEBUG,
        clap_verbosity_flag::log::Level::Trace => LevelFilter::TRACE,
      }
    });

  let layer = fmt::layer()
    .with_writer(std::io::stderr)
    .without_time()
    .compact()
    .with_line_number(true)
    .event_format(InfoFormatter)
    .with_filter(
      EnvFilter::from_env("NH_LOG").add_directive(fallback_level.into()),
    );

  tracing_subscriber::registry().with(layer).init();

  tracing::trace!("Logging OK");

  Ok(())
}
