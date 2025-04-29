use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Context};
use mlua::{Function, Lua, Table};
use tracing::{debug, error, info};

use crate::plugins::{CommandCategory, Hook, HookManager, PluginEvent, PluginMetadata};
use crate::Result;

/// Represents a loaded plugin
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Plugin metadata
    pub metadata: PluginMetadata,
    /// Lua state reference
    lua: Arc<Lua>,
}

impl Plugin {
    /// Create a new plugin instance
    pub fn new(
        name: String,
        path: PathBuf,
        lua: Arc<Lua>,
        hook_manager: &HookManager,
    ) -> Result<Self> {
        let code = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read plugin file: {path:?}"))?;

        // Create the plugin's namespace table
        let globals = lua.globals();

        let plugin_table = match lua.create_table() {
            Ok(t) => t,
            Err(e) => return Err(eyre!("Failed to create plugin table: {}", e)),
        };

        if let Err(e) = globals.set(name.clone(), plugin_table.clone()) {
            return Err(eyre!("Failed to set plugin table in globals: {}", e));
        }

        // Set the plugin metadata in Lua
        let metadata_table = match lua.create_table() {
            Ok(t) => t,
            Err(e) => return Err(eyre!("Failed to create metadata table: {}", e)),
        };

        if let Err(e) = metadata_table.set("name", name.clone()) {
            return Err(eyre!("Failed to set name in metadata: {}", e));
        }

        if let Err(e) = metadata_table.set("path", path.to_string_lossy().to_string()) {
            return Err(eyre!("Failed to set path in metadata: {}", e));
        }

        if let Err(e) = plugin_table.set("__metadata", metadata_table) {
            return Err(eyre!("Failed to set metadata in plugin table: {}", e));
        }

        // Compile and run the plugin code
        if let Err(e) = lua.load(&code).set_name(format!("plugin_{name}")).exec() {
            return Err(eyre!("Failed to execute plugin code: {}", e));
        }

        // Extract metadata from the plugin
        let metadata = Self::extract_metadata(&lua, &name, &path)?;

        // Register plugin hooks
        Self::register_hooks(&lua, &name, hook_manager)?;

        // Invoke the plugin's init function if it exists
        let plugin_table: Table = match globals.get(name.clone()) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to get plugin table for init: {}", e);
                // Not a fatal error, continue without calling init
                return Ok(Self { metadata, lua });
            }
        };

        if let Ok(init_fn) = plugin_table.get::<Function>("init") {
            debug!("Calling init function for plugin {}", name);
            if let Err(e) = init_fn.call::<()>(()) {
                error!("Failed to call init function for {}: {}", name, e);
            }
        }

        Ok(Self { metadata, lua })
    }

    /// Extract plugin metadata from its Lua table
    fn extract_metadata(lua: &Lua, name: &str, path: &Path) -> Result<PluginMetadata> {
        let globals = lua.globals();

        let plugin_table: Table = match globals.get(name) {
            Ok(t) => t,
            Err(e) => return Err(eyre!("Failed to get plugin table: {}", e)),
        };

        let version: String = plugin_table
            .get("__VERSION")
            .unwrap_or_else(|_| "0.1.0".to_string());
        let description: String = plugin_table
            .get("__DESCRIPTION")
            .unwrap_or_else(|_| String::new());
        let author: String = plugin_table
            .get("__AUTHOR")
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(PluginMetadata {
            name: name.to_string(),
            version,
            description,
            author,
            path: path.to_path_buf(),
            enabled: true,
        })
    }

    /// Register plugin hooks with the hook manager
    fn register_hooks(lua: &Lua, plugin_name: &str, hook_manager: &HookManager) -> Result<()> {
        let globals = lua.globals();
        debug!("Registering hooks for plugin: {}", plugin_name);

        let plugin_table: Table = match globals.get(plugin_name) {
            Ok(t) => t,
            Err(e) => return Err(eyre!("Failed to get plugin table: {}", e)),
        };

        // Check if the plugin has hooks table
        let hooks_table: Table = match plugin_table.get("hooks") {
            Ok(t) => {
                debug!("Found hooks table for plugin {}", plugin_name);
                t
            }
            Err(e) => {
                debug!("No hooks found for plugin {}: {}", plugin_name, e);
                return Ok(());
            }
        };

        // Iterate through all hook pairs and register them
        for pair_result in hooks_table.pairs::<String, mlua::Value>() {
            match pair_result {
                Ok((event_name, hook_value)) => {
                    debug!("Processing hook '{}' for {}", event_name, plugin_name);

                    // Parse hook priority and parameters
                    let (priority, function) = match hook_value {
                        mlua::Value::Function(f) => {
                            debug!("Found function hook for event: {}", event_name);
                            (100, f) // Default priority
                        }
                        mlua::Value::Table(hook_table) => {
                            let func = match hook_table.get::<Function>("fn") {
                                Ok(f) => {
                                    debug!(
                                        "Found function in table hook for event: {}",
                                        event_name
                                    );
                                    f
                                }
                                Err(e) => {
                                    error!("Hook table missing 'fn' key for {}: {}", event_name, e);
                                    continue;
                                }
                            };

                            // Get priority if specified
                            let priority = hook_table.get::<i32>("priority").unwrap_or(100);

                            (priority, func)
                        }
                        _ => {
                            error!("Hook '{}' is not a function or table", event_name);
                            continue;
                        }
                    };

                    // Store the function in the plugin table with a unique name
                    let fn_name = format!("__hook_{event_name}");
                    if let Err(e) = plugin_table.set(fn_name.clone(), function) {
                        error!("Failed to store hook function: {}", e);
                        continue;
                    }

                    // Register the hook
                    match parse_event_name(&event_name) {
                        Ok(event) => {
                            debug!("Registering hook '{}' for event {:?}", fn_name, event);
                            hook_manager.register_hook(
                                event,
                                Hook {
                                    plugin_name: plugin_name.to_string(),
                                    function_name: fn_name,
                                    priority,
                                },
                            );
                        }
                        Err(e) => {
                            error!("Failed to parse event name '{}': {}", event_name, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to process hook: {}", e);
                }
            }
        }

        debug!("Finished registering hooks for plugin: {}", plugin_name);
        Ok(())
    }

    /// Get the metadata for this plugin
    pub const fn get_metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    /// Get the Lua state reference
    pub const fn get_lua(&self) -> &Arc<Lua> {
        &self.lua
    }

    /// Reload the plugin from its path
    pub fn reload(&mut self, hook_manager: &HookManager) -> Result<()> {
        let name = self.metadata.name.clone();
        let path = self.metadata.path.clone();

        info!("Reloading plugin: {}", name);

        // Create a new instance with the same name and path
        let new_plugin = Self::new(name, path, Arc::clone(&self.lua), hook_manager)?;

        // Update this instance with the new plugin's data
        self.metadata = new_plugin.metadata;

        Ok(())
    }

    /// Execute a Lua function from this plugin
    pub fn execute_function(&self, name: &str, args: Vec<mlua::Value>) -> Result<mlua::Value> {
        let globals = self.lua.globals();

        let plugin_table: Table = match globals.get(self.metadata.name.as_str()) {
            Ok(t) => t,
            Err(e) => return Err(eyre!("Failed to get plugin table: {}", e)),
        };

        let func: Function = match plugin_table.get(name) {
            Ok(f) => f,
            Err(e) => return Err(eyre!("Failed to get function '{}': {}", name, e)),
        };

        // Convert Vec<Value> to MultiValue and call the function
        let args = mlua::MultiValue::from_vec(args);
        match func.call(args) {
            Ok(result) => Ok(result),
            Err(e) => Err(eyre!("Failed to execute function '{}': {}", name, e)),
        }
    }

    /// Check if the plugin provides a specific function
    pub fn has_function(&self, name: &str) -> bool {
        let globals = self.lua.globals();

        let plugin_table = match globals.get::<Table>(self.metadata.name.as_str()) {
            Ok(t) => t,
            Err(_) => return false,
        };

        plugin_table.get::<Function>(name).is_ok()
    }
}

/// Parse event name into a `PluginEvent`
fn parse_event_name(name: &str) -> Result<PluginEvent> {
    debug!("Parsing event name: {}", name);
    match name {
        "on_load" => {
            debug!("Matched on_load event");
            Ok(PluginEvent::OnLoad)
        }
        "on_unload" => {
            debug!("Matched on_unload event");
            Ok(PluginEvent::OnUnload)
        }
        "before_exit" => {
            debug!("Matched before_exit event");
            Ok(PluginEvent::BeforeExit)
        }
        "config_changed" => {
            debug!("Matched config_changed event");
            Ok(PluginEvent::ConfigChanged)
        }
        "pre_command" => {
            debug!("Matched generic pre_command event");
            Ok(PluginEvent::PreCommand {
                command: String::new(),
                category: CommandCategory::User,
            })
        }
        "pre_command_user" => {
            debug!("Matched generic pre_command_user event");
            Ok(PluginEvent::PreCommand {
                command: String::new(),
                category: CommandCategory::User,
            })
        }
        "pre_command_system" => {
            debug!("Matched generic pre_command_system event");
            Ok(PluginEvent::PreCommand {
                command: String::new(),
                category: CommandCategory::System,
            })
        }
        "pre_command_any" => {
            debug!("Matched generic pre_command_any event");
            Ok(PluginEvent::PreCommand {
                command: String::new(),
                category: CommandCategory::Any,
            })
        }
        "post_command" => {
            debug!("Matched generic post_command event");
            Ok(PluginEvent::PostCommand {
                command: String::new(),
                category: CommandCategory::User,
            })
        }
        "post_command_user" => {
            debug!("Matched generic post_command_user event");
            Ok(PluginEvent::PostCommand {
                command: String::new(),
                category: CommandCategory::User,
            })
        }
        "post_command_system" => {
            debug!("Matched generic post_command_system event");
            Ok(PluginEvent::PostCommand {
                command: String::new(),
                category: CommandCategory::System,
            })
        }
        "post_command_any" => {
            debug!("Matched generic post_command_any event");
            Ok(PluginEvent::PostCommand {
                command: String::new(),
                category: CommandCategory::Any,
            })
        }
        s if s.starts_with("pre_command_") => {
            let command = s.strip_prefix("pre_command_").unwrap();
            debug!("Matched specific pre_command event for: {}", command);
            Ok(PluginEvent::PreCommand {
                command: command.to_string(),
                category: CommandCategory::User,
            })
        }
        s if s.starts_with("post_command_") => {
            let command = s.strip_prefix("post_command_").unwrap();
            debug!("Matched specific post_command event for: {}", command);
            Ok(PluginEvent::PostCommand {
                command: command.to_string(),
                category: CommandCategory::User,
            })
        }
        s if s.starts_with("system_") => {
            let event = s.strip_prefix("system_").unwrap();
            debug!("Matched system event: {}", event);
            Ok(PluginEvent::System(event.to_string()))
        }
        _ => {
            debug!("Matched custom event: {}", name);
            Ok(PluginEvent::Custom(name.to_string()))
        }
    }
}
