//! Hook system for plugins to integrate with NH

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mlua::{Function, Lua, Table};
use tracing::{debug, error};

use crate::plugins::{CommandCategory, PluginContext, PluginEvent, PluginResult};

/// Hook type for plugin callbacks
#[derive(Debug, Clone)]
pub struct Hook {
    /// Plugin that owns this hook
    pub plugin_name: String,
    /// Lua function to call for this hook
    pub function_name: String,
    /// Priority of this hook (lower runs first)
    pub priority: i32,
}

/// Manager for plugin hooks
#[derive(Debug, Default)]
pub struct HookManager {
    /// Map of event types to hooks
    hooks: Arc<RwLock<HashMap<PluginEvent, Vec<Hook>>>>,
}

impl HookManager {
    /// Create a new hook manager
    pub fn new() -> Self {
        Self {
            hooks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new hook
    pub fn register_hook(&self, event: PluginEvent, hook: Hook) {
        let mut hooks = self.hooks.write().unwrap();
        let hooks_for_event = hooks.entry(event).or_default();

        // Add hook and sort by priority
        hooks_for_event.push(hook);
        hooks_for_event.sort_by_key(|h| h.priority);
    }

    /// Find matching hooks for an event
    fn find_matching_hooks(
        &self,
        _event: &PluginEvent,
        command: &str,
        category: &CommandCategory,
    ) -> Vec<Hook> {
        let hooks_guard = self.hooks.read().unwrap();

        let mut matching_hooks = Vec::new();

        // Collect all hooks that should trigger for this event
        for (hook_event, hooks) in hooks_guard.iter() {
            if hook_event.should_trigger_for(command, category) {
                matching_hooks.extend(hooks.clone());
            }
        }

        matching_hooks.sort_by_key(|h| h.priority);
        matching_hooks
    }

    /// Trigger a hook and execute all registered callbacks
    pub fn trigger_hook(
        &self,
        event: &PluginEvent,
        lua: &Lua,
        context: &mut PluginContext,
    ) -> PluginResult {
        let command = context.command.as_ref().map_or("", String::as_str);

        tracing::debug!(
            "Triggering hook for event: {:?}, command: {}, category: {:?}",
            event,
            command,
            context.category
        );

        // If this is a plugin management command, don't trigger hooks
        if matches!(context.category, CommandCategory::Plugin) {
            debug!("Skipping hooks for plugin management command");
            return PluginResult::Continue;
        }

        // Find all hooks that should trigger for this event
        let hooks = self.find_matching_hooks(event, command, &context.category);

        if hooks.is_empty() {
            return PluginResult::Continue;
        }

        for hook in hooks {
            // Clone these strings before using them
            let plugin_name = hook.plugin_name.clone();
            let function_name = hook.function_name.clone();

            // Get the plugin's global table
            let globals = lua.globals();

            // Get the plugin's function
            let plugin_table: Table = match globals.get(plugin_name.clone()) {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to get plugin table for {}: {}", plugin_name, e);
                    continue;
                }
            };

            // Get the actual function
            let func: Function = match plugin_table.get(function_name.clone()) {
                Ok(f) => f,
                Err(e) => {
                    error!(
                        "Failed to get function {} for plugin {}: {}",
                        function_name, plugin_name, e
                    );
                    continue;
                }
            };

            // Create table for arguments
            let args_table = match lua.create_table() {
                Ok(t) => t,
                Err(e) => {
                    error!("Failed to create args table: {}", e);
                    continue;
                }
            };

            // Add args to the table
            for (i, arg) in context.args.iter().enumerate() {
                if let Err(e) = args_table.set(i + 1, arg.clone()) {
                    error!("Failed to set arg: {}", e);
                }
            }

            // Create a category string
            let category_str = match context.category {
                CommandCategory::User => "user",
                CommandCategory::System => "system",
                CommandCategory::Plugin => "plugin",
                CommandCategory::Any => "any",
            };

            let cmd = context.command.clone().unwrap_or_default();

            // Call the function with command, args table, category and data
            match func.call((cmd, args_table, category_str, &context.data)) {
                Ok(mlua::Value::Boolean(false)) => {
                    return PluginResult::Skip;
                }
                Ok(mlua::Value::String(s)) => {
                    return PluginResult::Error(s.to_string_lossy());
                }
                Err(e) => {
                    error!(
                        "Error executing hook {} for plugin {}: {}",
                        function_name, plugin_name, e
                    );
                }
                _ => { /* Continue execution */ }
            }
        }

        PluginResult::Continue
    }
}
