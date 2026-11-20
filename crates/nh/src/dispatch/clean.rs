use nh_clean::{CleanOptions, CleanTarget};
use nh_cli::clean::{CleanArgs, CleanMode, CleanProfileArgs, CleanProxy};
use nh_core::command::ElevationStrategy;

use crate::Result;

pub(super) fn run(
  proxy: CleanProxy,
  elevation: ElevationStrategy,
) -> Result<()> {
  let (target, options) = target_and_options(proxy.command);
  target.run(&options, elevation)
}

fn target_and_options(mode: CleanMode) -> (CleanTarget, CleanOptions) {
  match mode {
    CleanMode::All { args } => (CleanTarget::All, options(args)),
    CleanMode::User { args } => (CleanTarget::User, options(args)),
    CleanMode::Profile {
      args: CleanProfileArgs { common, profile },
    } => (CleanTarget::Profile(profile), options(common)),
  }
}

fn options(args: CleanArgs) -> CleanOptions {
  let CleanArgs {
    keep,
    keep_since,
    delete_current,
    dry,
    ask,
    no_gc,
    no_gcroots,
    no_direnv,
    optimise,
    max,
    keep_one,
    cross_filesystems,
  } = args;
  CleanOptions {
    keep,
    keep_since,
    delete_current,
    dry,
    ask,
    no_gc,
    no_gcroots,
    no_direnv,
    optimise,
    max,
    keep_one,
    cross_filesystems,
  }
}
