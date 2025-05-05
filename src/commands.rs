use std::ffi::{
    OsStr,
    OsString,
};

use color_eyre::{
    eyre::{
        bail,
        Context,
    },
    Result,
};
use subprocess::{
    Exec,
    ExitStatus,
    Redirection,
};
use thiserror::Error;
use tracing::{
    debug,
    info,
};

use crate::installable::Installable;

#[derive(Debug)]
pub struct Command {
    dry:     bool,
    message: Option<String>,
    command: OsString,
    args:    Vec<OsString>,
    elevate: bool,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(command: S) -> Self {
        Self {
            dry:     false,
            message: None,
            command: command.as_ref().to_os_string(),
            args:    vec![],
            elevate: false,
        }
    }

    #[must_use]
    pub const fn elevate(mut self, elevate: bool) -> Self {
        self.elevate = elevate;
        self
    }

    #[must_use]
    pub const fn dry(mut self, dry: bool) -> Self {
        self.dry = dry;
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
        }
        .stderr(Redirection::None)
        .stdout(Redirection::None);

        if let Some(m) = &self.message {
            info!("{}", m);
        }

        debug!(?cmd);

        if !self.dry {
            if let Some(m) = &self.message {
                cmd.join().wrap_err(m.clone())?;
            } else {
                cmd.join()?;
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
    message:     Option<String>,
    installable: Installable,
    extra_args:  Vec<OsString>,
    nom:         bool,
}

impl Build {
    #[must_use]
    pub const fn new(installable: Installable) -> Self {
        Self {
            message: None,
            installable,
            extra_args: vec![],
            nom: false,
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

    #[must_use]
    pub const fn nom(mut self, yes: bool) -> Self {
        self.nom = yes;
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
                    .args(&self.extra_args)
                    .stdout(Redirection::Pipe)
                    .stderr(Redirection::Merge)
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
                .stdout(Redirection::None)
                .stderr(Redirection::Merge);

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
