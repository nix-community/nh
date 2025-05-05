use tracing::warn;

use crate::{
    commands::Command,
    installable::Installable,
    Result,
};

pub fn update(installable: &Installable, input: Option<String>) -> Result<()> {
    match installable {
        Installable::Flake { reference, .. } => {
            let mut cmd = Command::new("nix").args(["flake", "update"]);

            if let Some(i) = input {
                cmd = cmd.arg(&i).message(format!("Updating flake input {i}"));
            } else {
                cmd = cmd.message("Updating all flake inputs");
            }

            cmd.arg("--flake").arg(reference).run()?;
        },
        _ => {
            warn!(
                "Only flake installables can be updated, {} is not supported",
                installable.str_kind()
            );
        },
    }

    Ok(())
}
