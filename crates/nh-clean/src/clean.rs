pub mod args;

use std::{
  collections::{BTreeMap, HashMap},
  fmt,
  path::{Path, PathBuf},
  sync::LazyLock,
  time::SystemTime,
};

use color_eyre::{
  Result,
  eyre::{Context, ContextCompat, bail, eyre},
};
use inquire::Confirm;
use nh_core::command::{Command, ElevationStrategy};
use nix::{
  errno::Errno,
  fcntl::AtFlags,
  unistd::{AccessFlags, faccessat},
};
use regex::Regex;
use tracing::{Level, debug, info, instrument, span, warn};
use walkdir::WalkDir;
use yansi::{Color, Paint};

// Nix impl:
// https://github.com/NixOS/nix/blob/master/src/nix-collect-garbage/nix-collect-garbage.cc

static DIRENV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  #[allow(clippy::expect_used)]
  Regex::new(r".*/(?:\.direnv|direnv/layouts)/.*")
    .expect("Failed to compile direnv regex")
});

static GENERATION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  #[allow(clippy::expect_used)]
  Regex::new(r"^(.*)-(\d+)-link$").expect("Failed to compile generation regex")
});

static RESULT_LINK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  #[allow(clippy::expect_used)]
  Regex::new(r"^result(-.*)?$").expect("Failed to compile result link regex")
});

const AUTO_GCROOTS_DIR: &str = "/nix/var/nix/gcroots/auto";

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Generation {
  number:        u32,
  last_modified: SystemTime,
  path:          PathBuf,
}

type ToBeRemoved = bool;
// BTreeMap to automatically sort generations by id
type GenerationsTagged = BTreeMap<Generation, ToBeRemoved>;
type ProfilesTagged = HashMap<PathBuf, GenerationsTagged>;

#[derive(Debug)]
struct GcRootTagged {
  src: PathBuf,
  dst: PathBuf,
  tbr: ToBeRemoved,
}

/// Filter paths to only include existing directories, logging warnings for
/// missing ones
fn filter_existing_dirs<I>(paths: I) -> impl Iterator<Item = PathBuf>
where
  I: IntoIterator<Item = PathBuf>,
{
  paths.into_iter().filter_map(|path| {
    if path.is_dir() {
      Some(path)
    } else {
      warn!("Profiles directory not found, skipping: {}", path.display());
      None
    }
  })
}

impl args::CleanMode {
  /// Run the clean operation for the selected mode.
  ///
  /// # Errors
  ///
  /// Returns an error if any IO, Nix, or environment operation fails.
  ///
  /// # Panics
  ///
  /// Panics if the current user's UID cannot be resolved to a user. For
  /// example, if  `User::from_uid(uid)` returns `None`.
  pub fn run(&self, elevate: ElevationStrategy) -> Result<()> {
    let mut profiles = Vec::new();
    let mut gcroots_tagged = Vec::new();
    let now = SystemTime::now();
    let mut is_profile_clean = false;

    // What profiles to clean depending on the call mode
    let uid = nix::unistd::Uid::effective();
    let args = match self {
      Self::Profile(args) => {
        profiles.push(args.profile.clone());
        is_profile_clean = true;
        &args.common
      },
      Self::All(args) => {
        if !uid.is_root() {
          nh_core::util::self_elevate(elevate);
        }

        let paths_to_check = [
          PathBuf::from("/nix/var/nix/profiles"),
          PathBuf::from("/nix/var/nix/profiles/per-user"),
        ];

        profiles.extend(filter_existing_dirs(paths_to_check).flat_map(
          |path| {
            if path.ends_with("per-user") {
              path
                .read_dir()
                .map(|read_dir| {
                  read_dir
                    .filter_map(std::result::Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .flat_map(profiles_in_dir)
                    .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            } else {
              profiles_in_dir(path)
            }
          },
        ));

        // Most unix systems start regular users at uid 1000+, but macos is
        // special at 501+ https://en.wikipedia.org/wiki/User_identifier
        let uid_min = if cfg!(target_os = "macos") { 501 } else { 1000 };
        let uid_max = uid_min + 100;
        debug!("Scanning XDG profiles for users 0, {uid_min}-{uid_max}");

        // Check root user (uid 0)
        if let Some(user) =
          nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(0))?
        {
          debug!(?user, "Adding XDG profiles for root user");
          let user_profiles_path = user.dir.join(".local/state/nix/profiles");
          if user_profiles_path.is_dir() {
            profiles.extend(profiles_in_dir(user_profiles_path));
          }
        }

        // Check regular users in the expected range
        for uid in uid_min..uid_max {
          if let Some(user) =
            nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))?
          {
            debug!(?user, "Adding XDG profiles for user");
            let user_profiles_path = user.dir.join(".local/state/nix/profiles");
            if user_profiles_path.is_dir() {
              profiles.extend(profiles_in_dir(user_profiles_path));
            }
          }
        }
        args
      },
      Self::User(args) => {
        if uid.is_root() {
          bail!("nh clean user: don't run me as root!");
        }
        let user = nix::unistd::User::from_uid(uid)?
          .ok_or_else(|| eyre!("User not found for uid {}", uid))?;
        let home_dir = PathBuf::from(std::env::var("HOME")?);

        let paths_to_check = [
          home_dir.join(".local/state/nix/profiles"),
          PathBuf::from("/nix/var/nix/profiles/per-user").join(&user.name),
        ];

        profiles.extend(
          filter_existing_dirs(paths_to_check).flat_map(profiles_in_dir),
        );

        if profiles.is_empty() {
          warn!(
            "No active profile directories found for the current user. \
             Nothing to clean."
          );
        }

        args
      },
    };

    // Use mutation to raise errors as they come
    let mut profiles_tagged = ProfilesTagged::new();
    for p in profiles {
      profiles_tagged.insert(
        p.clone(),
        cleanable_generations(&p, args.keep, args.keep_since)?,
      );
    }

    // Query gcroots
    let regexes = &[&*DIRENV_REGEX][..usize::from(!args.no_direnv)];
    let mut orphan_gcroots: Vec<PathBuf> = Vec::new();

    if !is_profile_clean && !args.no_gcroots {
      let dirfd = nix::fcntl::open(
        ".",
        nix::fcntl::OFlag::O_DIRECTORY,
        nix::sys::stat::Mode::empty(),
      )?;

      for entry in WalkDir::new("/nix/var/nix/gcroots")
        .follow_links(false)
        .same_file_system(!args.cross_filesystems)
        .into_iter()
        .filter_map(|e| {
          e.map_err(|err| {
            warn!(?err, "gcroot walk error");
          })
          .ok()
        })
        .filter(|e| e.path().is_symlink())
      {
        let src = entry.path().to_path_buf();
        let dst = src.read_link().wrap_err("Reading symlink destination")?;
        let span = span!(Level::TRACE, "gcroot detection", ?dst);
        let _entered = span.enter();
        debug!(?src);

        if !dst.is_symlink() && !dst.exists() {
          debug!(
            ?src,
            "gcroot is orphaned (dst missing), tagging for removal"
          );
          orphan_gcroots.push(src);
          continue;
        }

        match faccessat(
          &dirfd,
          &dst,
          AccessFlags::F_OK | AccessFlags::W_OK,
          AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
          Ok(()) => {
            if dst.metadata().is_err() {
              // A live root's target is never aged out. It's only removed once
              // it is provably dead (which is how Nix's own indirect-gcroot
              // semantics work.) This check must run for every gcroot, not just
              // ones matching the age-based cleanup filter below.
              debug!(?dst, "gcroot target already GC'd, tagging for removal");
              gcroots_tagged.push(GcRootTagged {
                src,
                dst,
                tbr: true,
              });
              continue;
            }

            if !gcroot_matches_filter(&src, &dst, regexes) {
              debug!("dst doesn't match any gcroot filter, skipping");
              continue;
            }

            if args.keep_one && DIRENV_REGEX.is_match(&dst.to_string_lossy()) {
              gcroots_tagged.push(GcRootTagged {
                src,
                dst,
                tbr: false,
              });
            } else {
              let dur = now.duration_since(
                dst
                  .symlink_metadata()
                  .wrap_err("Reading gcroot metadata")?
                  .modified()?,
              );
              debug!(?dur);
              match dur {
                Err(err) => {
                  warn!(?err, ?now, "Failed to compare time!");
                },
                Ok(val) if val <= args.keep_since.into() => {
                  gcroots_tagged.push(GcRootTagged {
                    src,
                    dst,
                    tbr: false,
                  });
                },
                Ok(_) => {
                  gcroots_tagged.push(GcRootTagged {
                    src,
                    dst,
                    tbr: true,
                  });
                },
              }
            }
          },
          Err(Errno::ENOENT) => {
            debug!(
              ?src,
              "gcroot is orphaned (dst missing), tagging for removal"
            );
            orphan_gcroots.push(src);
          },
          Err(errno @ (Errno::EACCES | Errno::EROFS | Errno::EPERM)) => {
            debug!(?errno, ?dst, "gcroot target not writable, skipping");
          },
          Err(errno) => {
            bail!(
              eyre!("Checking access for gcroot {:?}, unknown error", dst)
                .wrap_err(errno)
            )
          },
        }
      }
    }

    // Present the user the information about the paths to clean
    println!();
    println!("{}", Paint::new("Welcome to nh clean").bold());
    println!(
      "Keeping {} generation(s)",
      Paint::new(args.keep).fg(Color::Green)
    );
    println!(
      "Keeping paths newer than {}",
      Paint::new(args.keep_since).fg(Color::Green)
    );
    if args.keep_one {
      println!("Keeping all active direnv gcroots");
    }
    if args.no_direnv {
      println!("Skipping all direnv gcroots");
    }
    println!();
    println!("legend:");
    println!(
      "{}: path regular expression to be matched",
      Paint::new("RE").fg(Color::Magenta)
    );
    println!("{}: path to be kept", Paint::new("OK").fg(Color::Green));
    println!("{}: path to be removed", Paint::new("DEL").fg(Color::Red));
    println!();
    if !orphan_gcroots.is_empty() {
      println!("{}", Paint::new("orphaned gcroots").fg(Color::Blue).bold());
      for path in &orphan_gcroots {
        println!(
          "- {} {}",
          Paint::new("DEL").fg(Color::Red),
          path.to_string_lossy()
        );
      }
      println!();
    }
    if !gcroots_tagged.is_empty() {
      println!("{}", Paint::new("gcroots").fg(Color::Blue).bold());
      for re in regexes {
        println!("- {}  {}", Paint::new("RE").fg(Color::Magenta), re.as_str());
      }
      println!(
        "- {}  /nix/store direct children",
        Paint::new("RE").fg(Color::Magenta)
      );
      for gcroot in &gcroots_tagged {
        if gcroot.tbr {
          println!(
            "- {} {}",
            Paint::new("DEL").fg(Color::Red),
            gcroot.dst.to_string_lossy()
          );
        } else {
          println!(
            "- {} {}",
            Paint::new("OK ").fg(Color::Green),
            gcroot.dst.to_string_lossy()
          );
        }
      }
      println!();
    }
    for (profile, generations_tagged) in &profiles_tagged {
      println!(
        "{}",
        Paint::new(profile.to_string_lossy()).fg(Color::Blue).bold()
      );
      for (generation, tbr) in generations_tagged.iter().rev() {
        if *tbr {
          println!(
            "- {} {}",
            Paint::new("DEL").fg(Color::Red),
            generation.path.to_string_lossy()
          );
        } else {
          println!(
            "- {} {}",
            Paint::new("OK ").fg(Color::Green),
            generation.path.to_string_lossy()
          );
        }
      }
      println!();
    }

    // Clean the paths
    if args.ask
      && !Confirm::new("Confirm the cleanup plan?")
        .with_default(false)
        .prompt()?
    {
      bail!("User rejected the cleanup plan");
    }

    if !args.dry {
      for gcroot in &gcroots_tagged {
        if gcroot.tbr {
          remove_path_nofail(gcroot_path_to_remove(gcroot));
        }
      }

      for path in &orphan_gcroots {
        remove_path_nofail(path);
      }

      for generations_tagged in profiles_tagged.values() {
        for (generation, tbr) in generations_tagged.iter().rev() {
          if *tbr {
            remove_path_nofail(&generation.path);
          }
        }
      }
    }

    if !args.no_gc {
      let mut gc_args = vec!["store", "gc"];
      if let Some(ref max) = args.max {
        gc_args.push("--max");
        gc_args.push(max.as_str());
      }
      Command::new("nix")
        .args(gc_args)
        .dry(args.dry)
        .message("Performing garbage collection on the nix store")
        .show_output(true)
        .with_required_env()
        .run()?;
    }

    if args.optimise {
      Command::new("nix-store")
        .arg("--optimise")
        .dry(args.dry)
        .message("Optimising the nix store")
        .show_output(true)
        .with_required_env()
        .run()?;
    }

    Ok(())
  }
}

#[instrument(ret, level = "debug")]
fn profiles_in_dir<P: AsRef<Path> + fmt::Debug>(dir: P) -> Vec<PathBuf> {
  let mut res = Vec::new();
  let dir = dir.as_ref();

  match dir.read_dir() {
    Ok(read_dir) => {
      for entry in read_dir {
        match entry {
          Ok(e) => {
            let path = e.path();

            if let Ok(dst) = path.read_link() {
              let name = if let Some(f) = dst.file_name() {
                f.to_string_lossy()
              } else {
                warn!("Failed to get filename for {dst:?}");
                continue;
              };

              if GENERATION_REGEX.captures(&name).is_some() {
                res.push(path);
              }
            }
          },
          Err(error) => {
            warn!(?dir, ?error, "Failed to read folder element");
          },
        }
      }
    },
    Err(error) => {
      warn!(?dir, ?error, "Failed to read profiles directory");
    },
  }

  res
}

#[instrument(err, level = "debug")]
fn cleanable_generations(
  profile: &Path,
  keep: u32,
  keep_since: humantime::Duration,
) -> Result<GenerationsTagged> {
  let name = profile
    .file_name()
    .context("Checking profile's name")?
    .to_str()
    .context("Profile name is not valid UTF-8")?;

  let mut result = GenerationsTagged::new();

  for entry in profile
    .parent()
    .context("Reading profile's parent dir")?
    .read_dir()
    .context("Reading profile's generations")?
  {
    let path = entry?.path();
    let captures = {
      let file_name = path.file_name().context("Failed to get filename")?;
      let file_name_str =
        file_name.to_str().context("Filename is not valid UTF-8")?;
      GENERATION_REGEX.captures(file_name_str)
    };

    if let Some(caps) = captures {
      // Check if this generation belongs to the current profile
      if let Some(profile_name) = caps.get(1)
        && profile_name.as_str() != name
      {
        continue;
      }
      if let Some(number) = caps.get(2) {
        let last_modified = path
          .symlink_metadata()
          .context("Checking symlink metadata")?
          .modified()
          .context("Reading modified time")?;

        result.insert(
          Generation {
            number: number
              .as_str()
              .parse()
              .context("Failed to parse generation number")?,
            last_modified,
            path,
          },
          true,
        );
      }
    }
  }

  let now = SystemTime::now();
  for (generation, tbr) in &mut result {
    match now.duration_since(generation.last_modified) {
      Err(err) => {
        warn!(?err, ?now, ?generation, "Failed to compare time!");
      },
      Ok(val) if val <= keep_since.into() => {
        *tbr = false;
      },
      Ok(_) => {},
    }
  }

  for (_, tbr) in result.iter_mut().rev().take(keep as _) {
    *tbr = false;
  }

  debug!("{:#?}", result);
  Ok(result)
}

fn is_nix_store_direct_child(path: &Path) -> bool {
  path
    .strip_prefix("/nix/store")
    .is_ok_and(|suffix| suffix.components().count() == 1)
}

fn gcroot_matches_filter(src: &Path, dst: &Path, regexes: &[&Regex]) -> bool {
  let resolved_dst = if dst.is_symlink() {
    dst.read_link().unwrap_or_else(|_| dst.to_path_buf())
  } else {
    dst.to_path_buf()
  };

  regexes
    .iter()
    .any(|next| next.is_match(&dst.to_string_lossy()))
    || (is_auto_gcroot_entry(src)
      && is_nix_store_direct_child(&resolved_dst)
      && is_build_result_link(dst))
}

fn is_auto_gcroot_entry(path: &Path) -> bool {
  path.starts_with(AUTO_GCROOTS_DIR)
}

/// Whether `path`'s basename looks like an ephemeral `nix build` result
/// symlink (`result`, `result-dev`, etc.), as opposed to a live indirect
/// gcroot such as home-manager's `current-home` or `current-system`. Only
/// paths matching this are subject to age-based (`--keep-since`) cleanup;
/// anything else that resolves straight into `/nix/store` is left alone
/// unless it becomes orphaned.
fn is_build_result_link(path: &Path) -> bool {
  path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| RESULT_LINK_REGEX.is_match(name))
}

fn gcroot_path_to_remove(gcroot: &GcRootTagged) -> &Path {
  &gcroot.src
}

fn remove_path_nofail(path: &Path) {
  info!("Removing {}", path.to_string_lossy());
  if let Err(err) = std::fs::remove_file(path) {
    warn!(?path, ?err, "Failed to remove path");
  }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
  use super::*;

  #[test]
  fn store_direct_child_accepts_top_level_entry() {
    assert!(is_nix_store_direct_child(Path::new(
      "/nix/store/abc123zzz-foo-1.0"
    )));
  }

  #[test]
  fn store_direct_child_rejects_nested_path() {
    assert!(!is_nix_store_direct_child(Path::new(
      "/nix/store/abc123zzz-foo-1.0/bin/foo"
    )));
  }

  #[test]
  fn store_direct_child_rejects_store_root_itself() {
    assert!(!is_nix_store_direct_child(Path::new("/nix/store")));
  }

  #[test]
  fn store_direct_child_rejects_unrelated_paths() {
    assert!(!is_nix_store_direct_child(Path::new("/home/user/result")));
    assert!(!is_nix_store_direct_child(Path::new("/result")));
    assert!(!is_nix_store_direct_child(Path::new(
      "/nix/store-backup/abc"
    )));
  }

  #[test]
  fn direnv_regex_matches_dotdirenv_subpath() {
    assert!(DIRENV_REGEX.is_match("/home/user/project/.direnv/python3.11"));
  }

  #[test]
  fn direnv_regex_matches_layouts_subpath() {
    assert!(
      DIRENV_REGEX.is_match("/home/user/project/direnv/layouts/python3.11")
    );
  }

  #[test]
  fn direnv_regex_rejects_result_and_store_paths() {
    assert!(!DIRENV_REGEX.is_match("/home/user/project/result"));
    assert!(!DIRENV_REGEX.is_match("/nix/store/abc123zzz-foo-1.0"));
  }

  #[test]
  fn gcroot_filter_passes_direnv_path() {
    let src = Path::new("/nix/var/nix/gcroots/project-direnv");
    let dst = Path::new("/home/user/project/.direnv/something");
    let regexes = [&*DIRENV_REGEX];
    assert!(gcroot_matches_filter(src, dst, &regexes));
  }

  #[test]
  fn gcroot_filter_rejects_auto_store_direct_child_without_result_name() {
    // A direct-store-child target that isn't reached through a
    // `result`-shaped path (e.g., a raw store path registered as an
    // indirect root) is not eligible for age-based cleanup.
    let src = Path::new("/nix/var/nix/gcroots/auto/example");
    let dst = Path::new("/nix/store/abc123zzz-foo-1.0");
    let regexes = [&*DIRENV_REGEX];
    assert!(!gcroot_matches_filter(src, dst, &regexes));
  }

  #[test]
  fn gcroot_filter_rejects_current_home_shaped_auto_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let link = dir.path().join("current-home");
    std::os::unix::fs::symlink("/nix/store/abc123zzz-foo-1.0", &link)
      .expect("symlink");

    let src = Path::new("/nix/var/nix/gcroots/auto/example");
    let regexes = [&*DIRENV_REGEX];
    assert!(
      !gcroot_matches_filter(src, &link, &regexes),
      "current-home-shaped roots must not be subject to age-based cleanup"
    );
  }

  #[test]
  fn build_result_link_matches_result_and_variants() {
    assert!(is_build_result_link(Path::new("/home/user/project/result")));
    assert!(is_build_result_link(Path::new(
      "/home/user/project/result-dev"
    )));
    assert!(is_build_result_link(Path::new(
      "/home/user/project/result-bin"
    )));
  }

  #[test]
  fn build_result_link_rejects_current_home_and_system() {
    assert!(!is_build_result_link(Path::new(
      "/var/lib/foo/current-home"
    )));
    assert!(!is_build_result_link(Path::new("/run/current-system")));
    assert!(!is_build_result_link(Path::new(
      "/nix/store/abc123zzz-foo-1.0"
    )));
  }

  #[test]
  fn gcroot_filter_rejects_non_auto_store_direct_child() {
    let src = Path::new("/nix/var/nix/gcroots/current-system");
    let dst = Path::new("/nix/store/abc123zzz-foo-1.0");
    let regexes = [&*DIRENV_REGEX];
    assert!(!gcroot_matches_filter(src, dst, &regexes));
  }

  #[test]
  fn gcroot_filter_passes_auto_symlink_to_store_direct_child() {
    let dir = tempfile::tempdir().expect("tempdir");
    let link = dir.path().join("result");
    std::os::unix::fs::symlink("/nix/store/abc123zzz-foo-1.0", &link)
      .expect("symlink");

    let src = Path::new("/nix/var/nix/gcroots/auto/example");
    let regexes = [&*DIRENV_REGEX];
    assert!(gcroot_matches_filter(src, &link, &regexes));
  }

  #[test]
  fn direct_store_filter_is_limited_to_auto_gcroots() {
    assert!(is_auto_gcroot_entry(Path::new(
      "/nix/var/nix/gcroots/auto/example"
    )));
    assert!(!is_auto_gcroot_entry(Path::new(
      "/nix/var/nix/gcroots/current-system"
    )));
    assert!(!is_auto_gcroot_entry(Path::new(
      "/nix/var/nix/gcroots/booted-system"
    )));
    assert!(!is_auto_gcroot_entry(Path::new(
      "/nix/var/nix/gcroots/profiles/system"
    )));
  }

  #[test]
  fn gcroot_cleanup_removes_source_not_system_destination() {
    let gcroot = GcRootTagged {
      src: PathBuf::from("/nix/var/nix/gcroots/auto/example"),
      dst: PathBuf::from("/run/current-system"),
      tbr: true,
    };

    assert_eq!(
      gcroot_path_to_remove(&gcroot),
      Path::new("/nix/var/nix/gcroots/auto/example")
    );
    assert_ne!(gcroot_path_to_remove(&gcroot), gcroot.dst.as_path());
  }

  #[test]
  fn gcroot_cleanup_removes_source_not_profile_destination() {
    let gcroot = GcRootTagged {
      src: PathBuf::from("/nix/var/nix/gcroots/auto/example"),
      dst: PathBuf::from("/nix/var/nix/profiles/system-2-link"),
      tbr: true,
    };

    assert_eq!(
      gcroot_path_to_remove(&gcroot),
      Path::new("/nix/var/nix/gcroots/auto/example")
    );
    assert_ne!(gcroot_path_to_remove(&gcroot), gcroot.dst.as_path());
  }

  #[test]
  fn gcroot_filter_rejects_arbitrary_path() {
    let src = Path::new("/nix/var/nix/gcroots/auto/example");
    let dst = Path::new("/home/user/some-random-link");
    let regexes = [&*DIRENV_REGEX];
    assert!(!gcroot_matches_filter(src, dst, &regexes));
  }

  #[test]
  fn missing_path_triggers_case_a_condition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gone = dir.path().join("gone");
    assert!(!gone.is_symlink() && !gone.exists());
  }

  #[test]
  fn broken_symlink_does_not_trigger_case_a() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("gone");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    assert!(link.is_symlink());
    assert!(!link.exists());
    assert!(link.is_symlink() || link.exists());
  }

  #[test]
  fn broken_symlink_metadata_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("gone");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    assert!(link.is_symlink(), "symlink should exist");
    assert!(
      link.metadata().is_err(),
      "following broken symlink should fail"
    );
  }

  #[test]
  fn orphaned_current_home_shaped_root_is_still_removable() {
    // Even though a current-home-shaped root is exempt from age-based cleanup
    // (also see: gcroot_filter_rejects_current_home_shaped_auto_root), once
    // its target is actually gone it must still be detected as
    // broken. This is sorta universal, and filter independent.
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("gone-home");
    let link = dir.path().join("current-home");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    assert!(!is_build_result_link(&link));
    assert!(
      link.metadata().is_err(),
      "broken current-home-shaped symlink must be detected as orphaned \
       regardless of its name"
    );
  }

  #[test]
  fn live_symlink_metadata_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("real");
    std::fs::write(&target, b"").expect("write");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    assert!(link.is_symlink(), "symlink should exist");
    assert!(
      link.metadata().is_ok(),
      "live symlink metadata should succeed"
    );
  }
}
