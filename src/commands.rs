use std::ffi::{OsStr, OsString};

use color_eyre::{
    Result,
    eyre::{Context, bail},
};
use subprocess::{Exec, ExitStatus, Redirection};
use thiserror::Error;
use tracing::{debug, info};

use crate::installable::Installable;

fn ssh_wrap(cmd: Exec, ssh: Option<&str>) -> Exec {
    if let Some(ssh) = ssh {
        Exec::cmd("ssh")
            .arg("-T")
            .arg(ssh)
            .stdin(cmd.to_cmdline_lossy().as_str())
    } else {
        cmd
    }
}

#[derive(Debug)]
pub struct Command {
    dry: bool,
    message: Option<String>,
    command: OsString,
    args: Vec<OsString>,
    elevate: bool,
    ssh: Option<String>,
    show_output: bool,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(command: S) -> Self {
        Self {
            dry: false,
            message: None,
            command: command.as_ref().to_os_string(),
            args: vec![],
            elevate: false,
            ssh: None,
            show_output: false,
        }
    }

    pub const fn elevate(mut self, elevate: bool) -> Self {
        self.elevate = elevate;
        self
    }

    pub const fn dry(mut self, dry: bool) -> Self {
        self.dry = dry;
        self
    }

    pub const fn show_output(mut self, show_output: bool) -> Self {
        self.show_output = show_output;
        self
    }

    pub fn ssh(mut self, ssh: Option<String>) -> Self {
        self.ssh = ssh;
        self
    }

    pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I>(mut self, args: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<OsStr>,
    {
        for elem in args {
            self.args.push(elem.as_ref().to_os_string());
        }
        self
    }

    pub fn message<S: AsRef<str>>(mut self, message: S) -> Self {
        self.message = Some(message.as_ref().to_string());
        self
    }

    pub fn run(&self) -> Result<()> {
        let cmd = if self.elevate {
            let cmd = if cfg!(target_os = "macos") {
                // Check for if sudo has the preserve-env flag
                Exec::cmd("sudo").args(
                    if Exec::cmd("sudo")
                        .args(&["--help"])
                        .stderr(Redirection::None)
                        .stdout(Redirection::Pipe)
                        .capture()?
                        .stdout_str()
                        .contains("--preserve-env")
                    {
                        &["--set-home", "--preserve-env=PATH", "env"]
                    } else {
                        &["--set-home"]
                    },
                )
            } else {
                Exec::cmd("sudo")
            };

            // use NH_SUDO_ASKPASS program for sudo if present
            let askpass = std::env::var("NH_SUDO_ASKPASS");
            let cmd = if let Ok(askpass) = askpass {
                cmd.env("SUDO_ASKPASS", askpass).arg("-A")
            } else {
                cmd
            };

            cmd.arg(&self.command).args(&self.args)
        } else {
            Exec::cmd(&self.command).args(&self.args)
        };
        // Configure output redirection based on show_output setting
        let cmd = ssh_wrap(
            if self.show_output {
                cmd.stderr(Redirection::Merge)
            } else {
                cmd.stderr(Redirection::None).stdout(Redirection::None)
            },
            self.ssh.as_deref(),
        );

        if let Some(m) = &self.message {
            info!("{}", m);
        }

        debug!(?cmd);

        if !self.dry {
            if let Some(m) = &self.message {
                cmd.capture().wrap_err(m.clone())?;
            } else {
                cmd.capture()?;
            }
        }

        Ok(())
    }

    pub fn run_capture(&self) -> Result<Option<String>> {
        let cmd = Exec::cmd(&self.command)
            .args(&self.args)
            .stderr(Redirection::None)
            .stdout(Redirection::Pipe);

        if let Some(m) = &self.message {
            info!("{}", m);
        }

        debug!(?cmd);

        if !self.dry {
            Ok(Some(cmd.capture()?.stdout_str()))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
pub struct Build {
    message: Option<String>,
    installable: Installable,
    extra_args: Vec<OsString>,
    nom: bool,
    builder: Option<String>,
}

impl Build {
    pub const fn new(installable: Installable) -> Self {
        Self {
            message: None,
            installable,
            extra_args: vec![],
            nom: false,
            builder: None,
        }
    }

    pub fn message<S: AsRef<str>>(mut self, message: S) -> Self {
        self.message = Some(message.as_ref().to_string());
        self
    }

    pub fn extra_arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        self.extra_args.push(arg.as_ref().to_os_string());
        self
    }

    pub const fn nom(mut self, yes: bool) -> Self {
        self.nom = yes;
        self
    }

    pub fn builder(mut self, builder: Option<String>) -> Self {
        self.builder = builder;
        self
    }

    pub fn extra_args<I>(mut self, args: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<OsStr>,
    {
        for elem in args {
            self.extra_args.push(elem.as_ref().to_os_string());
        }
        self
    }

    pub fn run(&self) -> Result<()> {
        if let Some(m) = &self.message {
            info!("{}", m);
        }

        let installable_args = self.installable.to_args();

        let exit = if self.nom {
            let cmd = {
                Exec::cmd("nix")
                    .arg("build")
                    .args(&installable_args)
                    .args(&["--log-format", "internal-json", "--verbose"])
                    .args(&match &self.builder {
                        Some(host) => {
                            vec!["--builders".to_string(), format!("ssh://{host} - - - 100")]
                        }
                        None => vec![],
                    })
                    .args(&self.extra_args)
                    .stderr(Redirection::Merge)
                    .stdout(Redirection::Pipe)
                    | Exec::cmd("nom").args(&["--json"])
            }
            .stdout(Redirection::None);
            debug!(?cmd);
            cmd.join()
        } else {
            let cmd = Exec::cmd("nix")
                .arg("build")
                .args(&installable_args)
                .args(&self.extra_args)
                .stderr(Redirection::Merge)
                .stdout(Redirection::None);

            debug!(?cmd);
            cmd.join()
        };

        match exit? {
            ExitStatus::Exited(0) => (),
            other => bail!(ExitError(other)),
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
#[error("Command exited with status {0:?}")]
pub struct ExitError(ExitStatus);
