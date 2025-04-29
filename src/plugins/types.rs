use std::fmt;
use std::path::PathBuf;

use mlua::{Lua, Table, Value};

/// Plugin metadata
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: String,
    /// Plugin description
    pub description: String,
    /// Plugin author
    pub author: String,
    /// Plugin path
    pub path: PathBuf,
    /// Whether the plugin is enabled
    pub enabled: bool,
}

impl PluginMetadata {
    /// Return a formatted string with plugin information
    pub fn format_info(&self) -> String {
        format!(
            "{} v{} by {}\n{}\nPath: {}\nStatus: {}",
            self.name,
            self.version,
            self.author,
            if self.description.is_empty() {
                "No description provided"
            } else {
                &self.description
            },
            self.path.display(),
            if self.enabled { "Enabled" } else { "Disabled" }
        )
    }

    /// Check if the plugin is compatible with the given NH version
    pub const fn is_compatible_with(&self, _nh_version: &str) -> bool {
        // For now we return true, because we do not have a reason to have breaking changes. In the
        // future we will check supported versions more precisely.
        true
    }
}

impl fmt::Display for PluginMetadata {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} v{}", self.name, self.version)
    }
}

/// Plugin event types that can be listened to
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginEvent {
    /// Before a command is executed
    PreCommand(String),
    /// After a command is executed
    PostCommand(String),
    /// Before NH exits
    BeforeExit,
    /// On plugin load
    OnLoad,
    /// On plugin unload
    OnUnload,
    /// Custom event
    Custom(String),
}

/// Result of plugin execution
#[derive(Debug)]
pub enum PluginResult {
    /// Continue execution
    Continue,
    /// Skip the next step in the pipeline
    Skip,
    /// Stop execution and return an error
    Error(String),
}

/// Plugin execution context
pub struct PluginContext<'lua> {
    /// Lua state
    pub lua: &'lua Lua,
    /// Command being executed
    pub command: Option<String>,
    /// Arguments for the command
    pub args: Vec<String>,
    /// Extra context data
    pub data: Table,
}

impl<'lua> PluginContext<'lua> {
    /// Create a new plugin context
    pub fn new(lua: &'lua Lua, command: Option<String>, args: Vec<String>) -> Self {
        let data = lua
            .create_table()
            .expect("Failed to create context data table");
        Self {
            lua,
            command,
            args,
            data,
        }
    }

    /// Add a value to the context data
    pub fn add_data(&self, key: &str, value: Value) -> mlua::Result<()> {
        self.data.set(key, value)
    }

    /// Get a value from the context data
    pub fn get_data<T: mlua::FromLua>(&self, key: &str) -> mlua::Result<T> {
        self.data.get(key)
    }

    /// Convert the context to a Lua table
    pub fn to_lua_table(&self) -> mlua::Result<Table> {
        let ctx_table = self.lua.create_table()?;

        // Set command if available
        if let Some(cmd) = &self.command {
            ctx_table.set("command", cmd.clone())?;
        } else {
            ctx_table.set("command", mlua::Nil)?;
        }

        // Set arguments
        let args_table = self.lua.create_table()?;
        for (i, arg) in self.args.iter().enumerate() {
            args_table.set(i + 1, arg.clone())?;
        }
        ctx_table.set("args", args_table)?;

        // Set data
        ctx_table.set("data", self.data.clone())?;

        Ok(ctx_table)
    }

    /// Execute Lua code in this context
    pub fn execute_lua(&self, code: &str) -> mlua::Result<Value> {
        self.lua.load(code).eval()
    }
}
