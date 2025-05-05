pub use color_eyre::Result;

// Re-export constants from main
pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NH_REV: Option<&str> = option_env!("NH_REV");

// Self-elevate function (used by some modules)
// This is just a stub for compilation
pub fn self_elevate() -> ! {
    panic!("self_elevate should only be called from main.rs")
}

// Re-export modules
pub mod check;
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
pub mod search;
pub mod update;
pub mod util;

// Testing infrastructure
#[cfg(test)] pub mod testing;

// Tests
#[cfg(test)] pub mod tests;
