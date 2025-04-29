//! Plugin manager responsible for loading and managing plugins

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use color_eyre::eyre::{bail, eyre};
use mlua::Lua;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::plugins::{
    HookManager, Plugin, PluginConfig, PluginContext, PluginEvent, PluginMetadata, PluginResult,
};
use crate::Result;

/// Manager for handling plugins lifecycle
#[derive(Debug)]
pub struct PluginManager {
    /// Lua state
    lua: Arc<Lua>,
    /// Loaded plugins
    plugins: Mutex<HashMap<String, Plugin>>,
    /// Plugin hooks
    hooks: HookManager,
    /// Plugin config
    config: PluginConfig,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Result<Self> {
        // Create a new Lua state with standard libraries
        let lua = Lua::new();

        let hooks = HookManager::new();
        let config = PluginConfig::load()?;

        Ok(Self {
            lua: Arc::new(lua),
            plugins: Mutex::new(HashMap::new()),
            hooks,
            config,
        })
    }

    /// Get all plugin directories
    fn get_plugin_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // Add user config directory
        if let Some(config_dir) = dirs::config_dir() {
            let plugin_dir = config_dir.join("nh").join("plugins");
            dirs.push(plugin_dir);
        }

        // Add XDG_DATA_HOME/nh/plugins if available
        if let Some(data_dir) = dirs::data_dir() {
            let plugin_dir = data_dir.join("nh").join("plugins");
            dirs.push(plugin_dir);
        }

        // Add system-wide plugins directory
        dirs.push(PathBuf::from("/etc/nh/plugins"));

        // Add custom directories from config
        dirs.extend(self.config.plugin_dirs.clone());

        dirs
    }

    /// Load all plugins from plugin directories
    pub fn load_plugins(&self) -> Result<()> {
        let plugin_dirs = self.get_plugin_dirs();

        for dir in plugin_dirs {
            if !dir.exists() {
                debug!("Plugin directory doesn't exist: {:?}", dir);
                continue;
            }

            info!("Loading plugins from {:?}", dir);
            if let Err(e) = self.load_plugins_from_dir(&dir) {
                warn!("Failed to load plugins from {}: {}", dir.display(), e);
            }
        }

        Ok(())
    }

    /// Load plugins from a specific directory
    pub fn load_plugins_from_dir(&self, dir: &Path) -> Result<()> {
        for entry in WalkDir::new(dir)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();

            // Check if this is a plugin directory with init.lua
            if path.is_dir() && path.join("init.lua").exists() {
                if let Err(e) = self.load_plugin_from_dir(path) {
                    warn!("Failed to load plugin from {}: {}", path.display(), e);
                }
                continue;
            }

            // Check if this is a standalone .lua file
            if path.extension().map_or(false, |ext| ext == "lua") {
                // Skip init.lua files at the root level
                if path.file_name().map_or(false, |name| name == "init.lua") {
                    continue;
                }

                if let Err(e) = self.load_plugin_from_file(path) {
                    warn!("Failed to load plugin from {}: {}", path.display(), e);
                }
            }
        }

        Ok(())
    }

    /// Load a plugin from a directory (must contain init.lua)
    pub fn load_plugin_from_dir(&self, dir: &Path) -> Result<()> {
        let plugin_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("Invalid plugin directory name"))?
            .to_string();

        info!("Loading plugin from directory: {}", plugin_name);

        let init_path = dir.join("init.lua");
        if !init_path.exists() {
            bail!("Plugin directory {} has no init.lua", plugin_name);
        }

        self.load_plugin(&plugin_name, &init_path)
    }

    /// Load a plugin from a single file
    pub fn load_plugin_from_file(&self, file: &Path) -> Result<()> {
        let plugin_name = file
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| eyre!("Invalid plugin file name"))?
            .to_string();

        info!("Loading plugin from file: {}", plugin_name);
        self.load_plugin(&plugin_name, file)
    }

    /// Load a plugin by name and path
    pub fn load_plugin(&self, name: &str, path: &Path) -> Result<()> {
        // Skip disabled plugins
        if self.config.is_plugin_disabled(name) {
            warn!("Plugin {} is disabled, skipping", name);
            return Ok(());
        }

        let plugin = Plugin::new(
            name.to_string(),
            path.to_path_buf(),
            Arc::clone(&self.lua),
            &self.hooks,
        )?;

        // Store the plugin
        let mut plugins = self.plugins.lock().unwrap();
        plugins.insert(name.to_string(), plugin);

        Ok(())
    }

    /// Get access to the Lua state
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Trigger a hook and execute all registered callbacks
    pub fn trigger_hook(&self, event: &PluginEvent, context: &mut PluginContext) -> PluginResult {
        self.hooks.trigger_hook(event, self.lua(), context)
    }

    /// Get a list of all loaded plugins
    pub fn get_plugins(&self) -> Vec<PluginMetadata> {
        let plugins = self.plugins.lock().unwrap();
        plugins.values().map(|p| p.get_metadata().clone()).collect()
    }

    /// Get a specific plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<PluginMetadata> {
        let plugins = self.plugins.lock().unwrap();
        plugins.get(name).map(|p| p.get_metadata().clone())
    }

    /// Reload a specific plugin
    pub fn reload_plugin(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.lock().unwrap();

        if let Some(plugin) = plugins.get_mut(name) {
            plugin.reload(&self.hooks)?;
            info!("Successfully reloaded plugin: {}", name);
            Ok(())
        } else {
            bail!("Plugin {} not found", name)
        }
    }

    /// Reload all plugins
    pub fn reload_all_plugins(&self) -> Result<()> {
        let mut plugins = self.plugins.lock().unwrap();

        for (name, plugin) in plugins.iter_mut() {
            if let Err(e) = plugin.reload(&self.hooks) {
                warn!("Failed to reload plugin {}: {}", name, e);
            }
        }

        Ok(())
    }

    /// Enable a plugin
    pub fn enable_plugin(&self, name: &str) -> Result<()> {
        // Update config
        let mut updated_config = self.config.clone();
        let disable_index = updated_config
            .disabled_plugins
            .iter()
            .position(|p| p == name);
        if let Some(index) = disable_index {
            updated_config.disabled_plugins.remove(index);

            // Save changes to disk
            if let Err(e) = updated_config.save() {
                warn!("Failed to save plugin configuration: {}", e);
            }
        }

        // If the plugin is not loaded, load it
        let plugins = self.plugins.lock().unwrap();
        if !plugins.contains_key(name) {
            drop(plugins); // release the lock

            // Find and load the plugin
            for dir in self.get_plugin_dirs() {
                let plugin_file = dir.join(format!("{name}.lua"));
                if plugin_file.exists() {
                    return self.load_plugin(name, &plugin_file);
                }

                let plugin_dir = dir.join(name);
                let init_file = plugin_dir.join("init.lua");
                if init_file.exists() {
                    return self.load_plugin(name, &init_file);
                }
            }

            bail!("Could not find plugin {} in any plugin directory", name);
        }

        Ok(())
    }

    /// Disable a plugin
    pub fn disable_plugin(&self, name: &str) -> Result<()> {
        // Update config
        let mut updated_config = self.config.clone();
        if !updated_config.disabled_plugins.contains(&name.to_string()) {
            updated_config.disabled_plugins.push(name.to_string());

            // Save changes to disk
            if let Err(e) = updated_config.save() {
                warn!("Failed to save plugin configuration: {}", e);
            }
        }

        // Remove the plugin if it's loaded
        let mut plugins = self.plugins.lock().unwrap();
        if plugins.contains_key(name) {
            plugins.remove(name);
            info!("Removed plugin {} from active plugins", name);
        }

        Ok(())
    }
}
