pub mod args;
pub mod checks;
pub mod command;
pub mod installable;
pub mod update;
pub mod util;

pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NH_REV: Option<&str> = option_env!("NH_REV");
