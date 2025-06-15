use tracing::warn;

use crate::Result;
use crate::commands::Command;
use crate::installable::Installable;

pub fn update(installable: &Installable, inputs: Option<Vec<String>>) -> Result<()> {
    match installable {
        Installable::Flake { reference, .. } => {
            let mut cmd = Command::new("nix").args(["flake", "update"]);

            if let Some(inputs) = inputs {
                for input in &inputs {
                    cmd = cmd.arg(input);
                }
                cmd = cmd.message(format!(
                    "Updating flake input{maybe_plural} {inputs}",
                    maybe_plural = if inputs.len() > 1 { "s" } else { "" },
                    inputs = inputs.join(", ")
                ));
            } else {
                cmd = cmd.message("Updating all flake inputs");
            }

            cmd.arg("--flake").arg(reference).run()?;
        }
        _ => {
            warn!(
                "Only flake installables can be updated, {} is not supported",
                installable.str_kind()
            );
        }
    }

    Ok(())
}
