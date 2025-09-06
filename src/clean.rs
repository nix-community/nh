use std::{
  collections::{BTreeMap, HashMap},
  fmt,
  path::{Path, PathBuf},
  sync::LazyLock,
  time::SystemTime,
};

use color_eyre::eyre::{Context, ContextCompat, bail, eyre};
use inquire::Confirm;
use nix::{
  errno::Errno,
  fcntl::AtFlags,
  unistd::{AccessFlags, faccessat},
};
use regex::Regex;
use tracing::{Level, debug, info, instrument, span, warn};

use crate::{
  Result,
  commands::{Command, ElevationStrategy},
  interface,
};

// Nix impl:
// https://github.com/NixOS/nix/blob/master/src/nix-collect-garbage/nix-collect-garbage.cc

static DIRENV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r".*/.direnv/.*").expect("Failed to compile direnv regex")
});

static RESULT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r".*result.*").expect("Failed to compile result regex")
});

static GENERATION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(.*)-(\d+)-link$").expect("Failed to compile generation regex")
});

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

impl interface::CleanMode {
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
    use owo_colors::OwoColorize;

    let mut profiles = Vec::new();
    let mut gcroots_tagged: HashMap<PathBuf, ToBeRemoved> = HashMap::new();
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
          crate::util::self_elevate(elevate);
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
    let regexes = [&*DIRENV_REGEX, &*RESULT_REGEX];

    if !is_profile_clean && !args.no_gcroots {
      for elem in PathBuf::from("/nix/var/nix/gcroots/auto")
        .read_dir()
        .wrap_err("Reading auto gcroots dir")?
      {
        let src = elem.wrap_err("Reading auto gcroots element")?.path();
        let dst = src.read_link().wrap_err("Reading symlink destination")?;
        let span = span!(Level::TRACE, "gcroot detection", ?dst);
        let _entered = span.enter();
        debug!(?src);

        if !regexes
          .iter()
          .any(|next| next.is_match(&dst.to_string_lossy()))
        {
          debug!("dst doesn't match any gcroot regex, skipping");
          continue;
        }

        // Create a file descriptor for the current working directory
        let dirfd = nix::fcntl::open(
          ".",
          nix::fcntl::OFlag::O_DIRECTORY,
          nix::sys::stat::Mode::empty(),
        )?;

        // Use .exists to not travel symlinks
        if match faccessat(
          &dirfd,
          &dst,
          AccessFlags::F_OK | AccessFlags::W_OK,
          AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
          Ok(()) => true,
          Err(errno) => {
            match errno {
              Errno::EACCES | Errno::ENOENT => false,
              _ => {
                bail!(
                  eyre!("Checking access for gcroot {:?}, unknown error", dst)
                    .wrap_err(errno)
                )
              },
            }
          },
        } {
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
              gcroots_tagged.insert(dst, false);
            },
            Ok(_) => {
              gcroots_tagged.insert(dst, true);
            },
          }
        } else {
          debug!("dst doesn't exist or is not writable, skipping");
        }
      }
    }

    // Present the user the information about the paths to clean
    println!();
    println!("{}", "Welcome to nh clean".bold());
    println!("Keeping {} generation(s)", args.keep.green());
    println!("Keeping paths newer than {}", args.keep_since.green());
    println!();
    println!("legend:");
    println!("{}: path regular expression to be matched", "RE".purple());
    println!("{}: path to be kept", "OK".green());
    println!("{}: path to be removed", "DEL".red());
    println!();
    if !gcroots_tagged.is_empty() {
      println!(
        "{}",
        "gcroots (matching the following regex patterns)"
          .blue()
          .bold()
      );
      for re in regexes {
        println!("- {}  {}", "RE".purple(), re.as_str());
      }
      for (path, tbr) in &gcroots_tagged {
        if *tbr {
          println!("- {} {}", "DEL".red(), path.to_string_lossy());
        } else {
          println!("- {} {}", "OK ".green(), path.to_string_lossy());
        }
      }
      println!();
    }
    for (profile, generations_tagged) in &profiles_tagged {
      println!("{}", profile.to_string_lossy().blue().bold());
      for (generation, tbr) in generations_tagged.iter().rev() {
        if *tbr {
          println!("- {} {}", "DEL".red(), generation.path.to_string_lossy());
        } else {
          println!("- {} {}", "OK ".green(), generation.path.to_string_lossy());
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
      for (path, tbr) in &gcroots_tagged {
        if *tbr {
          remove_path_nofail(path);
        }
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
        .args(["--optimise"])
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
      if let Some(profile_name) = caps.get(1) {
        if profile_name.as_str() != name {
          continue;
        }
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

fn remove_path_nofail(path: &Path) {
  info!("Removing {}", path.to_string_lossy());
  if let Err(err) = std::fs::remove_file(path) {
    warn!(?path, ?err, "Failed to remove path");
  }
}
