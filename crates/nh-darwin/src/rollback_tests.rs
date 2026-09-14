#![expect(
  clippy::unwrap_used,
  reason = "Tests assert errors from deliberately invalid fixtures."
)]

use std::{
  cell::RefCell,
  os::unix::fs::{PermissionsExt, symlink},
  path::PathBuf,
};

use super::*;

struct Fixture {
  directory:       tempfile::TempDir,
  profile:         PathBuf,
  current_profile: PathBuf,
}

impl Fixture {
  fn new() -> Result<Self> {
    let directory = tempfile::tempdir()?;
    let profile = directory.path().join("system");
    let current_profile = directory.path().join("current-system");
    let fixture = Self {
      directory,
      profile,
      current_profile,
    };
    for number in [2, 9, 12, 20] {
      let system = fixture.system(number);
      fs::create_dir_all(system.join("sw/bin"))?;
      for executable in ["activate", "sw/bin/darwin-rebuild"] {
        let path = system.join(executable);
        // Validated by production code; execution is replaced in these tests.
        fs::write(&path, "fixture")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
      }
      symlink(&system, fixture.generation(number))?;
    }
    fixture.select(12)?;
    symlink(fixture.system(12), &fixture.current_profile)?;
    Ok(fixture)
  }

  fn system(&self, number: u64) -> PathBuf {
    self.directory.path().join(format!("closure-{number}"))
  }

  fn generation(&self, number: u64) -> PathBuf {
    self.directory.path().join(format!("system-{number}-link"))
  }

  fn select(&self, number: u64) -> Result<()> {
    if self.profile.is_symlink() {
      fs::remove_file(&self.profile)?;
    }
    symlink(format!("system-{number}-link"), &self.profile)?;
    Ok(())
  }

  fn run(
    &self,
    args: DarwinRollbackArgs,
    mut select: impl FnMut(u64) -> Result<()>,
    activate: impl FnOnce(Command) -> Result<()>,
  ) -> Result<()> {
    args.rollback_with(
      &self.profile,
      &self.current_profile,
      None,
      |profile, number, elevation| {
        assert_eq!(profile, self.profile);
        assert!(elevation.is_none());
        select(number)
      },
      activate,
    )
  }
}

fn args() -> DarwinRollbackArgs {
  DarwinRollbackArgs {
    dry:                  false,
    ask:                  false,
    to:                   None,
    diff:                 DiffType::Never,
    bypass_root_check:    false,
    show_activation_logs: false,
  }
}

#[test]
fn generation_numbers_require_system_generation_names() {
  for (name, expected) in [
    ("system-42-link", Some(42)),
    ("/profiles/system-42-link", Some(42)),
    ("system-18446744073709551615-link", Some(u64::MAX)),
    ("system-18446744073709551616-link", None),
    ("system--1-link", None),
    ("system-one-link", None),
    ("system-42", None),
    ("home-42-link", None),
    ("system-42-link-extra", None),
  ] {
    assert_eq!(generation_number(Path::new(name)), expected, "{name}");
  }
}

#[test]
fn current_generation_accepts_relative_and_absolute_links() -> Result<()> {
  let fixture = Fixture::new()?;
  assert_eq!(current_generation(&fixture.profile)?, 12);
  fs::remove_file(&fixture.profile)?;
  symlink(fixture.generation(9), &fixture.profile)?;
  assert_eq!(current_generation(&fixture.profile)?, 9);
  Ok(())
}

#[test]
fn current_generation_rejects_a_direct_store_link() -> Result<()> {
  let fixture = Fixture::new()?;
  fs::remove_file(&fixture.profile)?;
  symlink(fixture.system(12), &fixture.profile)?;
  let error = current_generation(&fixture.profile).unwrap_err();
  assert!(
    error
      .to_string()
      .contains("does not point to a system generation")
  );
  Ok(())
}

#[test]
fn previous_generation_handles_gaps_and_ignores_non_generations() -> Result<()>
{
  let fixture = Fixture::new()?;
  fs::write(fixture.generation(11), "not a symlink")?;
  fs::create_dir(fixture.generation(10))?;
  symlink(
    fixture.system(9),
    fixture.directory.path().join("home-11-link"),
  )?;
  assert_eq!(previous_generation(&fixture.profile, 12)?, 9);
  let error = previous_generation(&fixture.profile, 2).unwrap_err();
  assert!(
    error
      .to_string()
      .contains("No Darwin generation older than 2")
  );
  Ok(())
}

#[test]
fn changed_generation_is_rejected() -> Result<()> {
  let fixture = Fixture::new()?;
  ensure_current_generation(&fixture.profile, 12)?;
  fixture.select(20)?;
  let error = ensure_current_generation(&fixture.profile, 12).unwrap_err();
  assert!(error.to_string().contains("refusing to overwrite"));
  Ok(())
}

#[test]
fn rollback_selects_before_activating() -> Result<()> {
  for (requested, expected) in [(None, 9), (Some(2), 2), (Some(20), 20)] {
    let fixture = Fixture::new()?;
    let calls = RefCell::new(Vec::new());
    let mut options = args();
    options.to = requested;
    fixture.run(
      options,
      |number| {
        calls.borrow_mut().push("select");
        assert_eq!(number, expected);
        fixture.select(number)
      },
      |_| {
        calls.borrow_mut().push("activate");
        assert_eq!(current_generation(&fixture.profile)?, expected);
        assert_eq!(
          fixture.profile.canonicalize()?,
          fixture.system(expected).canonicalize()?
        );
        Ok(())
      },
    )?;
    assert_eq!(*calls.borrow(), ["select", "activate"]);
    assert_eq!(current_generation(&fixture.profile)?, expected);
  }
  Ok(())
}

#[test]
fn dry_rollback_does_not_prompt_select_or_activate() -> Result<()> {
  let fixture = Fixture::new()?;
  let mut options = args();
  options.dry = true;
  options.ask = true;
  fixture.run(
    options,
    |_| bail!("dry rollback attempted selection"),
    |_| bail!("dry rollback attempted activation"),
  )?;
  assert_eq!(current_generation(&fixture.profile)?, 12);
  Ok(())
}

#[test]
fn missing_and_broken_targets_fail_before_mutation() -> Result<()> {
  let fixture = Fixture::new()?;
  for target in [99, 9] {
    if target == 9 {
      fs::remove_dir_all(fixture.system(9))?;
    }
    let mut options = args();
    options.to = Some(target);
    let error = fixture
      .run(
        options,
        |_| bail!("unexpected selection"),
        |_| bail!("unexpected activation"),
      )
      .unwrap_err();
    assert!(
      error
        .to_string()
        .contains("does not exist or is unavailable")
    );
    assert_eq!(current_generation(&fixture.profile)?, 12);
  }
  Ok(())
}

#[test]
fn invalid_activation_executables_fail_before_mutation() -> Result<()> {
  for executable in ["activate", "sw/bin/darwin-rebuild"] {
    for invalid_kind in ["missing", "directory", "not executable"] {
      let fixture = Fixture::new()?;
      let path = fixture.system(9).join(executable);
      fs::remove_file(&path)?;
      match invalid_kind {
        "directory" => fs::create_dir(&path)?,
        "not executable" => {
          fs::write(&path, "fixture")?;
          fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        },
        _ => {},
      }
      let error = fixture
        .run(
          args(),
          |_| bail!("unexpected selection"),
          |_| bail!("unexpected activation"),
        )
        .unwrap_err();
      let message = error.to_string();
      assert!(
        message.contains("Darwin activation executable"),
        "{message}"
      );
      assert!(message.contains(executable), "{message}");
      assert_eq!(current_generation(&fixture.profile)?, 12);
    }
  }
  Ok(())
}

#[test]
fn selection_failure_prevents_activation() -> Result<()> {
  let fixture = Fixture::new()?;
  let error = fixture
    .run(
      args(),
      |_| bail!("selection failed"),
      |_| bail!("unexpected activation"),
    )
    .unwrap_err();
  assert!(format!("{error:#}").contains("selection failed"));
  assert_eq!(current_generation(&fixture.profile)?, 12);
  Ok(())
}

#[test]
fn selection_must_actually_change_the_profile() -> Result<()> {
  let fixture = Fixture::new()?;
  let error = fixture
    .run(args(), |_| Ok(()), |_| bail!("unexpected activation"))
    .unwrap_err();
  assert!(error.to_string().contains("refusing to overwrite"));
  Ok(())
}

#[test]
fn activation_failure_restores_original_profile() -> Result<()> {
  let fixture = Fixture::new()?;
  let selections = RefCell::new(Vec::new());
  let error = fixture
    .run(
      args(),
      |number| {
        selections.borrow_mut().push(number);
        fixture.select(number)
      },
      |_| bail!("activation failed"),
    )
    .unwrap_err();
  assert_eq!(*selections.borrow(), [9, 12]);
  assert_eq!(current_generation(&fixture.profile)?, 12);
  let message = format!("{error:#}");
  assert!(message.contains("restored to generation 12"));
  assert!(message.contains("may have partially changed the system"));
  assert!(message.contains("activation failed"));
  Ok(())
}

#[test]
fn recovery_failure_reports_both_errors() -> Result<()> {
  let fixture = Fixture::new()?;
  let error = fixture
    .run(
      args(),
      |number| {
        if number == 12 {
          bail!("recovery selection failed");
        }
        fixture.select(number)
      },
      |_| bail!("activation failed"),
    )
    .unwrap_err();
  let message = format!("{error:#}");
  assert!(message.contains("could not be restored"));
  assert!(message.contains("recovery selection failed"));
  assert!(message.contains("activation failed"));
  assert_eq!(current_generation(&fixture.profile)?, 9);
  Ok(())
}

#[test]
fn recovery_does_not_overwrite_a_concurrent_profile_change() -> Result<()> {
  let fixture = Fixture::new()?;
  let error = fixture
    .run(
      args(),
      |number| fixture.select(number),
      |_| {
        fixture.select(20)?;
        bail!("activation failed");
      },
    )
    .unwrap_err();
  assert!(format!("{error:#}").contains("refusing to overwrite"));
  assert_eq!(current_generation(&fixture.profile)?, 20);
  Ok(())
}

#[test]
fn recovery_does_not_overwrite_a_changed_generation_target() -> Result<()> {
  let fixture = Fixture::new()?;
  let error = fixture
    .run(
      args(),
      |number| fixture.select(number),
      |_| {
        fs::remove_file(fixture.generation(9))?;
        symlink(fixture.system(20), fixture.generation(9))?;
        bail!("activation failed");
      },
    )
    .unwrap_err();
  assert!(format!("{error:#}").contains("System profile target changed"));
  assert_eq!(current_generation(&fixture.profile)?, 9);
  assert_eq!(
    fixture.profile.canonicalize()?,
    fixture.system(20).canonicalize()?
  );
  Ok(())
}

#[test]
fn rollback_to_current_generation_does_not_reselect_on_failure() -> Result<()> {
  let fixture = Fixture::new()?;
  let selections = RefCell::new(Vec::new());
  let mut options = args();
  options.to = Some(12);
  let error = fixture
    .run(
      options,
      |number| {
        selections.borrow_mut().push(number);
        fixture.select(number)
      },
      |_| bail!("activation failed"),
    )
    .unwrap_err();
  assert!(error.to_string().contains("restored to generation 12"));
  assert_eq!(*selections.borrow(), [12]);
  Ok(())
}
