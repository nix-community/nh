use nh_cli::common as cli;
use nh_core::{args, update};

pub(super) fn common_rebuild(
  common: cli::CommonRebuildArgs,
) -> args::CommonRebuildArgs {
  let cli::CommonRebuildArgs {
    dry,
    ask,
    installable: installable_args,
    no_rom,
    no_nom,
    out_link,
    diff: diff_args,
    passthrough: passthrough_args,
  } = common;
  args::CommonRebuildArgs {
    dry,
    ask,
    installable: installable(installable_args),
    no_rom,
    no_nom,
    out_link,
    diff: diff(diff_args),
    passthrough: passthrough(passthrough_args),
  }
}

pub(super) fn installable(
  installable: nh_cli::installable::InstallableArgs,
) -> nh_installable::InstallableArgs {
  let nh_cli::installable::InstallableArgs(installable) = installable;
  installable
}

pub(super) const fn diff(diff: cli::Diff) -> args::Diff {
  let cli::Diff(diff) = diff;
  diff
}

fn passthrough(
  passthrough: cli::NixBuildPassthroughArgs,
) -> args::NixBuildPassthroughArgs {
  let cli::NixBuildPassthroughArgs {
    max_jobs,
    cores,
    log_format,
    keep_going,
    keep_failed,
    fallback,
    repair,
    builders,
    include,
    print_build_logs,
    show_trace,
    accept_flake_config,
    refresh,
    impure,
    offline,
    no_net,
    recreate_lock_file,
    no_update_lock_file,
    no_write_lock_file,
    no_use_registries,
    no_registries,
    commit_lock_file,
    no_build_output,
    use_substitutes,
    json,
    option,
    override_input,
  } = passthrough;
  args::NixBuildPassthroughArgs {
    max_jobs,
    cores,
    log_format,
    keep_going,
    keep_failed,
    fallback,
    repair,
    builders,
    include,
    print_build_logs,
    show_trace,
    accept_flake_config,
    refresh,
    impure,
    offline,
    no_net,
    recreate_lock_file,
    no_update_lock_file,
    no_write_lock_file,
    no_use_registries,
    no_registries,
    commit_lock_file,
    no_build_output,
    use_substitutes,
    json,
    option,
    override_input,
  }
}

pub(super) fn update(update_args: cli::UpdateArgs) -> update::UpdateArgs {
  let cli::UpdateArgs {
    update_all,
    update_input,
  } = update_args;
  update::UpdateArgs {
    update_all,
    update_input,
  }
}
