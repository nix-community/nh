use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

use color_eyre::eyre::{Context, ContextCompat, bail, eyre};
use nix::errno::Errno;
use nix::{
    fcntl::AtFlags,
    unistd::{AccessFlags, faccessat},
};
use regex::Regex;
use tracing::{Level, debug, info, instrument, span, warn};
use uzers::os::unix::UserExt;

use crate::{Result, commands::Command, interface};

// Nix impl:
// https://github.com/NixOS/nix/blob/master/src/nix-collect-garbage/nix-collect-garbage.cc

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Generation {
    number: u32,
    last_modified: SystemTime,
    path: PathBuf,
}

type ToBeRemoved = bool;
// BTreeMap to automatically sort generations by id
type GenerationsTagged = BTreeMap<Generation, ToBeRemoved>;
type ProfilesTagged = HashMap<PathBuf, GenerationsTagged>;

impl interface::CleanMode {
    pub fn run(&self) -> Result<()> {
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
            }
            Self::All(args) => {
                if !uid.is_root() {
                    crate::self_elevate();
                }
                profiles.extend(profiles_in_dir("/nix/var/nix/profiles"));
                for read_dir in PathBuf::from("/nix/var/nix/profiles/per-user").read_dir()? {
                    let path = read_dir?.path();
                    profiles.extend(profiles_in_dir(path));
                }

                // Most unix systems start regular users at uid 1000+, but macos is special at 501+
                // https://en.wikipedia.org/wiki/User_identifier
                let uid_min = if cfg!(target_os = "macos") { 501 } else { 1000 };
                let uid_max = uid_min + 100;
                debug!("Scanning XDG profiles for users 0, ${uid_min}-${uid_max}");
                for user in unsafe { uzers::all_users() } {
                    if user.uid() >= uid_min && user.uid() < uid_max || user.uid() == 0 {
                        debug!(?user, "Adding XDG profiles for user");
                        profiles.extend(profiles_in_dir(
                            user.home_dir().join(".local/state/nix/profiles"),
                        ));
                    }
                }
                args
            }
            Self::User(args) => {
                if uid.is_root() {
                    bail!("nh clean user: don't run me as root!");
                }
                let user = nix::unistd::User::from_uid(uid)?.unwrap();
                profiles.extend(profiles_in_dir(
                    PathBuf::from(std::env::var("HOME")?).join(".local/state/nix/profiles"),
                ));
                profiles.extend(profiles_in_dir(
                    PathBuf::from("/nix/var/nix/profiles/per-user").join(user.name),
                ));
                args
            }
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
        let filename_tests = [r".*/.direnv/.*", r".*result.*"];
        let regexes = filename_tests
            .into_iter()
            .map(Regex::new)
            .collect::<Result<Vec<_>, regex::Error>>()?;

        if !is_profile_clean && !args.nogcroots {
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
                    Err(errno) => match errno {
                        Errno::EACCES | Errno::ENOENT => false,
                        _ => {
                            bail!(
                                eyre!("Checking access for gcroot {:?}, unknown error", dst)
                                    .wrap_err(errno)
                            )
                        }
                    },
                } {
                    let dur = now.duration_since(
                        dst.symlink_metadata()
                            .wrap_err("Reading gcroot metadata")?
                            .modified()?,
                    );
                    debug!(?dur);
                    match dur {
                        Err(err) => {
                            warn!(?err, ?now, "Failed to compare time!");
                        }
                        Ok(val) if val <= args.keep_since.into() => {
                            gcroots_tagged.insert(dst, false);
                        }
                        Ok(_) => {
                            gcroots_tagged.insert(dst, true);
                        }
                    }
                } else {
                    debug!("dst doesn't exist or is not writable, skipping");
                }
            }
        }

        // Present the user the information about the paths to clean
        use owo_colors::OwoColorize;
        println!();
        println!("{}", "Welcome to nh clean".bold());
        println!("Keeping {} generation(s)", args.keep.green());
        println!("Keeping paths newer than {}", args.keep_since.green());
        println!();
        println!("legend:");
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
                println!("- {}  {}", "RE".purple(), re);
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
        if args.ask {
            info!("Confirm the cleanup plan?");
            if !dialoguer::Confirm::new().default(false).interact()? {
                bail!("User rejected the cleanup plan");
            }
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

        if !args.nogc {
            Command::new("nix")
                .args(["store", "gc"])
                .dry(args.dry)
                .message("Performing garbage collection on the nix store")
                .show_output(true)
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
                            let name = dst
                                .file_name()
                                .expect("Failed to get filename")
                                .to_string_lossy();

                            let generation_regex = Regex::new(r"^(.*)-(\d+)-link$").unwrap();

                            if generation_regex.captures(&name).is_some() {
                                res.push(path);
                            }
                        }
                    }
                    Err(error) => {
                        warn!(?dir, ?error, "Failed to read folder element");
                    }
                }
            }
        }
        Err(error) => {
            warn!(?dir, ?error, "Failed to read profiles directory");
        }
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
        .unwrap();

    let generation_regex = Regex::new(&format!(r"^{name}-(\d+)-link"))?;

    let mut result = GenerationsTagged::new();

    for entry in profile
        .parent()
        .context("Reading profile's parent dir")?
        .read_dir()
        .context("Reading profile's generations")?
    {
        let path = entry?.path();
        let captures = generation_regex.captures(path.file_name().unwrap().to_str().unwrap());

        if let Some(caps) = captures {
            if let Some(number) = caps.get(1) {
                let last_modified = path
                    .symlink_metadata()
                    .context("Checking symlink metadata")?
                    .modified()
                    .context("Reading modified time")?;

                result.insert(
                    Generation {
                        number: number.as_str().parse().unwrap(),
                        last_modified,
                        path: path.clone(),
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
            }
            Ok(val) if val <= keep_since.into() => {
                *tbr = false;
            }
            Ok(_) => {}
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
