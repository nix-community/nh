use std::path::PathBuf;

use anstyle::Style;
use clap::ValueEnum;
use clap::{builder::Styles, Args, Parser, Subcommand};

use crate::installable::Installable;
use crate::Result;

const fn make_style() -> Styles {
    Styles::plain().header(Style::new().bold()).literal(
        Style::new()
            .bold()
            .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
    )
}

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    styles=make_style(),
    propagate_version = false,
    help_template = "
{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
)]
/// Yet another nix helper
pub struct Main {
    #[arg(short, long, global = true)]
    /// Show debug logs
    pub verbose: bool,

    #[command(subcommand)]
    pub command: NHCommand,
}

#[derive(Subcommand, Debug)]
#[command(disable_help_subcommand = true)]
pub enum NHCommand {
    Os(OsArgs),
    Home(HomeArgs),
    Darwin(DarwinArgs),
    Search(SearchArgs),
    Clean(CleanProxy),
    #[command(hide = true)]
    Completions(CompletionArgs),
}

impl NHCommand {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Os(args) => {
                unsafe { std::env::set_var("NH_CURRENT_COMMAND", "os"); }
                args.run()
            }
            Self::Search(args) => args.run(),
            Self::Clean(proxy) => proxy.command.run(),
            Self::Completions(args) => args.run(),
            Self::Home(args) => {
                unsafe { std::env::set_var("NH_CURRENT_COMMAND", "home"); }
                args.run()
            }
            Self::Darwin(args) => {
                unsafe { std::env::set_var("NH_CURRENT_COMMAND", "darwin"); }
                args.run()
            }
        }
    }
}

#[derive(Args, Debug)]
#[clap(verbatim_doc_comment)]
/// `NixOS` functionality
///
/// Implements functionality mostly around but not exclusive to nixos-rebuild
pub struct OsArgs {
    #[command(subcommand)]
    pub subcommand: OsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum OsSubcommand {
    /// Build and activate the new configuration, and make it the boot default
    Switch(OsRebuildArgs),

    /// Build the new configuration and make it the boot default
    Boot(OsRebuildArgs),

    /// Build and activate the new configuration
    Test(OsRebuildArgs),

    /// Build the new configuration
    Build(OsRebuildArgs),

    /// Load system in a repl
    Repl(OsReplArgs),

    /// List available generations from profile path
    Info(OsGenerationsArgs),

    /// Rollback to a previous generation
    Rollback(OsRollbackArgs),

    /// Build a `NixOS` VM image
    BuildVm(OsBuildVmArgs),
}

#[derive(Debug, Args)]
pub struct OsBuildVmArgs {
    #[command(flatten)]
    pub common: OsRebuildArgs,

    /// Build with bootloader. Bootloader is bypassed by default.
    #[arg(long, short = 'B')]
    pub with_bootloader: bool,
}

#[derive(Debug, Args)]
pub struct OsRebuildArgs {
    #[command(flatten)]
    pub common: CommonRebuildArgs,

    #[command(flatten)]
    pub update_args: UpdateArgs,

    /// When using a flake installable, select this hostname from nixosConfigurations
    #[arg(long, short = 'H', global = true)]
    pub hostname: Option<String>,

    /// Explicitly select some specialisation
    #[arg(long, short)]
    pub specialisation: Option<String>,

    /// Ignore specialisations
    #[arg(long, short = 'S')]
    pub no_specialisation: bool,

    /// Extra arguments passed to nix build
    #[arg(last = true)]
    pub extra_args: Vec<String>,

    /// Don't panic if calling nh as root
    #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
    pub bypass_root_check: bool,

    /// Deploy the configuration to a different host over ssh
    #[arg(long)]
    pub target_host: Option<String>,

    /// Build the configuration to a different host over ssh
    #[arg(long)]
    pub build_host: Option<String>,
}

#[derive(Debug, Args)]
pub struct OsRollbackArgs {
    /// Only print actions, without performing them
    #[arg(long, short = 'n')]
    pub dry: bool,

    /// Ask for confirmation
    #[arg(long, short)]
    pub ask: bool,

    /// Explicitly select some specialisation
    #[arg(long, short)]
    pub specialisation: Option<String>,

    /// Ignore specialisations
    #[arg(long, short = 'S')]
    pub no_specialisation: bool,

    /// Rollback to a specific generation number (defaults to previous generation)
    #[arg(long, short)]
    pub to: Option<u64>,

    /// Don't panic if calling nh as root
    #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
    pub bypass_root_check: bool,
}

#[derive(Debug, Args)]
pub struct CommonRebuildArgs {
    /// Only print actions, without performing them
    #[arg(long, short = 'n')]
    pub dry: bool,

    /// Ask for confirmation
    #[arg(long, short)]
    pub ask: bool,

    #[command(flatten)]
    pub installable: Installable,

    /// Don't use nix-output-monitor for the build process
    #[arg(long)]
    pub no_nom: bool,

    /// Path to save the result link, defaults to using a temporary directory
    #[arg(long, short)]
    pub out_link: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct OsReplArgs {
    #[command(flatten)]
    pub installable: Installable,

    /// When using a flake installable, select this hostname from nixosConfigurations
    #[arg(long, short = 'H', global = true)]
    pub hostname: Option<String>,
}

#[derive(Debug, Args)]
pub struct OsGenerationsArgs {
    /// Path to Nix' profiles directory
    #[arg(long, short = 'P', default_value = "/nix/var/nix/profiles/system")]
    pub profile: Option<String>,
}

#[derive(Args, Debug)]
/// Searches packages by querying search.nixos.org
pub struct SearchArgs {
    #[arg(long, short, default_value = "30")]
    /// Number of search results to display
    pub limit: u64,

    #[arg(
        long,
        short,
        env = "NH_SEARCH_CHANNEL",
        default_value = "nixos-unstable"
    )]
    /// Name of the channel to query (e.g nixos-23.11, nixos-unstable, etc)
    pub channel: String,

    #[arg(long, short = 'P', env = "NH_SEARCH_PLATFORM", value_parser = clap::builder::BoolishValueParser::new())]
    /// Show supported platforms for each package
    pub platforms: bool,

    #[arg(long, short = 'j', env = "NH_SEARCH_JSON", value_parser = clap::builder::BoolishValueParser::new())]
    /// Output results as JSON
    pub json: bool,

    /// Name of the package to search
    pub query: Vec<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum SearchNixpkgsFrom {
    Flake,
    Path,
}

// Needed a struct to have multiple sub-subcommands
#[derive(Debug, Clone, Args)]
pub struct CleanProxy {
    #[clap(subcommand)]
    command: CleanMode,
}

#[derive(Debug, Clone, Subcommand)]
/// Enhanced nix cleanup
pub enum CleanMode {
    /// Clean all profiles
    All(CleanArgs),
    /// Clean the current user's profiles
    User(CleanArgs),
    /// Clean a specific profile
    Profile(CleanProfileArgs),
}

#[derive(Args, Clone, Debug)]
#[clap(verbatim_doc_comment)]
/// Enhanced nix cleanup
///
/// For --keep-since, see the documentation of humantime for possible formats: <https://docs.rs/humantime/latest/humantime/fn.parse_duration.html>
pub struct CleanArgs {
    #[arg(long, short, default_value = "1")]
    /// At least keep this number of generations
    pub keep: u32,

    #[arg(long, short = 'K', default_value = "0h")]
    /// At least keep gcroots and generations in this time range since now.
    pub keep_since: humantime::Duration,

    /// Only print actions, without performing them
    #[arg(long, short = 'n')]
    pub dry: bool,

    /// Ask for confirmation
    #[arg(long, short)]
    pub ask: bool,

    /// Don't run nix store --gc
    #[arg(long)]
    pub nogc: bool,

    /// Don't clean gcroots
    #[arg(long)]
    pub nogcroots: bool,
}

#[derive(Debug, Clone, Args)]
pub struct CleanProfileArgs {
    #[command(flatten)]
    pub common: CleanArgs,

    /// Which profile to clean
    pub profile: PathBuf,
}

#[derive(Debug, Args)]
/// Home-manager functionality
pub struct HomeArgs {
    #[command(subcommand)]
    pub subcommand: HomeSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum HomeSubcommand {
    /// Build and activate a home-manager configuration
    Switch(HomeRebuildArgs),

    /// Build a home-manager configuration
    Build(HomeRebuildArgs),

    /// Load a home-manager configuration in a Nix REPL
    Repl(HomeReplArgs),
}

#[derive(Debug, Args)]
pub struct HomeRebuildArgs {
    #[command(flatten)]
    pub common: CommonRebuildArgs,

    #[command(flatten)]
    pub update_args: UpdateArgs,

    /// Name of the flake homeConfigurations attribute, like username@hostname
    ///
    /// If unspecified, will try <username>@<hostname> and <username>
    #[arg(long, short)]
    pub configuration: Option<String>,

    /// Explicitly select some specialisation
    #[arg(long, short)]
    pub specialisation: Option<String>,

    /// Ignore specialisations
    #[arg(long, short = 'S')]
    pub no_specialisation: bool,

    /// Extra arguments passed to nix build
    #[arg(last = true)]
    pub extra_args: Vec<String>,

    /// Move existing files by backing up with this file extension
    #[arg(long, short = 'b')]
    pub backup_extension: Option<String>,
}

#[derive(Debug, Args)]
pub struct HomeReplArgs {
    #[command(flatten)]
    pub installable: Installable,

    /// Name of the flake homeConfigurations attribute, like username@hostname
    ///
    /// If unspecified, will try <username>@<hostname> and <username>
    #[arg(long, short)]
    pub configuration: Option<String>,

    /// Extra arguments passed to nix repl
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Parser)]
/// Generate shell completion files into stdout
pub struct CompletionArgs {
    /// Name of the shell
    pub shell: clap_complete::Shell,
}

/// Nix-darwin functionality
///
/// Implements functionality mostly around but not exclusive to darwin-rebuild
#[derive(Debug, Args)]
pub struct DarwinArgs {
    #[command(subcommand)]
    pub subcommand: DarwinSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DarwinSubcommand {
    /// Build and activate a nix-darwin configuration
    Switch(DarwinRebuildArgs),
    /// Build a nix-darwin configuration
    Build(DarwinRebuildArgs),
    /// Load a nix-darwin configuration in a Nix REPL
    Repl(DarwinReplArgs),
}

#[derive(Debug, Args)]
pub struct DarwinRebuildArgs {
    #[command(flatten)]
    pub common: CommonRebuildArgs,

    #[command(flatten)]
    pub update_args: UpdateArgs,

    /// When using a flake installable, select this hostname from darwinConfigurations
    #[arg(long, short = 'H', global = true)]
    pub hostname: Option<String>,

    /// Extra arguments passed to nix build
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Args)]
pub struct DarwinReplArgs {
    #[command(flatten)]
    pub installable: Installable,

    /// When using a flake installable, select this hostname from darwinConfigurations
    #[arg(long, short = 'H', global = true)]
    pub hostname: Option<String>,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[arg(short = 'u', long = "update")]
    /// Update all flake inputs
    pub update: bool,

    #[arg(short = 'U', long = "update-input")]
    /// Update a single flake input
    pub update_input: Option<String>,
}
