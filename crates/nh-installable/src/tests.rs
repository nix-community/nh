use std::env;

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

#[test]
fn test_resolve_non_unspecified_returns_unchanged() {
  // Test that non-Unspecified installables are returned as-is
  let flake = Installable::Flake {
    reference: String::from("/path/to/flake"),
    attribute: vec![String::from("host")],
  };
  let resolved = flake.clone().resolve(CommandContext::Os).unwrap();
  assert_eq!(flake.to_args(), resolved.to_args());

  let file = Installable::File {
    path:      PathBuf::from("/path/to/file.nix"),
    attribute: vec![String::from("config")],
  };
  let resolved = file.clone().resolve(CommandContext::Home).unwrap();
  assert_eq!(file.to_args(), resolved.to_args());

  let store = Installable::Store {
    path: PathBuf::from("/nix/store/abc"),
  };
  let resolved = store.clone().resolve(CommandContext::Darwin).unwrap();
  assert_eq!(store.to_args(), resolved.to_args());

  let expr = Installable::Expression {
    expression: String::from("{ pkgs }: pkgs.hello"),
    attribute:  vec![],
  };
  let resolved = expr.clone().resolve(CommandContext::Os).unwrap();
  assert_eq!(expr.to_args(), resolved.to_args());
}

#[test]
fn test_resolve_or_default_non_unspecified_returns_unchanged() {
  let flake = Installable::Flake {
    reference: String::from("/path/to/flake"),
    attribute: vec![String::from("host")],
  };

  let resolved = flake
    .clone()
    .resolve_or_default(CommandContext::Os)
    .unwrap();

  assert_eq!(flake.to_args(), resolved.to_args());
}

#[test]
#[serial]
fn test_resolve_or_default_uses_env_before_default() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos#myhost");

  let resolved = Installable::Unspecified
    .resolve_or_default(CommandContext::Os)
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
fn test_resolve_os_context_uses_nh_os_flake() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos#myhost");

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Home)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Home)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Darwin)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Darwin)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
    .unwrap();
  assert!(matches!(resolved, Installable::Unspecified));

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Home)
    .unwrap();
  assert!(matches!(resolved, Installable::Unspecified));

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Darwin)
    .unwrap();
  assert!(matches!(resolved, Installable::Unspecified));
}

#[test]
#[serial]
fn test_resolve_with_empty_attribute() {
  let env_guard = EnvGuard::clear();
  env_guard.set("NH_OS_FLAKE", "/etc/nixos");

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
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

  let resolved = Installable::Unspecified
    .resolve(CommandContext::Home)
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
  let resolved = Installable::Unspecified
    .resolve(CommandContext::Os)
    .unwrap();
  assert!(matches!(resolved, Installable::Unspecified));

  // But Home context should
  let resolved = Installable::Unspecified
    .resolve(CommandContext::Home)
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
