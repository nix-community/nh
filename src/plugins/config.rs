use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Plugin configuration
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct PluginConfig {
    /// List of plugin directories to search for plugins
    #[serde(default)]
    pub plugin_dirs: Vec<PathBuf>,

    /// Disabled plugins
    #[serde(default)]
    pub disabled_plugins: Vec<String>,

    /// Plugin-specific settings
    #[serde(default)]
    pub plugin_settings: HashMap<String, PluginSettings>,
}

/// Plugin-specific settings
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PluginSettings {
    /// Whether the plugin is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Plugin-specific configuration options
    #[serde(default)]
    pub config: HashMap<String, toml::Value>,
}

const fn default_true() -> bool {
    true
}

impl PluginConfig {
    /// Load plugin configuration from default locations
    pub fn load() -> Result<Self> {
        let config_paths = Self::get_config_paths();

        for path in config_paths {
            if path.exists() {
                debug!("Loading plugin config from {:?}", path);
                return Self::load_from_file(&path);
            }
        }

        debug!("No plugin config found, using defaults");
        Ok(Self::default())
    }

    /// Load plugin configuration from a specific file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Self =
            toml::from_str(&content).map_err(|e| eyre!("Failed to parse config file: {}", e))?;
        Ok(config)
    }

    /// Get potential config file paths
    fn get_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // Add XDG config directory
        if let Some(config_dir) = dirs::config_dir() {
            paths.push(config_dir.join("nh").join("plugins.toml"));
        }

        // Add home directory
        if let Some(home_dir) = dirs::home_dir() {
            paths.push(home_dir.join(".config/nh/plugins.toml"));
        }

        // Add system-wide config. Might be useful for the NixOS module
        // for NH.
        paths.push(PathBuf::from("/etc/nh/plugins.toml"));

        paths
    }

    /// Check if a plugin is disabled
    pub fn is_plugin_disabled(&self, name: &str) -> bool {
        self.disabled_plugins.iter().any(|p| p == name)
            || self
                .plugin_settings
                .get(name)
                .map_or(false, |settings| !settings.enabled)
    }
}
