//! Plugin system for extending NH functionality with Lua scripts
//!
//! This module provides facilities for loading and executing Lua plugins,
//! managing plugin lifecycle, and exposing NH functionality to plugins.

mod api;
mod config;
mod hooks;
mod manager;
mod plugin;
mod types;

pub use api::register_api;
pub use config::PluginConfig;
pub use hooks::{Hook, HookManager};
pub use manager::PluginManager;
pub use plugin::Plugin;
pub use types::{PluginContext, PluginEvent, PluginMetadata, PluginResult};

use crate::Result;

/// Initialize the plugin system
pub fn init() -> Result<PluginManager> {
    let manager = PluginManager::new()?;
    manager.load_plugins()?;
    Ok(manager)
}
