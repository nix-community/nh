use std::fmt;
use std::path::PathBuf;

use mlua::{Lua, Table};

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

/// Command category to control which commands trigger hooks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    /// Regular user command
    User,
    /// System/administrative command that should not trigger regular hooks
    System,
    /// Plugin management command that should not trigger hooks at all
    Plugin,
    /// Any command category
    Any,
}

/// Plugin event types that can be listened to
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PluginEvent {
    /// Before a command is executed
    PreCommand {
        /// Command name
        command: String,
        /// Command category
        category: CommandCategory,
    },
    /// After a command is executed
    PostCommand {
        /// Command name
        command: String,
        /// Command category
        category: CommandCategory,
    },
    /// Before NH exits
    BeforeExit,
    /// On plugin load
    OnLoad,
    /// On plugin unload
    OnUnload,
    /// When configuration is loaded or changed
    ConfigChanged,
    /// Generic system event
    System(String),
    /// Custom event
    Custom(String),
}

impl PluginEvent {
    /// Check if this event should trigger for a given command and category
    pub fn should_trigger_for(&self, command: &str, category: &CommandCategory) -> bool {
        match self {
            // Never trigger hooks for plugin management commands
            _ if *category == CommandCategory::Plugin => false,

            // For pre/post command events, check if command and category match
            Self::PreCommand {
                command: cmd,
                category: cat,
            } => {
                (cmd.is_empty() || cmd == command)
                    && (*cat == CommandCategory::Any || cat == category)
            }
            Self::PostCommand {
                command: cmd,
                category: cat,
            } => {
                (cmd.is_empty() || cmd == command)
                    && (*cat == CommandCategory::Any || cat == category)
            }

            // Other events don't depend on command or category
            _ => true,
        }
    }
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
pub struct PluginContext {
    /// Command being executed
    pub command: Option<String>,
    /// Arguments for the command
    pub args: Vec<String>,
    /// Extra context data
    pub data: Table,
    /// Command category
    pub category: CommandCategory,
}

impl PluginContext {
    /// Create a new plugin context with specified category
    pub fn with_category(
        lua: &Lua,
        command: Option<String>,
        args: Vec<String>,
        category: CommandCategory,
    ) -> Self {
        let data = lua
            .create_table()
            .expect("Failed to create context data table");

        Self {
            command,
            args,
            data,
            category,
        }
    }
}
