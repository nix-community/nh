use std::{env, fs};

use serial_test::serial;

use super::*;

struct EnvGuard {
  saved: [(&'static str, Option<String>); 6],
}

impl EnvGuard {
  fn clear() -> Self {
    let saved = [
      ("NH_FLAKE", env::var("NH_FLAKE").ok()),
      ("NH_OS_FLAKE", env::var("NH_OS_FLAKE").ok()),
      ("NH_HOME_FLAKE", env::var("NH_HOME_FLAKE").ok()),
      ("NH_DARWIN_FLAKE", env::var("NH_DARWIN_FLAKE").ok()),
      ("NH_FILE", env::var("NH_FILE").ok()),
      ("NH_ATTRP", env::var("NH_ATTRP").ok()),
    ];

    unsafe {
      for (name, _) in &saved {
        env::remove_var(name);
      }
    }

    Self { saved }
  }

  fn set(&self, name: &'static str, value: &str) {
    debug_assert!(self.saved.iter().any(|(saved_name, _)| *saved_name == name));

    unsafe {
      env::set_var(name, value);
    }
  }
}

impl Drop for EnvGuard {
  fn drop(&mut self) {
    unsafe {
      for (name, value) in &self.saved {
        match value {
          Some(value) => env::set_var(name, value),
          None => env::remove_var(name),
        }
      }
    }
  }
}

fn specified(installable: Installable) -> InstallableArgs {
  InstallableArgs::Specified(installable)
}

#[test]
fn test_resolve_non_unspecified_returns_unchanged() {
  let flake = Installable::Flake {
    reference: String::from("/path/to/flake"),
    attribute: vec![String::from("host")],
  };
  let resolved = specified(flake.clone())
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  assert_eq!(flake.to_args(), resolved.to_args());

  let file = Installable::File {
    path:      PathBuf::from("/path/to/file.nix"),
    attribute: vec![String::from("config")],
  };
  let resolved = specified(file.clone())
    .resolve(CommandContext::Home)
    .unwrap()
    .unwrap();
  assert_eq!(file.to_args(), resolved.to_args());

  let store = Installable::Store {
    path: PathBuf::from("/nix/store/abc"),
  };
  let resolved = specified(store.clone())
    .resolve(CommandContext::Darwin)
    .unwrap()
    .unwrap();
  assert_eq!(store.to_args(), resolved.to_args());

  let expr = Installable::Expression {
    expression: String::from("{ pkgs }: pkgs.hello"),
    attribute:  vec![],
  };
  let resolved = specified(expr.clone())
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  assert_eq!(expr.to_args(), resolved.to_args());
}

#[test]
fn test_resolve_or_default_non_unspecified_returns_unchanged() {
  let flake = Installable::Flake {
    reference: String::from("github:user/repo"),
    attribute: vec![String::from("host")],
  };

  let resolved = specified(flake.clone())
    .resolve_or_default(CommandContext::Os)
    .unwrap();

  assert_eq!(flake.to_args(), resolved.to_args());
}

#[test]
#[serial]
fn test_resolve_or_default_uses_env_before_default() {
  let env_guard = EnvGuard::clear();
  let flake_dir = tempfile::tempdir().unwrap();
  fs::write(flake_dir.path().join("flake.nix"), "{}").unwrap();
  env_guard.set(
    "NH_OS_FLAKE",
    &format!("{}#myhost", flake_dir.path().display()),
  );

  let resolved = InstallableArgs::Unspecified
    .resolve_or_default(CommandContext::Os)
    .unwrap();

  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, flake_dir.path().to_string_lossy());
      assert_eq!(attribute, vec!["myhost"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
fn test_resolve_or_default_accepts_existing_local_flake_path() {
  let flake_dir = tempfile::tempdir().unwrap();
  fs::write(flake_dir.path().join("flake.nix"), "{}").unwrap();

  let installable = Installable::Flake {
    reference: flake_dir.path().to_string_lossy().into_owned(),
    attribute: vec![],
  };

  let resolved = specified(installable)
    .resolve_or_default(CommandContext::Os)
    .unwrap();

  assert_eq!(resolved.to_args(), vec![format!(
    "{}#",
    flake_dir.path().display()
  )]);
}

#[test]
fn test_resolve_or_default_rejects_missing_absolute_path() {
  let parent = tempfile::tempdir().unwrap();
  let missing_path = parent.path().join("missing-flake");
  assert!(!missing_path.exists());

  let installable = Installable::Flake {
    reference: missing_path.to_string_lossy().into_owned(),
    attribute: vec![],
  };

  let err = specified(installable)
    .resolve_or_default(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("Flake reference"));
  assert!(err.contains("does not exist or does not contain a flake.nix"));
  assert!(err.contains("NH_FLAKE/NH_OS_FLAKE"));
}

#[test]
fn test_resolve_or_default_rejects_existing_dir_without_flake_nix() {
  let dir = tempfile::tempdir().unwrap();

  let installable = Installable::Flake {
    reference: dir.path().to_string_lossy().into_owned(),
    attribute: vec![],
  };

  let err = specified(installable)
    .resolve_or_default(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("does not exist or does not contain a flake.nix"));
}

#[test]
fn test_resolve_or_default_rejects_subdir_inside_flake() {
  let flake_dir = tempfile::tempdir().unwrap();
  fs::write(flake_dir.path().join("flake.nix"), "{}").unwrap();
  let subdir = flake_dir.path().join("modules");
  fs::create_dir(&subdir).unwrap();

  let installable = Installable::Flake {
    reference: subdir.to_string_lossy().into_owned(),
    attribute: vec![],
  };

  let err = specified(installable)
    .resolve_or_default(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("does not exist or does not contain a flake.nix"));
}

#[test]
fn test_resolve_or_default_rejects_missing_path_scheme() {
  let parent = tempfile::tempdir().unwrap();
  let missing_path = parent.path().join("missing-flake");
  assert!(!missing_path.exists());

  let installable = Installable::Flake {
    reference: format!("path:{}", missing_path.display()),
    attribute: vec![],
  };

  let err = specified(installable)
    .resolve_or_default(CommandContext::Home)
    .unwrap_err()
    .to_string();

  assert!(err.contains("NH_FLAKE/NH_HOME_FLAKE"));
}

#[test]
fn test_resolve_or_default_accepts_path_scheme_with_query() {
  let flake_dir = tempfile::tempdir().unwrap();
  fs::write(flake_dir.path().join("flake.nix"), "{}").unwrap();
  let reference = format!("path:{}?lastModified=1", flake_dir.path().display());
  let installable = Installable::Flake {
    reference: reference.clone(),
    attribute: vec![],
  };

  let resolved = specified(installable)
    .resolve_or_default(CommandContext::Os)
    .unwrap();

  assert_eq!(resolved.to_args(), vec![format!("{reference}#")]);
}

#[test]
fn test_resolve_or_default_ignores_registry_and_url_refs() {
  for reference in ["nixpkgs", "github:NixOS/nixpkgs"] {
    let installable = Installable::Flake {
      reference: reference.to_string(),
      attribute: vec![],
    };

    specified(installable)
      .resolve_or_default(CommandContext::Os)
      .unwrap();
  }
}

#[test]
#[serial]
fn test_resolve_rejects_empty_nh_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_FLAKE", "");

  let err = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("NH_FLAKE is empty"));
}

#[test]
#[serial]
fn test_resolve_rejects_empty_command_specific_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "");
  env_guard.set("NH_FLAKE", "github:user/repo");

  let err = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("NH_OS_FLAKE is empty"));
}

#[test]
#[serial]
fn test_resolve_rejects_env_flake_without_reference_before_attribute() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_FLAKE", "#fallback");

  let err = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("NH_FLAKE missing reference part before `#`"));
}

#[test]
#[serial]
fn test_resolve_rejects_malformed_nh_attrp() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_FILE", "/path/to/file.nix");
  env_guard.set("NH_ATTRP", r#"foo."bar"#);

  let err = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap_err()
    .to_string();

  assert!(err.contains("NH_ATTRP contains an unclosed quoted attribute"));
}

#[test]
fn test_cli_installable_rejects_empty_flake_reference() {
  let cmd = InstallableArgs::augment_args(clap::Command::new("test"));
  let err = InstallableArgs::from_arg_matches(
    &cmd.try_get_matches_from(["test", ""]).unwrap(),
  )
  .unwrap_err()
  .to_string();

  assert!(err.contains("installable argument is empty"));
}

#[test]
fn test_cli_installable_rejects_attribute_without_reference() {
  let cmd = InstallableArgs::augment_args(clap::Command::new("test"));
  let err = InstallableArgs::from_arg_matches(
    &cmd.try_get_matches_from(["test", "#fallback"]).unwrap(),
  )
  .unwrap_err()
  .to_string();

  assert!(
    err.contains("installable argument missing reference part before `#`")
  );
}

#[test]
fn test_cli_file_rejects_malformed_attribute() {
  let cmd = InstallableArgs::augment_args(clap::Command::new("test"));
  let matches = cmd
    .try_get_matches_from(["test", "--file", "file.nix", r#"foo."bar"#])
    .unwrap();
  let err = InstallableArgs::from_arg_matches(&matches)
    .unwrap_err()
    .to_string();

  assert!(err.contains("attribute path contains an unclosed quoted attribute"));
}

#[test]
#[serial]
fn test_uses_flakes_checks_cli_and_env_inputs() {
  let env_guard = EnvGuard::clear();

  assert!(!InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));

  let file = specified(Installable::File {
    path:      PathBuf::from("/path/to/file.nix"),
    attribute: vec![],
  });
  assert!(!file.uses_flakes(CommandContext::Os));

  let flake = specified(Installable::Flake {
    reference: String::from("github:user/repo"),
    attribute: vec![],
  });
  assert!(flake.uses_flakes(CommandContext::Os));

  env_guard.set("NH_FLAKE", "github:user/repo");
  assert!(InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));
  assert!(InstallableArgs::Unspecified.uses_flakes(CommandContext::Home));
  assert!(InstallableArgs::Unspecified.uses_flakes(CommandContext::Darwin));
}

#[test]
#[serial]
fn test_uses_flakes_checks_context_specific_env() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_HOME_FLAKE", "github:user/home");

  assert!(!InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));
  assert!(InstallableArgs::Unspecified.uses_flakes(CommandContext::Home));
  assert!(!InstallableArgs::Unspecified.uses_flakes(CommandContext::Darwin));
}

#[test]
#[serial]
fn test_uses_flakes_respects_resolution_precedence() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_FLAKE", "github:user/repo");

  let file = specified(Installable::File {
    path:      PathBuf::from("/path/to/file.nix"),
    attribute: vec![],
  });
  assert!(!file.uses_flakes(CommandContext::Os));

  env_guard.set("NH_FILE", "/path/to/file.nix");
  assert!(!InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));

  env_guard.set("NH_OS_FLAKE", "github:user/os");
  assert!(InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));
}

#[test]
#[serial]
fn test_uses_flakes_ignores_empty_env_values() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "");
  env_guard.set("NH_FLAKE", "");

  assert!(!InstallableArgs::Unspecified.uses_flakes(CommandContext::Os));
}

#[test]
#[serial]
fn test_resolve_os_context_uses_nh_os_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos#myhost");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/etc/nixos");
      assert_eq!(attribute, vec!["myhost"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_os_context_prefers_os_flake_over_generic() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos#myhost");
  env_guard.set("NH_FLAKE", "/home/user/flake#other");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/etc/nixos");
      assert_eq!(attribute, vec!["myhost"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_os_context_falls_back_to_nh_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_FLAKE", "/home/user/flake#fallback");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/home/user/flake");
      assert_eq!(attribute, vec!["fallback"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_home_context_uses_nh_home_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_HOME_FLAKE", "~/.config/home-manager#myuser");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Home)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "~/.config/home-manager");
      assert_eq!(attribute, vec!["myuser"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_home_context_prefers_home_flake_over_generic() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_HOME_FLAKE", "~/.config/home-manager#myuser");
  env_guard.set("NH_FLAKE", "/other/flake#other");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Home)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "~/.config/home-manager");
      assert_eq!(attribute, vec!["myuser"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_darwin_context_uses_nh_darwin_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_DARWIN_FLAKE", "/etc/nix-darwin#macbook");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Darwin)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/etc/nix-darwin");
      assert_eq!(attribute, vec!["macbook"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_darwin_context_prefers_darwin_flake_over_generic() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_DARWIN_FLAKE", "/etc/nix-darwin#macbook");
  env_guard.set("NH_FLAKE", "/other/flake#other");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Darwin)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/etc/nix-darwin");
      assert_eq!(attribute, vec!["macbook"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_no_env_vars_returns_unspecified() {
  let _env_guard = EnvGuard::clear();

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap();
  assert!(resolved.is_none());

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Home)
    .unwrap();
  assert!(resolved.is_none());

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Darwin)
    .unwrap();
  assert!(resolved.is_none());
}

#[test]
#[serial]
fn test_resolve_with_empty_attribute() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos");

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "/etc/nixos");
      assert!(attribute.is_empty());
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_with_nested_attribute() {
  let env_guard = EnvGuard::clear();
  env_guard.set(
    "NH_HOME_FLAKE",
    "~/.config/home-manager#homeConfigurations.user",
  );

  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Home)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "~/.config/home-manager");
      assert_eq!(attribute, vec!["homeConfigurations", "user"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}

#[test]
#[serial]
fn test_resolve_command_specific_isolation() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_HOME_FLAKE", "~/.config/home-manager#user");

  // OS context should not pick up NH_HOME_FLAKE
  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Os)
    .unwrap();
  assert!(resolved.is_none());

  // But Home context should
  let resolved = InstallableArgs::Unspecified
    .resolve(CommandContext::Home)
    .unwrap()
    .unwrap();
  match resolved {
    Installable::Flake {
      reference,
      attribute,
    } => {
      assert_eq!(reference, "~/.config/home-manager");
      assert_eq!(attribute, vec!["user"]);
    },
    _ => panic!("Expected Flake, got {resolved:?}"),
  }
}
