//! Internal library output for NH. This is not meant for public consumption.
pub mod checks;
pub mod clean;
pub mod commands;
pub mod completion;
pub mod darwin;
pub mod generations;
pub mod home;
pub mod installable;
pub mod interface;
pub mod json;
pub mod logging;
pub mod nixos;
pub mod notify;
pub mod search;
pub mod update;
pub mod util;

pub use color_eyre::Result;

pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NH_REV: Option<&str> = option_env!("NH_REV");
