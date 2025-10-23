use std::{collections::HashMap, fs, path::Path, process};

use chrono::{DateTime, Local, TimeZone, Utc};
use clap::ValueEnum;
use color_eyre::eyre::{Result, bail};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct GenerationInfo {
  /// Number of a generation
  pub number: String,

  /// Date on switch a generation was built
  pub date: String,

  /// `NixOS` version derived from `nixos-version`
  pub nixos_version: String,

  /// Version of the bootable kernel for a given generation
  pub kernel_version: String,

  /// Revision for a configuration. This will be the value
  /// set in `config.system.configurationRevision`
  pub configuration_revision: Option<String>,

  /// Specialisations, if any.
  pub specialisations: Option<Vec<String>>,

  /// Whether a given generation is the current one.
  pub current: bool,

  /// Closure size of the generation.
  pub closure_size: String,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum Field {
  /// Generation Id
  Id,

  /// Build Date
  Date,

  /// Nixos Version
  Nver,

  /// Kernel Version
  Kernel,

  /// Configuration Revision
  #[clap(name = "confRev")]
  Confrev,

  /// Specialisations
  Spec,

  /// Closure Size
  Size,
}

#[derive(Clone, Copy)]
struct ColumnWidths {
  id:      usize,
  date:    usize,
  nver:    usize,
  kernel:  usize,
  confrev: usize,
  spec:    usize,
  size:    usize,
}

impl Field {
  fn column_info(&self, width: ColumnWidths) -> (&'static str, usize) {
    match self {
      Self::Id => ("Generation No", width.id),
      Self::Date => ("Build Date", width.date),
      Self::Nver => ("NixOS Version", width.nver),
      Self::Kernel => ("Kernel", width.kernel),
      Self::Confrev => ("Configuration Revision", width.confrev),
      Self::Spec => ("Specialisations", width.spec),
      Self::Size => ("Closure Size", width.size),
    }
  }
}
#[must_use]
pub fn from_dir(generation_dir: &Path) -> Option<u64> {
  generation_dir
    .file_name()
    .and_then(|os_str| os_str.to_str())
    .and_then(|generation_base| {
      let no_link_gen = generation_base.trim_end_matches("-link");
      no_link_gen
        .rsplit_once('-')
        .and_then(|(_, generation_num)| generation_num.parse::<u64>().ok())
    })
}

#[must_use]
pub fn get_closure_size(generation_dir: &Path) -> String {
  let store_path = generation_dir
    .read_link()
    .unwrap_or_else(|_| generation_dir.to_path_buf());
  match process::Command::new("nix")
    .arg("path-info")
    .arg(generation_dir)
    .arg("-Sh")
    .arg("--json")
    .output()
  {
    Ok(output) => {
      let output_str = String::from_utf8_lossy(&output.stdout);
      match serde_json::from_str::<serde_json::Value>(&output_str) {
        #[allow(clippy::cast_precision_loss)]
        Ok(json) => {
          // `nix path-info --json` returns an array, we need to handle it
          let store_path_str = store_path.to_string_lossy();
          let closure_size = json.as_array().and_then(|arr| {
            arr.iter().find_map(|entry| {
              let path = entry.get("path").and_then(|v| v.as_str());
              let size =
                entry.get("closureSize").and_then(serde_json::Value::as_u64);
              if let (Some(path), Some(size)) = (path, size) {
                if path == store_path_str {
                  return Some(size);
                }
              }
              None
            })
          });
          if closure_size.is_none() {
            let paths: Vec<String> = json
              .as_array()
              .map(|arr| {
                arr
                  .iter()
                  .filter_map(|entry| {
                    entry.get("path").and_then(|v| {
                      v.as_str().map(std::string::ToString::to_string)
                    })
                  })
                  .collect()
              })
              .unwrap_or_default();
            debug!(
              "get_closure_size: store_path not found or closureSize missing. \
               store_path: {store_path_str}, json paths: {:?}, output: {}",
              paths, output_str
            );
          }
          closure_size.map_or_else(
            || "Unknown".to_string(),
            |bytes| format!("{:.1} GB", bytes as f64 / 1_073_741_824.0),
          )
        },
        Err(e) => {
          debug!(
            "get_closure_size: failed to parse JSON: {e} output: {output_str}"
          );
          "Unknown".to_string()
        },
      }
    },
    Err(e) => {
      debug!("get_closure_size: failed to run nix path-info: {e:?}");
      "Unknown".to_string()
    },
  }
}

pub fn describe(generation_dir: &Path) -> Option<GenerationInfo> {
  let generation_number = from_dir(generation_dir)?;
  let closure_size = get_closure_size(generation_dir);
  // Get metadata once and reuse for both date and existence checks
  let metadata = fs::metadata(generation_dir).ok()?;
  let build_date = metadata
    .created()
    .or_else(|_| metadata.modified())
    .map_or_else(
      |_| "Unknown".to_string(),
      |system_time| {
        let duration = system_time
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default();
        DateTime::<Utc>::from(std::time::UNIX_EPOCH + duration).to_rfc3339()
      },
    );

  let nixos_version = fs::read_to_string(generation_dir.join("nixos-version"))
    .unwrap_or_else(|_| "Unknown".to_string());

  // XXX: Nixpkgs appears to have changed where kernel modules are stored in a
  // recent change. I do not care to track which, but we should try the new path
  // and fall back to the old one IF and ONLY IF the new one fails. This is to
  // avoid breakage for outdated channels.
  let kernel_modules_dir_new =
    generation_dir.join("kernel-modules/lib/modules");
  let kernel_modules_dir_old = generation_dir
    .join("kernel")
    .canonicalize()
    .ok()
    .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    .unwrap_or_else(|| generation_dir.to_path_buf())
    .join("lib/modules");

  let kernel_version = if kernel_modules_dir_new.exists() {
    fs::read_dir(&kernel_modules_dir_new).map_or_else(
      |_| "Unknown".to_string(),
      |entries| {
        let mut versions = Vec::with_capacity(4);
        for entry in entries.filter_map(Result::ok) {
          if let Some(name) = entry.file_name().to_str() {
            versions.push(name.to_string());
          }
        }
        versions.join(", ")
      },
    )
  } else if kernel_modules_dir_old.exists() {
    fs::read_dir(&kernel_modules_dir_old).map_or_else(
      |_| "Unknown".to_string(),
      |entries| {
        let mut versions = Vec::with_capacity(4);
        for entry in entries.filter_map(Result::ok) {
          if let Some(name) = entry.file_name().to_str() {
            versions.push(name.to_string());
          }
        }
        versions.join(", ")
      },
    )
  } else {
    "Unknown".to_string()
  };

  let configuration_revision = {
    let nixos_version_path = generation_dir.join("sw/bin/nixos-version");
    if nixos_version_path.exists() {
      process::Command::new(&nixos_version_path)
        .arg("--configuration-revision")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    } else {
      None
    }
  };

  let specialisations = {
    let specialisation_path = generation_dir.join("specialisation");
    if specialisation_path.exists() {
      let specs = fs::read_dir(specialisation_path)
        .map(|entries| {
          entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .collect::<Vec<String>>()
        })
        .unwrap_or_default();
      if specs.is_empty() { None } else { Some(specs) }
    } else {
      None
    }
  };

  // Check if this generation is the current one
  let Some(run_current_target) = fs::read_link("/run/current-system")
    .ok()
    .and_then(|p| fs::canonicalize(p).ok())
  else {
    return Some(GenerationInfo {
      number: generation_number.to_string(),
      date: build_date,
      nixos_version,
      kernel_version,
      configuration_revision,
      specialisations,
      current: false,
      closure_size,
    });
  };

  let Some(gen_store_path) = fs::read_link(generation_dir)
    .ok()
    .and_then(|p| fs::canonicalize(p).ok())
  else {
    return Some(GenerationInfo {
      number: generation_number.to_string(),
      date: build_date,
      nixos_version,
      kernel_version,
      configuration_revision,
      specialisations,
      current: false,
      closure_size,
    });
  };

  let current = run_current_target == gen_store_path;

  Some(GenerationInfo {
    number: generation_number.to_string(),
    date: build_date,
    nixos_version,
    kernel_version,
    configuration_revision,
    specialisations,
    current,
    closure_size,
  })
}

/// Print information about the given generations.
///
/// # Errors
///
/// Returns an error if output or formatting fails.
#[expect(clippy::too_many_lines)]
pub fn print_info(
  mut generations: Vec<GenerationInfo>,
  fields: &[Field],
) -> Result<()> {
  // Parse all dates at once and cache them
  let mut parsed_dates = HashMap::with_capacity(generations.len());
  for generation in &generations {
    let date = DateTime::parse_from_rfc3339(&generation.date).map_or_else(
      |_| Local.timestamp_opt(0, 0).unwrap(),
      |dt| dt.with_timezone(&Local),
    );
    parsed_dates.insert(
      generation.date.clone(),
      date.format("%Y-%m-%d %H:%M:%S").to_string(),
    );
  }

  // Sort generations by numeric value of the generation number
  generations
    .sort_by_key(|generation| generation.number.parse::<u64>().unwrap_or(0));

  let current_generation =
    generations.iter().find(|generation| generation.current);
  debug!(?current_generation);

  if let Some(current) = current_generation {
    println!("NixOS {}", current.nixos_version);
  } else {
    bail!("Error getting current generation!");
  }

  // Conditionally hide columns if they are empty for all generations
  let has_confrev = generations
    .iter()
    .any(|g| g.configuration_revision.is_some());
  let has_spec = generations.iter().any(|g| g.specialisations.is_some());

  let visible_fields: Vec<&Field> = fields
    .iter()
    .filter(|f| {
      match f {
        Field::Confrev => has_confrev,
        Field::Spec => has_spec,
        _ => true,
      }
    })
    .collect();

  // Determine column widths for pretty printing
  let max_nixos_version_len = generations
    .iter()
    .map(|g| g.nixos_version.len())
    .max()
    .unwrap_or(22); // length of version + date + rev, assumes no tags

  let max_kernel_len = generations
    .iter()
    .map(|g| g.kernel_version.len())
    .max()
    .unwrap_or(12); // arbitrary value

  let max_generation_no = generations
    .iter()
    .map(|g| g.number.len())
    .max()
    .unwrap_or(5);

  let widths = ColumnWidths {
    id:      max_generation_no + 10, // "Generation No"
    date:    20,                     // "Build Date"
    nver:    max_nixos_version_len,
    kernel:  max_kernel_len,
    confrev: 22, // "Configuration Revision"
    spec:    15, // "Specialisations"
    size:    12, // "Closure Size"
  };

  let header = visible_fields
    .iter()
    .map(|f| {
      let (name, width) = f.column_info(widths);
      format!("{name:<width$}")
    })
    .collect::<Vec<String>>()
    .join(" ");
  println!("{header}");

  // Print generations in descending order
  for generation in generations.iter().rev() {
    let formatted_date = parsed_dates
      .get(&generation.date)
      .cloned()
      .unwrap_or_else(|| "Unknown".to_string());

    let specialisations = generation.specialisations.as_ref().map(|specs| {
      specs
        .iter()
        .map(|s| format!("*{s}"))
        .collect::<Vec<String>>()
        .join(" ")
    });

    let row: String = visible_fields
      .iter()
      .map(|f| {
        let (_, width) = f.column_info(widths);
        let cell_content = match f {
          Field::Id => {
            format!(
              "{}{}",
              generation.number,
              if generation.current { " (current)" } else { "" }
            )
          },
          Field::Date => formatted_date.clone(),
          Field::Nver => generation.nixos_version.clone(),
          Field::Kernel => generation.kernel_version.clone(),
          Field::Confrev => {
            generation
              .configuration_revision
              .clone()
              .unwrap_or_default()
          },
          Field::Spec => specialisations.clone().unwrap_or_default(),
          Field::Size => generation.closure_size.clone(),
        };
        format!("{cell_content:width$}")
      })
      .collect::<Vec<String>>()
      .join(" ");
    println!("{row}");
  }

  Ok(())
}
