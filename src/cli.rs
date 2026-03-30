use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::model::DesktopEnvironment;

#[derive(Debug, Parser)]
#[command(
    name = "pane",
    version,
    about = "Prepare and launch a Linux desktop session from WSL2.",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create or adopt a Pane-managed Arch environment.
    Init(InitArgs),
    /// Initialize Pane Arch, configure the login user, and verify launch readiness.
    Onboard(OnboardArgs),
    /// Generate or execute the Arch-first MVP launch path.
    Launch(LaunchArgs),
    /// Reapply Pane-managed Arch integration without opening mstsc.exe.
    Repair(RepairArgs),
    /// Refresh Arch packages and reapply the Pane-managed integration layer.
    Update(UpdateArgs),
    /// Inspect WSL, the selected distro, managed environment, and the last generated Pane assets.
    Status(StatusArgs),
    /// Show Pane's managed Linux environment catalog and support tiers.
    Environments(EnvironmentsArgs),
    /// Run support-focused diagnostics before launch or reconnect.
    Doctor(DoctorArgs),
    /// Reopen mstsc.exe for the last generated Pane session.
    Connect(ConnectArgs),
    #[command(hide = true)]
    Relay(RelayArgs),
    /// Open or print the Pane-managed shared directory for a session.
    Share(ShareArgs),
    /// Create or repair the default Arch user and WSL config for Pane.
    SetupUser(SetupUserArgs),
    /// Open an interactive terminal inside the managed Arch environment.
    Terminal(TerminalArgs),
    /// Stop the XRDP services inside the selected distro.
    Stop(StopArgs),
    /// Remove Pane-managed local state and optionally purge WSL session wiring.
    Reset(ResetArgs),
    /// Print the last bootstrap transcript plus live XRDP logs when available.
    Logs(LogsArgs),
    /// Create a zipped support bundle with reports, state, and workspace artifacts.
    Bundle(BundleArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// WSL distro name for the Pane-managed Arch environment.
    #[arg(long, default_value = "pane-arch")]
    pub distro_name: String,
    /// Adopt an existing Arch distro into Pane management instead of importing a fresh rootfs.
    #[arg(long)]
    pub existing_distro: Option<String>,
    /// Path to an Arch Linux rootfs tarball to import into WSL as a fresh Pane-managed distro.
    #[arg(long)]
    pub rootfs_tar: Option<PathBuf>,
    /// Optional installation directory used with --rootfs-tar. Defaults to %LOCALAPPDATA%\Pane\distros\<distro-name>.
    #[arg(long)]
    pub install_dir: Option<PathBuf>,
    /// Print the managed-environment plan without changing WSL or Pane state.
    #[arg(long)]
    pub dry_run: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct OnboardArgs {
    /// WSL distro name for the Pane-managed Arch environment.
    #[arg(long, default_value = "pane-arch")]
    pub distro_name: String,
    /// Adopt an existing Arch distro into Pane management instead of installing a fresh Pane-managed distro.
    #[arg(long)]
    pub existing_distro: Option<String>,
    /// Path to an Arch Linux rootfs tarball to import into WSL as a fresh Pane-managed distro.
    #[arg(long)]
    pub rootfs_tar: Option<PathBuf>,
    /// Optional installation directory used with --rootfs-tar. Defaults to %LOCALAPPDATA%\Pane\distros\<distro-name>.
    #[arg(long)]
    pub install_dir: Option<PathBuf>,
    /// Linux username to create or repair during onboarding.
    #[arg(long)]
    pub username: String,
    /// Linux password to set for the user. Prefer --password-stdin so the password does not appear in the process list.
    #[arg(long)]
    pub password: Option<String>,
    /// Read the Linux password from stdin instead of the command line.
    #[arg(long)]
    pub password_stdin: bool,
    /// Desktop environment to validate after onboarding. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the readiness check workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port to validate for the post-onboarding readiness check.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Print the onboarding plan without changing WSL or Pane state.
    #[arg(long)]
    pub dry_run: bool,
    /// Leave WSL running after writing /etc/wsl.conf instead of restarting it immediately.
    #[arg(long)]
    pub no_shutdown: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LaunchArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro when one exists, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to provision in the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the bootstrap and RDP assets without executing WSL or mstsc.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the WSL bootstrap execution after writing the assets.
    #[arg(long)]
    pub skip_bootstrap: bool,
    /// Do not open mstsc.exe after bootstrap succeeds.
    #[arg(long)]
    pub no_connect: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct RepairArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro when one exists, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to repair inside the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the repaired bootstrap and RDP assets without executing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro when one exists, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to update inside the distro. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port written into the generated xrdp.ini patch and .rdp profile.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Write the updated bootstrap and RDP assets without executing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Print the generated bootstrap script to stdout after writing it.
    #[arg(long)]
    pub print_script: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct EnvironmentsArgs {
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Desktop environment to validate. MVP support is Arch + XFCE only.
    #[arg(long, value_enum, default_value_t = DesktopEnvironment::Xfce)]
    pub de: DesktopEnvironment,
    /// Session slug used for the generated workspace on Windows.
    #[arg(long, default_value = "pane")]
    pub session_name: String,
    /// XRDP port to validate.
    #[arg(long, default_value_t = 3390)]
    pub port: u16,
    /// Validate an already-bootstrapped environment instead of a fresh bootstrap path.
    #[arg(long)]
    pub skip_bootstrap: bool,
    /// Skip mstsc.exe validation when you only want bootstrap readiness.
    #[arg(long)]
    pub no_connect: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ConnectArgs {
    /// Session slug to reconnect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// Open mstsc.exe even when readiness checks report a blocker.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RelayArgs {
    #[arg(long)]
    pub distro: String,
    #[arg(long)]
    pub listen_port: u16,
    #[arg(long)]
    pub target_port: u16,
    #[arg(long, default_value_t = 90)]
    pub startup_timeout_seconds: u64,
    #[arg(long)]
    pub log_file: Option<PathBuf>,
    #[arg(long)]
    pub ready_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ShareArgs {
    /// Session slug to inspect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// Print the resolved paths without opening Explorer.
    #[arg(long)]
    pub print_only: bool,
}

#[derive(Debug, Args)]
pub struct SetupUserArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Linux username to create or repair.
    #[arg(long)]
    pub username: String,
    /// Linux password to set for the user. Prefer --password-stdin so the password does not appear in the process list.
    #[arg(long)]
    pub password: Option<String>,
    /// Read the Linux password from stdin instead of the command line.
    #[arg(long)]
    pub password_stdin: bool,
    /// Print the onboarding plan without changing WSL.
    #[arg(long)]
    pub dry_run: bool,
    /// Leave WSL running after writing /etc/wsl.conf instead of restarting it immediately.
    #[arg(long)]
    pub no_shutdown: bool,
    /// Emit structured JSON instead of a human-readable summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TerminalArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Optional Linux user for the interactive shell.
    #[arg(long)]
    pub user: Option<String>,
    /// Print the resolved terminal target without opening it.
    #[arg(long)]
    pub print_only: bool,
}

#[derive(Debug, Args)]
pub struct StopArgs {
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
}

#[derive(Debug, Args)]
pub struct ResetArgs {
    /// Session slug to reset. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name used when purging Pane-managed session wiring.
    #[arg(long)]
    pub distro: Option<String>,
    /// Also remove Pane-managed .xsession content and stop XRDP inside WSL.
    #[arg(long)]
    pub purge_wsl: bool,
    /// Remove Pane's managed-environment ownership record without deleting the distro.
    #[arg(long, conflicts_with = "factory_reset")]
    pub release_managed_environment: bool,
    /// Destroy a Pane-imported managed distro, delete its install root, and clear Pane ownership.
    #[arg(long, conflicts_with = "release_managed_environment")]
    pub factory_reset: bool,
    /// Print the reset plan without changing WSL, local workspaces, or Pane state.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Session slug to inspect. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Number of XRDP log lines to fetch from WSL.
    #[arg(long, default_value_t = 50)]
    pub lines: usize,
}

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// Session slug to include. Defaults to the last launched session.
    #[arg(long)]
    pub session_name: Option<String>,
    /// WSL distro name. Defaults to the Pane-managed Arch distro, then the last launched distro, then a supported Arch distro.
    #[arg(long)]
    pub distro: Option<String>,
    /// Optional zip path for the generated support bundle.
    #[arg(long)]
    pub output: Option<PathBuf>,
}
