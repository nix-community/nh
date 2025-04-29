use std::env;
use std::fs;
use std::path::Path;

use color_eyre::eyre::{eyre, WrapErr};
use mlua::{Lua, Table, UserData, UserDataMethods, Value};
use tracing::{debug, error, info, warn};

use crate::commands::Command;
use crate::installable::Installable;
use crate::util;
use crate::Result;

// Centralized error mapping for registration failures.
fn registration_err<E: std::fmt::Display>(
    context: &str,
) -> impl FnOnce(E) -> color_eyre::Report + '_ {
    move |e| eyre!("Failed to register Lua API - {}: {}", context, e)
}

// Maps a Rust result/error into an `mlua::Error` for Lua FFI.
fn to_lua_err<T, E: std::fmt::Display>(result: std::result::Result<T, E>) -> mlua::Result<T> {
    result.map_err(|e| mlua::Error::external(e.to_string()))
}

// Register the NH API with Lua
pub fn register_api(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    let nh_table = lua
        .create_table()
        .map_err(registration_err("create nh table"))?;

    globals
        .set("nh", nh_table.clone())
        .map_err(registration_err("set nh global"))?;

    nh_table
        .set("version", crate::NH_VERSION)
        .map_err(registration_err("set version"))?;

    register_logging_api(lua, &nh_table).wrap_err("Failed to register logging API")?;
    register_command_api(lua, &nh_table).wrap_err("Failed to register command API")?;
    register_util_api(lua, &nh_table).wrap_err("Failed to register utility API")?;
    register_env_api(lua, &nh_table).wrap_err("Failed to register environment API")?;
    register_fs_api(lua, &nh_table).wrap_err("Failed to register filesystem API")?;

    Ok(())
}

/// Register logging API
fn register_logging_api(lua: &Lua, nh_table: &Table) -> Result<()> {
    let log_fn = lua
        .create_function(|_, (level, message): (String, String)| {
            match level.as_str() {
                "debug" => debug!("{}", message),
                "info" => info!("{}", message),
                "warn" => warn!("{}", message),
                "error" => error!("{}", message),
                _ => warn!("Unknown log level: {}. Message: {}", level, message),
            }
            Ok(())
        })
        .map_err(registration_err("create log function"))?;
    nh_table
        .set("log", log_fn)
        .map_err(registration_err("set log function"))?;

    macro_rules! create_log_fn {
        ($lua:expr, $name:ident, $log_macro:ident) => {
            $lua.create_function(|_, message: String| {
                $log_macro!("{}", message);
                Ok(())
            })
            .map_err(registration_err(concat!(
                "create ",
                stringify!($name),
                " function"
            )))
        };
    }

    nh_table
        .set("debug", create_log_fn!(lua, debug, debug)?)
        .map_err(registration_err("set debug function"))?;

    nh_table
        .set("info", create_log_fn!(lua, info, info)?)
        .map_err(registration_err("set info function"))?;

    nh_table
        .set("warn", create_log_fn!(lua, warn, warn)?)
        .map_err(registration_err("set warn function"))?;

    nh_table
        .set("error", create_log_fn!(lua, error, error)?)
        .map_err(registration_err("set error function"))?;

    Ok(())
}

/// Register command API
fn register_command_api(lua: &Lua, nh_table: &Table) -> Result<()> {
    let exec_fn = lua
        .create_function(|_, (cmd, args_table): (String, Table)| {
            let mut command = Command::new(cmd);
            let mut args_vec = Vec::new();

            for i in 1..=args_table.raw_len() {
                if let Ok(arg) = args_table.get::<String>(i) {
                    args_vec.push(arg);
                } else {
                    warn!("Non-string argument at index {} in exec call", i);
                }
            }

            command = command.args(args_vec);
            to_lua_err(command.run_capture())
        })
        .map_err(registration_err("create exec function"))?;
    nh_table
        .set("exec", exec_fn)
        .map_err(registration_err("set exec function"))?;

    #[derive(Debug, Clone)]
    struct LuaCommand {
        cmd: String,
        args: Vec<String>,
        message: Option<String>,
        elevate: bool,
        dry: bool,
    }

    impl LuaCommand {
        fn build_command(&self) -> Command {
            let mut command = Command::new(self.cmd.clone()).args(self.args.clone());

            if let Some(msg) = &self.message {
                command = command.message(msg.clone());
            }
            command = command.elevate(self.elevate);
            command = command.dry(self.dry);
            command
        }
    }

    impl UserData for LuaCommand {
        fn add_methods<'lua, M: UserDataMethods<Self>>(methods: &mut M) {
            methods.add_method_mut("arg", |_, this, arg: String| {
                this.args.push(arg);
                Ok(())
            });

            methods.add_method_mut("args", |_, this, args_table: Table| {
                for i in 1..=args_table.raw_len() {
                    if let Ok(arg) = args_table.get::<String>(i) {
                        this.args.push(arg);
                    } else {
                        warn!("Non-string argument at index {} in args() call", i);
                    }
                }
                Ok(())
            });

            methods.add_method_mut("message", |_, this, msg: String| {
                this.message = Some(msg);
                Ok(())
            });

            methods.add_method_mut("elevate", |_, this, elevate: bool| {
                this.elevate = elevate;
                Ok(())
            });

            methods.add_method_mut("dry", |_, this, dry: bool| {
                this.dry = dry;
                Ok(())
            });

            methods.add_method("run", |_, this, ()| {
                to_lua_err(this.build_command().run()).map(|()| true)
            });

            methods.add_method("run_capture", |_, this, ()| {
                to_lua_err(this.build_command().run_capture())
            });
        }
    }

    let command_fn = lua
        .create_function(|_, cmd: String| {
            Ok(LuaCommand {
                cmd,
                args: Vec::new(),
                message: None,
                elevate: false,
                dry: false,
            })
        })
        .map_err(registration_err("create command function"))?;
    nh_table
        .set("command", command_fn)
        .map_err(registration_err("set command function"))?;

    Ok(())
}

/// Convert Installable to a Lua table
fn installable_to_lua_table(lua: &Lua, installable: Installable) -> mlua::Result<Table> {
    let result = lua.create_table()?;
    let attr_table = lua.create_table()?;

    match installable {
        Installable::Flake {
            reference,
            attribute,
        } => {
            result.set("type", "flake")?;
            result.set("reference", reference)?;
            for (i, attr) in attribute.into_iter().enumerate() {
                attr_table.set(i + 1, attr)?;
            }
            result.set("attribute", attr_table)?;
        }
        Installable::File { path, attribute } => {
            result.set("type", "file")?;
            result.set("path", path.to_string_lossy().to_string())?;
            for (i, attr) in attribute.into_iter().enumerate() {
                attr_table.set(i + 1, attr)?;
            }
            result.set("attribute", attr_table)?;
        }
        Installable::Expression {
            expression,
            attribute,
        } => {
            result.set("type", "expression")?;
            result.set("expression", expression)?;
            for (i, attr) in attribute.into_iter().enumerate() {
                attr_table.set(i + 1, attr)?;
            }
            result.set("attribute", attr_table)?;
        }
        Installable::Store { path } => {
            result.set("type", "store")?;
            result.set("path", path.to_string_lossy().to_string())?;
            result.set("attribute", attr_table)?;
        }
    }

    Ok(result)
}

fn parse_installable_string(s: &str) -> color_eyre::Result<Installable> {
    use crate::installable::parse_attribute;

    if let Ok(p) = fs::canonicalize(s) {
        if p.starts_with("/nix/store") {
            return Ok(Installable::Store { path: p });
        }
    }

    let path = Path::new(s);
    if path.exists() && path.is_file() {
        return Ok(Installable::File {
            path: path.to_path_buf(),
            attribute: vec![],
        });
    }

    let mut elems = s.splitn(2, '#');
    let reference = elems.next().unwrap_or_default().to_owned();
    let attribute_str = elems.next().map(str::to_string).unwrap_or_default();

    Ok(Installable::Flake {
        reference,
        attribute: parse_attribute(attribute_str),
    })
}

/// Register utility API
fn register_util_api(lua: &Lua, nh_table: &Table) -> Result<()> {
    let hostname_fn = lua
        .create_function(|_, ()| to_lua_err(util::get_hostname()))
        .map_err(registration_err("create hostname function"))?;
    nh_table
        .set("hostname", hostname_fn)
        .map_err(registration_err("set hostname function"))?;

    let parse_installable_fn = lua
        .create_function(|lua, installable_str: String| {
            match parse_installable_string(&installable_str) {
                Ok(installable) => installable_to_lua_table(lua, installable),
                Err(e) => Err(mlua::Error::external(format!(
                    "Failed to parse installable '{installable_str}': {e}"
                ))),
            }
        })
        .map_err(registration_err("create parse_installable function"))?;
    nh_table
        .set("parse_installable", parse_installable_fn)
        .map_err(registration_err("set parse_installable function"))?;

    Ok(())
}

/// Register environment API
fn register_env_api(lua: &Lua, nh_table: &Table) -> Result<()> {
    let getenv_fn = lua
        .create_function(|_, name: String| Ok(env::var(name).ok()))
        .map_err(registration_err("create getenv function"))?;
    nh_table
        .set("getenv", getenv_fn)
        .map_err(registration_err("set getenv function"))?;

    let setenv_fn = lua
        .create_function(|_, (name, value): (String, String)| {
            env::set_var(name, value);
            Ok(())
        })
        .map_err(registration_err("create setenv function"))?;
    nh_table
        .set("setenv", setenv_fn)
        .map_err(registration_err("set setenv function"))?;

    let env_fn = lua
        .create_function(|lua, ()| {
            let env_table = lua.create_table()?;
            for (key, value) in env::vars() {
                env_table.set(key, value)?;
            }
            Ok(env_table)
        })
        .map_err(registration_err("create env function"))?;
    nh_table
        .set("env", env_fn)
        .map_err(registration_err("set env function"))?;

    Ok(())
}

/// Register filesystem API
fn register_fs_api(lua: &Lua, nh_table: &Table) -> Result<()> {
    let fs_table = lua
        .create_table()
        .map_err(registration_err("create fs table"))?;
    nh_table
        .set("fs", fs_table.clone())
        .map_err(registration_err("set fs table"))?;

    let exists_fn = lua
        .create_function(|_, path: String| Ok(Path::new(&path).exists()))
        .map_err(registration_err("create fs.exists function"))?;
    fs_table
        .set("exists", exists_fn)
        .map_err(registration_err("set fs.exists function"))?;

    let is_file_fn = lua
        .create_function(|_, path: String| Ok(Path::new(&path).is_file()))
        .map_err(registration_err("create fs.is_file function"))?;
    fs_table
        .set("is_file", is_file_fn)
        .map_err(registration_err("set fs.is_file function"))?;

    let is_dir_fn = lua
        .create_function(|_, path: String| Ok(Path::new(&path).is_dir()))
        .map_err(registration_err("create fs.is_dir function"))?;
    fs_table
        .set("is_dir", is_dir_fn)
        .map_err(registration_err("set fs.is_dir function"))?;

    let read_file_fn = lua
        .create_function(|_, path: String| Ok(fs::read_to_string(path).ok()))
        .map_err(registration_err("create fs.read_file function"))?;
    fs_table
        .set("read_file", read_file_fn)
        .map_err(registration_err("set fs.read_file function"))?;

    let write_file_fn = lua
        .create_function(|_, (path, contents): (String, String)| {
            to_lua_err(fs::write(path, contents)).map(|()| true)
        })
        .map_err(registration_err("create fs.write_file function"))?;
    fs_table
        .set("write_file", write_file_fn)
        .map_err(registration_err("set fs.write_file function"))?;

    let mkdir_fn = lua
        .create_function(|_, path: String| to_lua_err(fs::create_dir_all(path)).map(|()| true))
        .map_err(registration_err("create fs.mkdir function"))?;
    fs_table
        .set("mkdir", mkdir_fn)
        .map_err(registration_err("set fs.mkdir function"))?;

    let list_dir_fn = lua
        .create_function(|lua, path: String| match fs::read_dir(path) {
            Ok(entries) => {
                let result_table = lua.create_table()?;
                let mut index = 1;
                for entry_result in entries {
                    match entry_result {
                        Ok(entry) => {
                            let path_str = entry.path().to_string_lossy().to_string();
                            result_table.set(index, path_str)?;
                            index += 1;
                        }
                        Err(e) => {
                            warn!("Error reading directory entry: {}", e);
                        }
                    }
                }
                Ok(Value::Table(result_table))
            }
            Err(e) => {
                warn!("Error listing directory: {}", e);
                Ok(Value::Nil)
            }
        })
        .map_err(registration_err("create fs.list_dir function"))?;
    fs_table
        .set("list_dir", list_dir_fn)
        .map_err(registration_err("set fs.list_dir function"))?;

    Ok(())
}
