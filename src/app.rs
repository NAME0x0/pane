#![allow(clippy::uninlined_format_args)]

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use clap::Parser;
use serde::Serialize;

use crate::{
    bootstrap::{render_bootstrap_script, render_update_script},
    cli::{
        BundleArgs, Cli, Commands, ConnectArgs, DoctorArgs, EnvironmentsArgs, InitArgs, LaunchArgs,
        LogsArgs, OnboardArgs, RelayArgs, RepairArgs, ResetArgs, SetupUserArgs, ShareArgs,
        StatusArgs, StopArgs, TerminalArgs, UpdateArgs,
    },
    error::{AppError, AppResult},
    model::{
        managed_environment_catalog, DesktopEnvironment, DistroFamily, DistroRecord,
        ManagedEnvironment,
    },
    plan::{
        app_root, managed_distro_install_root, shared_dir_for_workspace, windows_to_wsl_path,
        workspace_for, LaunchPlan, WorkspacePaths,
    },
    rdp::render_rdp_profile,
    state::{
        clear_managed_environment, clear_state, load_state, save_managed_environment,
        save_state_record, LaunchStage, LaunchTransport, ManagedEnvironmentOwnership,
        ManagedEnvironmentState, PaneState, StoredLaunch,
    },
    wsl::{
        self, probe_inventory, run_wsl_shell_as_user_capture,
        run_wsl_shell_as_user_capture_with_input, shell_quote, PasswordStatus, WslInventory,
    },
};

#[derive(Debug)]
struct LaunchTarget {
    distro: DistroRecord,
    hypothetical: bool,
}

#[derive(Debug)]
enum InitSource {
    AdoptExisting {
        distro_name: String,
    },
    InstallOnline {
        distro_name: String,
        install_dir: PathBuf,
    },
    ImportRootfs {
        distro_name: String,
        rootfs_tar: PathBuf,
        install_dir: PathBuf,
    },
}

#[derive(Debug, Serialize)]
struct InitReport {
    product_shape: &'static str,
    managed_environment: ManagedEnvironmentState,
    dry_run: bool,
    present_in_inventory: bool,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SetupUserReport {
    product_shape: &'static str,
    distro: String,
    username: String,
    dry_run: bool,
    password_updated: bool,
    default_user_configured: bool,
    systemd_configured: bool,
    wsl_shutdown: bool,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct OnboardReport {
    product_shape: &'static str,
    managed_environment: ManagedEnvironmentState,
    setup_user: SetupUserReport,
    launch_readiness: Option<DoctorReport>,
    dry_run: bool,
    ready_for_launch: bool,
    notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct DoctorRequest {
    distro: Option<String>,
    session_name: String,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_requested: bool,
    connect_requested: bool,
}

#[derive(Debug, Serialize)]
struct StatusReport {
    platform: &'static str,
    wsl_available: bool,
    wsl_version_banner: Option<String>,
    managed_environment: Option<ManagedEnvironmentState>,
    selected_distro: Option<DistroHealth>,
    known_distros: Vec<DistroRecord>,
    last_launch: Option<StoredLaunch>,
    last_launch_workspace: Option<WorkspaceHealth>,
}

#[derive(Debug, Serialize)]
struct EnvironmentCatalogReport {
    product_shape: &'static str,
    strategy: &'static str,
    environments: Vec<ManagedEnvironment>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DistroHealth {
    distro: DistroRecord,
    supported_for_mvp: bool,
    present_in_inventory: bool,
    checked_port: u16,
    systemd_configured: Option<bool>,
    xrdp_installed: Option<bool>,
    xrdp_service_active: Option<bool>,
    xrdp_listening: Option<bool>,
    localhost_reachable: Option<bool>,
    pane_relay_available: Option<bool>,
    preferred_transport: Option<LaunchTransport>,
    xsession_present: Option<bool>,
    pane_session_assets_ready: Option<bool>,
    user_home_ready: Option<bool>,
    default_user_password_status: Option<PasswordStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PreparedTransport {
    kind: LaunchTransport,
    host: String,
}

impl PreparedTransport {
    fn direct_localhost() -> Self {
        Self {
            kind: LaunchTransport::DirectLocalhost,
            host: "localhost".to_string(),
        }
    }

    fn direct_wsl_ip(host: String) -> Self {
        Self {
            kind: LaunchTransport::DirectWslIp,
            host,
        }
    }

    fn pane_relay() -> Self {
        Self {
            kind: LaunchTransport::PaneRelay,
            host: "localhost".to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct WorkspaceHealth {
    root_exists: bool,
    shared_dir_exists: bool,
    bootstrap_script_exists: bool,
    rdp_profile_exists: bool,
    bootstrap_log_exists: bool,
    transport_log_exists: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CheckStatus {
    Pass,
    Fail,
}

impl CheckStatus {
    fn display_name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    id: String,
    status: CheckStatus,
    summary: String,
    remediation: Option<String>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    target_distro: Option<String>,
    session_name: String,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_requested: bool,
    connect_requested: bool,
    supported_for_mvp: bool,
    ready: bool,
    selected_distro: Option<DistroHealth>,
    workspace: WorkspaceHealth,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == CheckStatus::Fail)
    }
}

#[derive(Debug, Serialize)]
struct BundleManifest {
    created_at_epoch_seconds: u64,
    session_name: String,
    selected_distro: Option<String>,
    output_zip: String,
    included_files: Vec<String>,
    notes: Vec<String>,
}

pub fn run() -> AppResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => init(args),
        Commands::Onboard(args) => onboard(args),
        Commands::Launch(args) => launch(args),
        Commands::Repair(args) => repair(args),
        Commands::Update(args) => update(args),
        Commands::Status(args) => status(args),
        Commands::Environments(args) => environments(args),
        Commands::Doctor(args) => doctor(args),
        Commands::Connect(args) => connect(args),
        Commands::Relay(args) => relay(args),
        Commands::Share(args) => share(args),
        Commands::SetupUser(args) => setup_user(args),
        Commands::Terminal(args) => terminal(args),
        Commands::Stop(args) => stop(args),
        Commands::Reset(args) => reset(args),
        Commands::Logs(args) => logs(args),
        Commands::Bundle(args) => bundle(args),
    }
}

fn init(args: InitArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = initialize_managed_arch_environment(&args, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_init_report(&report);
    Ok(())
}

fn onboard(args: OnboardArgs) -> AppResult<()> {
    let init_report = initialize_managed_arch_environment(
        &InitArgs {
            distro_name: args.distro_name.clone(),
            existing_distro: args.existing_distro.clone(),
            rootfs_tar: args.rootfs_tar.clone(),
            install_dir: args.install_dir.clone(),
            dry_run: args.dry_run,
            json: false,
        },
        &probe_inventory(),
        load_state()?.as_ref(),
    )?;

    let setup_report = if args.dry_run && !init_report.present_in_inventory {
        build_planned_setup_user_report(
            &SetupUserArgs {
                distro: Some(init_report.managed_environment.distro_name.clone()),
                username: args.username.clone(),
                password: args.password.clone(),
                password_stdin: args.password_stdin,
                dry_run: true,
                no_shutdown: args.no_shutdown,
                json: false,
            },
            &init_report.managed_environment.distro_name,
        )?
    } else {
        configure_arch_user(
            &SetupUserArgs {
                distro: Some(init_report.managed_environment.distro_name.clone()),
                username: args.username.clone(),
                password: args.password.clone(),
                password_stdin: args.password_stdin,
                dry_run: args.dry_run,
                no_shutdown: args.no_shutdown,
                json: false,
            },
            &probe_inventory(),
            load_state()?.as_ref(),
        )?
    };

    let mut notes = init_report.notes.clone();
    notes.extend(setup_report.notes.iter().cloned());

    let (launch_readiness, ready_for_launch) = if args.dry_run {
        notes.push(
            "Dry run did not execute the final readiness check. Run `pane onboard` without --dry-run for a real launch-readiness result."
                .to_string(),
        );
        (None, false)
    } else {
        let post_setup_inventory = probe_inventory();
        let post_setup_state = load_state()?;
        let readiness = evaluate_doctor(
            &DoctorRequest {
                distro: Some(setup_report.distro.clone()),
                session_name: crate::plan::sanitize_session_name(&args.session_name),
                desktop_environment: args.de,
                port: args.port,
                bootstrap_requested: true,
                connect_requested: true,
            },
            &post_setup_inventory,
            post_setup_state.as_ref(),
        )?;
        let ready = readiness.ready && readiness.supported_for_mvp;
        notes.push(if ready {
            "Pane verified that Arch is ready for the supported launch path. Use `pane launch` or the Launch Arch button next."
                .to_string()
        } else {
            "Pane completed onboarding, but launch readiness still has blockers. Review the embedded doctor report before launching."
                .to_string()
        });
        (Some(readiness), ready)
    };

    let report = OnboardReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        managed_environment: init_report.managed_environment,
        setup_user: setup_report,
        launch_readiness,
        dry_run: args.dry_run,
        ready_for_launch,
        notes,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_onboard_report(&report);
    }

    Ok(())
}

fn build_planned_setup_user_report(
    args: &SetupUserArgs,
    distro: &str,
) -> AppResult<SetupUserReport> {
    validate_setup_username(&args.username)?;
    let password_updated = match &args.password {
        Some(password) => {
            validate_setup_password(password)?;
            true
        }
        None => args.password_stdin,
    };

    let mut notes = vec![format!(
        "Pane would configure '{}' as the default WSL user for {} after the managed Arch distro is provisioned.",
        args.username, distro
    )];
    notes.push(
        "Pane would also ensure /etc/wsl.conf advertises systemd=true so the Arch desktop path can start cleanly."
            .to_string(),
    );
    if args.password_stdin {
        notes.push(
            "Dry run mode did not read the password from stdin, but the live onboarding flow would apply it during user setup."
                .to_string(),
        );
    }
    if !args.no_shutdown {
        notes.push(
            "WSL would be shut down after setup so the new default user and systemd settings take effect immediately."
                .to_string(),
        );
    }

    Ok(SetupUserReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        distro: distro.to_string(),
        username: args.username.clone(),
        dry_run: true,
        password_updated,
        default_user_configured: true,
        systemd_configured: true,
        wsl_shutdown: false,
        notes,
    })
}

fn setup_user(args: SetupUserArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = configure_arch_user(&args, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_setup_user_report(&report);
    }

    Ok(())
}

fn configure_arch_user(
    args: &SetupUserArgs,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<SetupUserReport> {
    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only configure a user inside a live WSL installation.",
        ));
    }

    let distro = resolve_operational_distro(args.distro.as_deref(), inventory, saved_state)?;
    if !inventory_contains_distro(inventory, &distro) {
        return Err(AppError::message(format!(
            "Resolved distro '{}' is not currently installed in WSL. Run `pane init` first or pass --distro <arch-distro-name>.",
            distro
        )));
    }

    validate_setup_username(&args.username)?;
    let password = resolve_setup_user_password(args)?;
    let distro_record = wsl::inspect_distro(&distro, inventory)?;
    if !distro_record.is_mvp_supported() {
        return Err(AppError::message(format!(
            "Pane MVP currently supports user setup only for Arch Linux + XFCE paths. Resolved distro: {}.",
            distro_record.label()
        )));
    }

    let current_wsl_conf = wsl::run_wsl_shell_as_user(
        &distro,
        Some("root"),
        "cat /etc/wsl.conf 2>/dev/null || true",
    )
    .unwrap_or_default();
    let updated_wsl_conf = ensure_wsl_conf_setting(
        &ensure_wsl_conf_setting(&current_wsl_conf, "boot", "systemd", "true"),
        "user",
        "default",
        &args.username,
    );

    let mut notes = vec![format!(
        "Pane will configure '{}' as the default WSL user for {}.",
        args.username, distro
    )];
    notes.push(
        "Pane also ensures /etc/wsl.conf advertises systemd=true so the Arch desktop path can start cleanly."
            .to_string(),
    );
    if args.dry_run {
        if !args.no_shutdown {
            notes.push(
                "WSL would be shut down after setup so the new default user and systemd settings take effect immediately."
                    .to_string(),
            );
        }
        return Ok(SetupUserReport {
            product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
            distro,
            username: args.username.clone(),
            dry_run: true,
            password_updated: password.is_some(),
            default_user_configured: true,
            systemd_configured: true,
            wsl_shutdown: false,
            notes,
        });
    }

    let setup_command = build_setup_user_shell_command(&args.username);
    let password = password.expect("validated password for non-dry-run setup");
    let credentials = format!("{}:{}\n", args.username, password);
    let setup_transcript = run_wsl_shell_as_user_capture_with_input(
        &distro,
        Some("root"),
        &setup_command,
        &credentials,
    )?;
    if !setup_transcript.success {
        return Err(AppError::message(format!(
            "Pane could not configure user '{}' inside {}: {}",
            args.username,
            distro,
            setup_transcript.combined_output().trim()
        )));
    }

    let write_conf_command =
        format!("cat > /etc/wsl.conf <<'__PANE_WSL_CONF__'\n{updated_wsl_conf}__PANE_WSL_CONF__");
    let write_conf = run_wsl_shell_as_user_capture(&distro, Some("root"), &write_conf_command)?;
    if !write_conf.success {
        return Err(AppError::message(format!(
            "Pane could not update /etc/wsl.conf inside {}: {}",
            distro,
            write_conf.combined_output().trim()
        )));
    }

    let wsl_shutdown = if args.no_shutdown {
        notes.push(
            "WSL was left running. Restart WSL manually before relying on the new default user or systemd state."
                .to_string(),
        );
        false
    } else {
        let shutdown = wsl::shutdown_wsl()?;
        if !shutdown.success {
            return Err(AppError::message(format!(
                "Pane configured '{}' but could not restart WSL: {}",
                args.username,
                shutdown.combined_output().trim()
            )));
        }
        notes.push(
            "WSL was shut down so the new default user and systemd settings will apply on the next launch."
                .to_string(),
        );
        true
    };

    Ok(SetupUserReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        distro,
        username: args.username.clone(),
        dry_run: false,
        password_updated: true,
        default_user_configured: true,
        systemd_configured: true,
        wsl_shutdown,
        notes,
    })
}

fn launch(args: LaunchArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for(&session_name);

    let plan = LaunchPlan {
        steps: build_steps(
            &target.distro,
            args.de,
            args.port,
            !args.skip_bootstrap,
            !args.no_connect,
        ),
        bootstrap_script: render_bootstrap_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: !args.no_connect,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch = StoredLaunch::planned_from_plan(
        &plan,
        args.dry_run,
        target.hypothetical,
        !args.skip_bootstrap,
        !args.no_connect,
    );
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: !args.skip_bootstrap,
        connect_requested: !args.no_connect,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_launch_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane launch", &doctor_report)),
        ));
    }

    if !args.skip_bootstrap {
        if let Err(error) = execute_bootstrap(&plan) {
            return Err(fail_launch(&mut stored_launch, error));
        }

        stored_launch.mark_bootstrapped();
        save_state_record(stored_launch.clone())?;
        println!(
            "Bootstrap completed inside {}. Transcript: {}",
            plan.distro.name,
            plan.workspace.bootstrap_log.display()
        );

        if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
            return Err(fail_launch(
                &mut stored_launch,
                AppError::message(format!(
                    "XRDP did not become ready inside WSL on port {} after bootstrap. Review {} or run `pane logs`.",
                    plan.port,
                    plan.workspace.bootstrap_log.display()
                )),
            ));
        }
    } else {
        println!("Skipped the WSL bootstrap step.");
    }

    if !args.no_connect {
        let transport = ensure_transport_ready(&plan.distro.name, plan.port, &plan.workspace)
            .map_err(|error| fail_launch(&mut stored_launch, error))?;
        if let Err(error) = write_runtime_rdp_profile(
            &plan.workspace.rdp_profile,
            &plan.distro,
            &transport.host,
            plan.port,
        ) {
            return Err(fail_launch(&mut stored_launch, error));
        }
        if let Err(error) = open_rdp_profile(&plan.workspace.rdp_profile) {
            return Err(fail_launch(&mut stored_launch, error));
        }

        stored_launch.mark_rdp_launched(transport.kind);
        save_state_record(stored_launch.clone())?;
        println!(
            "Opened mstsc.exe with {} over {} targeting {}:{}.",
            plan.workspace.rdp_profile.display(),
            transport.kind.display_name(),
            transport.host,
            plan.port,
        );
    } else {
        println!(
            "RDP profile written to {}. Open it manually when ready.",
            plan.workspace.rdp_profile.display()
        );
    }

    Ok(())
}

fn repair(args: RepairArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for(&session_name);

    let plan = LaunchPlan {
        steps: build_steps(&target.distro, args.de, args.port, true, false),
        bootstrap_script: render_bootstrap_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: false,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch =
        StoredLaunch::planned_from_plan(&plan, args.dry_run, target.hypothetical, true, false);
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: true,
        connect_requested: false,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_repair_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane repair", &doctor_report)),
        ));
    }

    if let Err(error) = execute_bootstrap(&plan) {
        return Err(fail_launch(&mut stored_launch, error));
    }

    stored_launch.mark_bootstrapped();
    save_state_record(stored_launch.clone())?;
    println!(
        "Repair completed inside {}. Transcript: {}",
        plan.distro.name,
        plan.workspace.bootstrap_log.display()
    );

    if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format!(
                "XRDP did not become ready inside WSL on port {} after repair. Review {} or run `pane logs`.",
                plan.port,
                plan.workspace.bootstrap_log.display()
            )),
        ));
    }

    println!(
        "Pane repair finished. Reconnect with `pane connect --session-name {}` or use the Control Center.",
        plan.session_name
    );

    Ok(())
}

fn update(args: UpdateArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let target = resolve_launch_target(
        args.distro.as_deref(),
        &inventory,
        saved_state.as_ref(),
        args.dry_run,
    )?;
    let session_name = crate::plan::sanitize_session_name(&args.session_name);
    let workspace = workspace_for(&session_name);

    let plan = LaunchPlan {
        steps: build_update_steps(&target.distro, args.de, args.port),
        bootstrap_script: render_update_script(
            &target.distro,
            args.de,
            args.port,
            &windows_to_wsl_path(&shared_dir_for_workspace(&workspace)),
        ),
        rdp_profile: render_rdp_profile(&target.distro, "localhost", args.port),
        session_name,
        distro: target.distro.clone(),
        desktop_environment: args.de,
        port: args.port,
        connect_after_bootstrap: false,
        workspace,
    };

    crate::plan::write_workspace(&plan)?;

    let mut stored_launch =
        StoredLaunch::planned_from_plan(&plan, args.dry_run, target.hypothetical, true, false);
    save_state_record(stored_launch.clone())?;

    let doctor_request = DoctorRequest {
        distro: Some(plan.distro.name.clone()),
        session_name: plan.session_name.clone(),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: true,
        connect_requested: false,
    };
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;

    print_update_summary(&plan, &stored_launch);

    if args.print_script {
        println!();
        println!("{}", plan.bootstrap_script);
    }

    if args.dry_run {
        return Ok(());
    }

    if doctor_report.has_failures() {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format_doctor_blockers("pane update", &doctor_report)),
        ));
    }

    if let Err(error) = execute_bootstrap(&plan) {
        return Err(fail_launch(&mut stored_launch, error));
    }

    stored_launch.mark_bootstrapped();
    save_state_record(stored_launch.clone())?;
    println!(
        "Update completed inside {}. Transcript: {}",
        plan.distro.name,
        plan.workspace.bootstrap_log.display()
    );

    if !wait_for_runtime_ready(&plan.distro.name, plan.port) {
        return Err(fail_launch(
            &mut stored_launch,
            AppError::message(format!(
                "XRDP did not become ready inside WSL on port {} after update. Review {} or run `pane logs`.",
                plan.port,
                plan.workspace.bootstrap_log.display()
            )),
        ));
    }

    println!(
        "Pane update finished. Reconnect with `pane connect --session-name {}` or use the Control Center.",
        plan.session_name
    );

    Ok(())
}

fn status(args: StatusArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let report = build_status_report(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_status_report(&report);
    Ok(())
}

fn environments(args: EnvironmentsArgs) -> AppResult<()> {
    let report = build_environment_catalog_report();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_environment_catalog_report(&report);
    Ok(())
}

fn doctor(args: DoctorArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let request = DoctorRequest {
        distro: args.distro,
        session_name: crate::plan::sanitize_session_name(&args.session_name),
        desktop_environment: args.de,
        port: args.port,
        bootstrap_requested: !args.skip_bootstrap,
        connect_requested: !args.no_connect,
    };
    let report = evaluate_doctor(&request, &inventory, saved_state.as_ref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_doctor_report(&report);
    Ok(())
}

fn connect(args: ConnectArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let mut launch = resolve_saved_launch(args.session_name.as_deref(), saved_state.as_ref())?;
    let report = evaluate_doctor(
        &DoctorRequest {
            distro: Some(launch.distro.name.clone()),
            session_name: launch.session_name.clone(),
            desktop_environment: launch.desktop_environment,
            port: launch.port,
            bootstrap_requested: false,
            connect_requested: true,
        },
        &inventory,
        saved_state.as_ref(),
    )?;

    if report.has_failures() && !args.force {
        return Err(AppError::message(format_doctor_blockers(
            "pane connect",
            &report,
        )));
    }

    if !launch.workspace.rdp_profile.exists() {
        return Err(AppError::message(format!(
            "The saved RDP profile was not found at {}. Run `pane launch` again.",
            launch.workspace.rdp_profile.display()
        )));
    }

    let transport = ensure_transport_ready(&launch.distro.name, launch.port, &launch.workspace)?;
    write_runtime_rdp_profile(
        &launch.workspace.rdp_profile,
        &launch.distro,
        &transport.host,
        launch.port,
    )?;
    open_rdp_profile(&launch.workspace.rdp_profile)?;
    launch.mark_rdp_launched(transport.kind);
    save_state_record(launch.clone())?;
    println!(
        "Opened mstsc.exe with the saved Pane profile over {} targeting {}:{}.",
        transport.kind.display_name(),
        transport.host,
        launch.port,
    );
    Ok(())
}

fn relay(args: RelayArgs) -> AppResult<()> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], args.listen_port)))
        .map_err(|error| {
            AppError::message(format!(
                "failed to bind the Pane relay to 127.0.0.1:{}: {error}",
                args.listen_port
            ))
        })?;
    listener.set_nonblocking(true).map_err(|error| {
        AppError::message(format!(
            "failed to configure the Pane relay listener on 127.0.0.1:{}: {error}",
            args.listen_port
        ))
    })?;

    if let Some(ready_file) = args.ready_file.as_deref() {
        if let Some(parent) = ready_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(ready_file, "ready\n").map_err(|error| {
            AppError::message(format!(
                "failed to publish the Pane relay readiness file at {}: {error}",
                ready_file.display()
            ))
        })?;
    }

    log_transport_event(
        args.log_file.as_deref(),
        &format!(
            "relay listening on 127.0.0.1:{} for {}:{}",
            args.listen_port, args.distro, args.target_port
        ),
    );

    let deadline = Instant::now() + Duration::from_secs(args.startup_timeout_seconds.max(1));
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                log_transport_event(
                    args.log_file.as_deref(),
                    &format!(
                        "relay accepted {} for {}:{}",
                        peer, args.distro, args.target_port
                    ),
                );
                relay_connection(
                    &args.distro,
                    args.target_port,
                    stream,
                    args.log_file.as_deref(),
                )?;
                log_transport_event(
                    args.log_file.as_deref(),
                    &format!(
                        "relay session finished for {}:{}",
                        args.distro, args.target_port
                    ),
                );
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    log_transport_event(
                        args.log_file.as_deref(),
                        &format!(
                            "relay timed out waiting for a client on 127.0.0.1:{}",
                            args.listen_port
                        ),
                    );
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(AppError::message(format!(
                    "the Pane relay failed while waiting on 127.0.0.1:{}: {error}",
                    args.listen_port
                )));
            }
        }
    }
}

fn share(args: ShareArgs) -> AppResult<()> {
    let saved_state = load_state()?;
    let (session_name, _saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    let shared_directory = shared_dir_for_workspace(&workspace);
    fs::create_dir_all(&shared_directory)?;
    let shared_wsl_path = windows_to_wsl_path(&shared_directory);

    if !args.print_only {
        open_directory_in_explorer(&shared_directory)?;
    }

    println!("Pane Shared Directory");
    println!("  Session        {}", session_name);
    println!("  Windows Path   {}", shared_directory.display());
    println!("  WSL Path       {}", shared_wsl_path);
    println!("  Linux Link     ~/PaneShared");
    if !args.print_only {
        println!("  Explorer       opened");
    }

    Ok(())
}

fn terminal(args: TerminalArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;

    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only open a terminal inside a live WSL installation.",
        ));
    }

    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if !inventory_contains_distro(&inventory, &distro) {
        return Err(AppError::message(format!(
            "Resolved distro '{}' is not currently installed in WSL. Run `pane init` first or pass --distro <arch-distro-name>.",
            distro
        )));
    }

    let default_user = wsl::inspect_distro(&distro, &inventory)
        .ok()
        .and_then(|record| record.default_user);
    let selected_user = args.user.clone().or(default_user);

    println!("Pane Arch Terminal");
    println!("  Distro         {}", distro);
    println!(
        "  User           {}",
        selected_user.as_deref().unwrap_or("default")
    );
    println!(
        "  Managed Flow   Use this shell for first-run setup, package installs, dotfiles, and desktop customization."
    );

    if args.print_only {
        return Ok(());
    }

    wsl::open_interactive_terminal(&distro, args.user.as_deref())
}

fn stop(args: StopArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())?;

    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Pane can only stop XRDP inside a live WSL installation.",
        ));
    }

    let output = wsl::stop_xrdp_services(&distro)?;
    println!("Stopped XRDP services inside {distro}.");
    if !output.trim().is_empty() {
        println!("{}", output.trim());
    }
    Ok(())
}

fn reset(args: ResetArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (_normalized_session, saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    let managed_environment = resolve_managed_environment_for_reset(&args, saved_state.as_ref())?;
    let managed_distro = managed_environment
        .as_ref()
        .map(|environment| resolve_reset_distro_name(args.distro.as_deref(), environment))
        .transpose()?;

    if args.dry_run {
        println!("Pane Reset Plan");
        if workspace.root.exists() {
            println!(
                "  Session Workspace  would remove {}",
                workspace.root.display()
            );
        } else {
            println!(
                "  Session Workspace  no workspace exists at {}",
                workspace.root.display()
            );
        }
        if args.purge_wsl {
            let purge_target = managed_distro.clone().or_else(|| {
                resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())
                    .ok()
            });
            let distro = purge_target.as_deref().unwrap_or("not resolved");
            println!(
                "  WSL Purge          would stop XRDP and remove Pane session assets in {}",
                distro
            );
        }
        if let Some(environment) = &managed_environment {
            if args.release_managed_environment {
                println!(
                    "  Managed Reset      would release Pane management for {} without deleting the distro",
                    environment.distro_name
                );
            }
            if args.factory_reset {
                println!(
                    "  Managed Reset      would unregister {} from WSL and clear Pane ownership",
                    environment.distro_name
                );
                if let Some(install_dir) = &environment.install_dir {
                    println!(
                        "  Install Root       would remove {}",
                        install_dir.display()
                    );
                }
            }
        }
        if saved_launch.is_some() || args.session_name.is_none() {
            println!("  Saved State        would clear the saved launch state");
        }
        println!("  Dry Run            no files, WSL distros, or Pane state were changed");
        return Ok(());
    }

    if args.purge_wsl && !args.factory_reset {
        if let Ok(distro) =
            resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref())
        {
            for note in purge_wsl_integration(&distro, &inventory)? {
                println!("WSL purge: {note}");
            }
        }
    }

    if let Some(environment) = &managed_environment {
        let distro_name = managed_distro
            .as_deref()
            .unwrap_or(&environment.distro_name);

        if args.factory_reset {
            if inventory.available && inventory_contains_distro(&inventory, distro_name) {
                for note in purge_wsl_integration(distro_name, &inventory)? {
                    println!("Factory reset: {note}");
                }
                let transcript = wsl::unregister_distro(distro_name)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL unregister failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }
                println!("Unregistered {distro_name} from WSL.");
            } else {
                println!("Managed distro {distro_name} was not present in WSL.");
            }

            if let Some(install_dir) = &environment.install_dir {
                if install_dir.exists() {
                    fs::remove_dir_all(install_dir)?;
                    println!("Removed {}.", install_dir.display());
                } else {
                    println!(
                        "No managed install root existed at {}.",
                        install_dir.display()
                    );
                }
            }
        }

        clear_managed_environment(Some(distro_name))?;
        if args.factory_reset {
            println!("Cleared Pane ownership for {distro_name} after factory reset.");
        } else {
            println!("Released Pane management for {distro_name} without deleting the distro.");
        }
    }

    if workspace.root.exists() {
        fs::remove_dir_all(&workspace.root)?;
        println!("Removed {}.", workspace.root.display());
    } else {
        println!("No Pane workspace existed at {}.", workspace.root.display());
    }

    if saved_launch.is_some() || args.session_name.is_none() {
        clear_state()?;
        println!("Cleared saved Pane state.");
    }

    Ok(())
}

fn logs(args: LogsArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (normalized_session, _saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());

    println!("Pane Logs");
    println!("  Session        {}", normalized_session);
    println!("  Bootstrap Log  {}", workspace.bootstrap_log.display());
    println!();

    if workspace.bootstrap_log.exists() {
        println!("Bootstrap Transcript");
        println!("{}", fs::read_to_string(&workspace.bootstrap_log)?);
    } else {
        println!("Bootstrap Transcript");
        println!("  No bootstrap log has been captured for this session.");
    }

    let distro =
        resolve_operational_distro(args.distro.as_deref(), &inventory, saved_state.as_ref()).ok();
    if let Some(distro) = distro.filter(|_| inventory.available) {
        let live_logs = wsl::tail_xrdp_logs(&distro, args.lines)?;
        println!();
        println!("Live XRDP Logs");
        if live_logs.trim().is_empty() {
            println!("  No XRDP log output was found inside {distro}.");
        } else {
            println!("{}", live_logs.trim_end());
        }
    }

    Ok(())
}

fn bundle(args: BundleArgs) -> AppResult<()> {
    let inventory = probe_inventory();
    let saved_state = load_state()?;
    let (session_name, saved_launch, workspace) =
        resolve_session_context(args.session_name.as_deref(), saved_state.as_ref());
    let status_report =
        build_status_report(args.distro.as_deref(), &inventory, saved_state.as_ref())?;
    let doctor_request = build_bundle_doctor_request(
        &session_name,
        args.distro,
        saved_launch.as_ref(),
        saved_state.as_ref(),
        &inventory,
    )?;
    let doctor_report = evaluate_doctor(&doctor_request, &inventory, saved_state.as_ref())?;
    let output_zip = resolve_bundle_output_path(args.output.as_deref(), &session_name);
    let staging_root = app_root().join("support").join(format!(
        "bundle-{}-{}",
        session_name,
        current_epoch_seconds()
    ));

    if staging_root.exists() {
        fs::remove_dir_all(&staging_root)?;
    }
    fs::create_dir_all(&staging_root)?;

    let manifest = write_support_bundle(
        &staging_root,
        &output_zip,
        &session_name,
        saved_state.as_ref(),
        &workspace,
        &status_report,
        &doctor_report,
    )?;

    if let Err(error) = compress_bundle_dir(&staging_root, &output_zip) {
        return Err(AppError::message(format!(
            "{error} Staged files remain at {}.",
            staging_root.display()
        )));
    }

    let _ = fs::remove_dir_all(&staging_root);

    println!("Pane Support Bundle");
    println!("  Session        {}", session_name);
    println!("  Output         {}", output_zip.display());
    println!("  Included Files {}", manifest.included_files.len());
    if !manifest.notes.is_empty() {
        println!("Notes");
        for note in manifest.notes {
            println!("  - {}", note);
        }
    }

    Ok(())
}

fn build_status_report(
    explicit_distro: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<StatusReport> {
    let distro_name = resolve_status_distro(explicit_distro, inventory, saved_state)?;
    let selected_distro = distro_name
        .as_deref()
        .map(|name| build_distro_health(name, inventory, saved_state, None))
        .transpose()?;
    let last_launch_workspace = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| inspect_workspace(&launch.workspace));

    Ok(StatusReport {
        platform: std::env::consts::OS,
        wsl_available: inventory.available,
        wsl_version_banner: inventory.version_banner.clone(),
        managed_environment: saved_state.and_then(|state| state.managed_environment.clone()),
        selected_distro,
        known_distros: inventory.distros.clone(),
        last_launch: saved_state.and_then(|state| state.last_launch.clone()),
        last_launch_workspace,
    })
}

fn resolve_session_context(
    session_name: Option<&str>,
    saved_state: Option<&PaneState>,
) -> (String, Option<StoredLaunch>, WorkspacePaths) {
    let requested_session = session_name
        .map(ToString::to_string)
        .or_else(|| {
            saved_state
                .and_then(|state| state.last_launch.as_ref())
                .map(|launch| launch.session_name.clone())
        })
        .unwrap_or_else(|| "pane".to_string());
    let normalized_session = crate::plan::sanitize_session_name(&requested_session);
    let saved_launch = saved_state
        .and_then(|state| state.last_launch.clone())
        .filter(|launch| launch.session_name == normalized_session);
    let workspace = saved_launch
        .as_ref()
        .map(|launch| launch.workspace.clone())
        .unwrap_or_else(|| workspace_for(&normalized_session));

    (normalized_session, saved_launch, workspace)
}

fn build_bundle_doctor_request(
    session_name: &str,
    explicit_distro: Option<String>,
    saved_launch: Option<&StoredLaunch>,
    saved_state: Option<&PaneState>,
    inventory: &WslInventory,
) -> AppResult<DoctorRequest> {
    let distro = explicit_distro
        .or_else(|| saved_launch.map(|launch| launch.distro.name.clone()))
        .or(resolve_status_distro(None, inventory, saved_state)?);
    let port = distro
        .as_deref()
        .map(|name| status_port_for(name, saved_state))
        .or_else(|| saved_launch.map(|launch| launch.port))
        .unwrap_or(3390);

    Ok(DoctorRequest {
        distro,
        session_name: session_name.to_string(),
        desktop_environment: saved_launch
            .map(|launch| launch.desktop_environment)
            .unwrap_or(DesktopEnvironment::Xfce),
        port,
        bootstrap_requested: saved_launch
            .map(|launch| {
                launch.bootstrap_requested && launch.bootstrapped_at_epoch_seconds.is_none()
            })
            .unwrap_or(true),
        connect_requested: saved_launch
            .map(|launch| launch.connect_requested)
            .unwrap_or(true),
    })
}

fn resolve_bundle_output_path(explicit: Option<&Path>, session_name: &str) -> PathBuf {
    let default_name = default_bundle_file_name(session_name);

    match explicit {
        Some(path) if path.is_dir() => path.join(default_name),
        Some(path) if path.extension().is_none() => path.with_extension("zip"),
        Some(path) => path.to_path_buf(),
        None => app_root().join("support").join(default_name),
    }
}

fn default_bundle_file_name(session_name: &str) -> String {
    format!(
        "pane-support-{}-{}.zip",
        session_name,
        current_epoch_seconds()
    )
}

fn write_support_bundle(
    staging_root: &Path,
    output_zip: &Path,
    session_name: &str,
    saved_state: Option<&PaneState>,
    workspace: &WorkspacePaths,
    status_report: &StatusReport,
    doctor_report: &DoctorReport,
) -> AppResult<BundleManifest> {
    let mut included_files = Vec::new();
    let mut notes = Vec::new();

    write_bundle_json(
        staging_root,
        "status.json",
        status_report,
        &mut included_files,
    )?;
    write_bundle_json(
        staging_root,
        "doctor.json",
        doctor_report,
        &mut included_files,
    )?;

    if let Some(state) = saved_state {
        write_bundle_json(staging_root, "state.json", state, &mut included_files)?;
    } else {
        notes.push("Pane state has not been written yet.".to_string());
    }

    let shared_directory = shared_dir_for_workspace(workspace);
    let shared_details = format!(
        "Session: {}\nWindows Path: {}\nWSL Path: {}\nLinux Link: ~/PaneShared\n",
        session_name,
        shared_directory.display(),
        windows_to_wsl_path(&shared_directory)
    );
    write_bundle_text(
        staging_root,
        "shared-directory.txt",
        &shared_details,
        &mut included_files,
    )?;

    copy_bundle_file_if_exists(
        &workspace.bootstrap_script,
        staging_root,
        "workspace/pane-bootstrap.sh",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.rdp_profile,
        staging_root,
        "workspace/pane.rdp",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.bootstrap_log,
        staging_root,
        "workspace/bootstrap.log",
        &mut included_files,
        &mut notes,
    )?;
    copy_bundle_file_if_exists(
        &workspace.transport_log,
        staging_root,
        "workspace/transport.log",
        &mut included_files,
        &mut notes,
    )?;

    if let Some(distro) = doctor_report
        .target_distro
        .as_deref()
        .filter(|_| status_report.wsl_available)
    {
        match wsl::tail_xrdp_logs(distro, 100) {
            Ok(logs) if logs.trim().is_empty() => {
                notes.push(format!(
                    "No live XRDP logs were available inside {}.",
                    distro
                ));
            }
            Ok(logs) => {
                write_bundle_text(
                    staging_root,
                    "wsl-xrdp-logs.txt",
                    &logs,
                    &mut included_files,
                )?;
            }
            Err(error) => {
                notes.push(format!(
                    "Could not collect live XRDP logs from {}: {}",
                    distro, error
                ));
            }
        }
    } else {
        notes.push("No live WSL distro was available for XRDP log collection.".to_string());
    }

    let mut manifest_files = included_files.clone();
    manifest_files.push("manifest.json".to_string());
    let manifest = BundleManifest {
        created_at_epoch_seconds: current_epoch_seconds(),
        session_name: session_name.to_string(),
        selected_distro: doctor_report.target_distro.clone(),
        output_zip: output_zip.display().to_string(),
        included_files: manifest_files,
        notes,
    };

    write_bundle_json(
        staging_root,
        "manifest.json",
        &manifest,
        &mut included_files,
    )?;
    Ok(manifest)
}

fn write_bundle_json<T: Serialize>(
    staging_root: &Path,
    relative_path: &str,
    value: &T,
    included_files: &mut Vec<String>,
) -> AppResult<()> {
    let path = staging_root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, serde_json::to_string_pretty(value)?)?;
    included_files.push(relative_path.replace('\\', "/"));
    Ok(())
}

fn write_bundle_text(
    staging_root: &Path,
    relative_path: &str,
    value: &str,
    included_files: &mut Vec<String>,
) -> AppResult<()> {
    let path = staging_root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, value)?;
    included_files.push(relative_path.replace('\\', "/"));
    Ok(())
}

fn copy_bundle_file_if_exists(
    source: &Path,
    staging_root: &Path,
    relative_path: &str,
    included_files: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> AppResult<()> {
    if source.exists() {
        let destination = staging_root.join(relative_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(source, &destination)?;
        included_files.push(relative_path.replace('\\', "/"));
    } else {
        notes.push(format!(
            "{} was not found at {}.",
            relative_path.replace('\\', "/"),
            source.display()
        ));
    }

    Ok(())
}

fn compress_bundle_dir(staging_root: &Path, output_zip: &Path) -> AppResult<()> {
    if let Some(parent) = output_zip.parent() {
        fs::create_dir_all(parent)?;
    }

    let archive_input = format!("{}\\*", staging_root.display());
    let command = format!(
        "Compress-Archive -Path {} -DestinationPath {} -Force",
        powershell_quote(&archive_input),
        powershell_quote(&output_zip.display().to_string()),
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &command])
        .output()
        .map_err(|error| {
            AppError::message(format!(
                "failed to run PowerShell compression for {}: {error}",
                output_zip.display()
            ))
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let details = format!("{} {}", stdout.trim(), stderr.trim())
        .trim()
        .to_string();

    Err(AppError::message(format!(
        "failed to create support bundle at {}{}",
        output_zip.display(),
        if details.is_empty() {
            ".".to_string()
        } else {
            format!(": {}", details)
        }
    )))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn initialize_managed_arch_environment(
    args: &InitArgs,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<InitReport> {
    let source = resolve_init_source(args, inventory)?;
    let mut notes = Vec::new();
    let existing_managed_environment = saved_state
        .and_then(|state| state.managed_environment.as_ref())
        .filter(|environment| environment.is_arch())
        .cloned();

    let managed_environment = match source {
        InitSource::AdoptExisting { distro_name } => {
            let distro = validate_arch_distro(&distro_name, inventory)?;
            if args.existing_distro.is_some()
                && !distro.name.eq_ignore_ascii_case(&args.distro_name)
            {
                notes.push(format!(
                    "Adopted the existing WSL distro '{}' instead of creating a new '{}'.",
                    distro.name, args.distro_name
                ));
            } else {
                notes.push(format!(
                    "Pane will use the existing Arch distro '{}' as its managed environment.",
                    distro.name
                ));
            }

            if let Some(existing) = existing_managed_environment
                .as_ref()
                .filter(|environment| environment.distro_name.eq_ignore_ascii_case(&distro.name))
            {
                notes.push(format!(
                    "Preserved Pane ownership metadata for '{}' because it is already the managed Arch environment.",
                    distro.name
                ));
                ManagedEnvironmentState {
                    environment_id: existing.environment_id.clone(),
                    distro_name: distro.name,
                    family: DistroFamily::Arch,
                    ownership: existing.ownership,
                    install_dir: existing.install_dir.clone(),
                    source_rootfs: existing.source_rootfs.clone(),
                    created_at_epoch_seconds: existing.created_at_epoch_seconds,
                }
            } else {
                ManagedEnvironmentState {
                    environment_id: "arch".to_string(),
                    distro_name: distro.name,
                    family: DistroFamily::Arch,
                    ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                    install_dir: None,
                    source_rootfs: None,
                    created_at_epoch_seconds: current_epoch_seconds(),
                }
            }
        }
        InitSource::InstallOnline {
            distro_name,
            install_dir,
        } => {
            if args.dry_run {
                notes.push(format!(
                    "Pane would install the official Arch WSL image as '{}' into {} using the WSL online install path.",
                    distro_name,
                    install_dir.display()
                ));
            } else {
                if !inventory.available {
                    return Err(AppError::message(
                        "wsl.exe was not found. Install WSL2 before Pane can provision Arch automatically.",
                    ));
                }

                ensure_managed_install_dir_available(&install_dir)?;
                let transcript =
                    wsl::install_online_distro("archlinux", &distro_name, &install_dir)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL online install failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }

                let refreshed_inventory = probe_inventory();
                let installed = validate_arch_distro(&distro_name, &refreshed_inventory)?;
                notes.push(format!(
                    "Installed the official Arch WSL image as '{}' in {}.",
                    installed.name,
                    install_dir.display()
                ));
            }

            ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name,
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::InstalledOnline,
                install_dir: Some(install_dir),
                source_rootfs: None,
                created_at_epoch_seconds: current_epoch_seconds(),
            }
        }
        InitSource::ImportRootfs {
            distro_name,
            rootfs_tar,
            install_dir,
        } => {
            if !rootfs_tar.exists() {
                return Err(AppError::message(format!(
                    "The Arch rootfs tarball was not found at {}.",
                    rootfs_tar.display()
                )));
            }

            if args.dry_run {
                notes.push(format!(
                    "Pane would import {} into {} from {}.",
                    distro_name,
                    install_dir.display(),
                    rootfs_tar.display()
                ));
            } else {
                ensure_managed_install_dir_available(&install_dir)?;
                let transcript = wsl::import_distro(&distro_name, &install_dir, &rootfs_tar)?;
                if !transcript.success {
                    return Err(AppError::message(format!(
                        "WSL import failed for '{}': {}",
                        distro_name,
                        transcript.combined_output().trim()
                    )));
                }

                let refreshed_inventory = probe_inventory();
                let imported = validate_arch_distro(&distro_name, &refreshed_inventory)?;
                notes.push(format!(
                    "Imported {} into {} as a Pane-managed Arch environment.",
                    imported.name,
                    install_dir.display()
                ));
            }

            ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name,
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::ImportedRootfs,
                install_dir: Some(install_dir),
                source_rootfs: Some(rootfs_tar),
                created_at_epoch_seconds: current_epoch_seconds(),
            }
        }
    };

    let present_in_inventory =
        inventory_contains_distro(&probe_inventory(), &managed_environment.distro_name);
    if !args.dry_run {
        save_managed_environment(managed_environment.clone())?;
    }

    if args.dry_run {
        notes.push("Dry run only. No WSL distro or Pane state was modified.".to_string());
    } else {
        notes.push(format!(
            "Pane will now prefer '{}' when no --distro override is provided.",
            managed_environment.distro_name
        ));
    }

    Ok(InitReport {
        product_shape: "Pane owns a dedicated Arch environment first, then layers launch/bootstrap/recovery on top of it.",
        managed_environment,
        dry_run: args.dry_run,
        present_in_inventory,
        notes,
    })
}

fn resolve_init_source(args: &InitArgs, inventory: &WslInventory) -> AppResult<InitSource> {
    if args.existing_distro.is_some() && args.rootfs_tar.is_some() {
        return Err(AppError::message(
            "Choose either --existing-distro or --rootfs-tar for `pane init`, not both.",
        ));
    }

    if args.existing_distro.is_some() && args.install_dir.is_some() {
        return Err(AppError::message(
            "--install-dir is only valid when Pane is provisioning a managed distro, not with --existing-distro.",
        ));
    }

    if let Some(existing_distro) = &args.existing_distro {
        return Ok(InitSource::AdoptExisting {
            distro_name: existing_distro.clone(),
        });
    }

    if let Some(rootfs_tar) = &args.rootfs_tar {
        return Ok(InitSource::ImportRootfs {
            distro_name: args.distro_name.clone(),
            rootfs_tar: rootfs_tar.clone(),
            install_dir: args
                .install_dir
                .clone()
                .unwrap_or_else(|| managed_distro_install_root(&args.distro_name)),
        });
    }

    if inventory.available && inventory_contains_distro(inventory, &args.distro_name) {
        return Ok(InitSource::AdoptExisting {
            distro_name: args.distro_name.clone(),
        });
    }

    Ok(InitSource::InstallOnline {
        distro_name: args.distro_name.clone(),
        install_dir: args
            .install_dir
            .clone()
            .unwrap_or_else(|| managed_distro_install_root(&args.distro_name)),
    })
}

fn validate_arch_distro(name: &str, inventory: &WslInventory) -> AppResult<DistroRecord> {
    if !inventory.available {
        return Err(AppError::message(
            "wsl.exe was not found. Install WSL2 before initializing Pane Arch.",
        ));
    }

    if !inventory_contains_distro(inventory, name) {
        return Err(AppError::message(format!(
            "WSL distro '{}' was not found. Available distros: {}",
            name,
            available_distros(inventory)
        )));
    }

    let distro = wsl::inspect_distro(name, inventory)?;
    if distro.family != DistroFamily::Arch {
        return Err(AppError::message(format!(
            "Pane can only initialize Arch right now, but '{}' resolved to {}.",
            distro.name,
            distro.family.display_name()
        )));
    }

    Ok(distro)
}

fn ensure_managed_install_dir_available(install_dir: &Path) -> AppResult<()> {
    if install_dir.exists() {
        if !install_dir.is_dir() {
            return Err(AppError::message(format!(
                "{} already exists and is not a directory.",
                install_dir.display()
            )));
        }

        let mut entries = fs::read_dir(install_dir)?;
        if entries.next().transpose()?.is_some() {
            return Err(AppError::message(format!(
                "{} is not empty. Choose an empty install directory for Pane-managed Arch provisioning.",
                install_dir.display()
            )));
        }
    } else {
        fs::create_dir_all(install_dir)?;
    }

    Ok(())
}

fn resolve_managed_environment_for_reset(
    args: &ResetArgs,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<ManagedEnvironmentState>> {
    if !args.release_managed_environment && !args.factory_reset {
        return Ok(None);
    }

    let Some(environment) = saved_state.and_then(|state| state.managed_environment.clone()) else {
        return Err(AppError::message(
            "Pane is not currently managing a distro. Run `pane init` first.",
        ));
    };

    if args.factory_reset && !environment.ownership.can_factory_reset() {
        return Err(AppError::message(format!(
            "Factory reset is only supported for Pane-provisioned distros. '{}' is {}. Use `pane reset --release-managed-environment` instead.",
            environment.distro_name,
            environment.ownership.display_name()
        )));
    }

    Ok(Some(environment))
}

fn resolve_reset_distro_name(
    explicit: Option<&str>,
    managed_environment: &ManagedEnvironmentState,
) -> AppResult<String> {
    if let Some(name) = explicit {
        if !name.eq_ignore_ascii_case(&managed_environment.distro_name) {
            return Err(AppError::message(format!(
                "The requested reset target '{}' does not match the managed distro '{}'.",
                name, managed_environment.distro_name
            )));
        }

        return Ok(name.to_string());
    }

    Ok(managed_environment.distro_name.clone())
}

fn purge_wsl_integration(distro: &str, inventory: &WslInventory) -> AppResult<Vec<String>> {
    if !inventory.available || !inventory_contains_distro(inventory, distro) {
        return Ok(vec![format!(
            "No live WSL distro named '{}' was available for purge.",
            distro
        )]);
    }

    let mut notes = Vec::new();
    let stopped = wsl::stop_xrdp_services(distro)?;
    notes.push(format!("XRDP stop result: {}", stopped.trim()));

    let inspected = wsl::inspect_distro(distro, inventory)?;
    if let Some(user) = inspected
        .default_user
        .as_deref()
        .filter(|user| !user.eq_ignore_ascii_case("root"))
    {
        let result = wsl::remove_pane_session_assets(distro, user)?;
        notes.push(format!("Pane session asset cleanup: {}", result.trim()));
    } else {
        notes.push(
            "No non-root default user was available for Pane session-asset cleanup.".to_string(),
        );
    }

    Ok(notes)
}

fn managed_arch_name(saved_state: Option<&PaneState>) -> Option<String> {
    saved_state
        .and_then(|state| state.managed_environment.as_ref())
        .filter(|environment| environment.is_arch())
        .map(|environment| environment.distro_name.clone())
}

fn resolve_launch_target(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
    dry_run: bool,
) -> AppResult<LaunchTarget> {
    if let Some(name) = explicit {
        if inventory.available {
            if !inventory_contains_distro(inventory, name) {
                return Err(AppError::message(format!(
                    "WSL distro '{}' was not found. Available distros: {}",
                    name,
                    available_distros(inventory)
                )));
            }

            return Ok(LaunchTarget {
                distro: wsl::inspect_distro(name, inventory)?,
                hypothetical: false,
            });
        }

        if dry_run {
            return Ok(LaunchTarget {
                distro: DistroRecord {
                    name: name.to_string(),
                    family: DistroFamily::Arch,
                    pretty_name: Some(name.to_string()),
                    ..DistroRecord::default()
                },
                hypothetical: true,
            });
        }

        return Err(AppError::message(
            "wsl.exe was not found. Install WSL2 or rerun with --dry-run --distro <arch-distro-name>.",
        ));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        if inventory.available && inventory_contains_distro(inventory, &name) {
            return Ok(LaunchTarget {
                distro: wsl::inspect_distro(&name, inventory)?,
                hypothetical: false,
            });
        }

        if dry_run {
            return Ok(LaunchTarget {
                distro: DistroRecord {
                    name: name.clone(),
                    family: DistroFamily::Arch,
                    pretty_name: Some(name),
                    ..DistroRecord::default()
                },
                hypothetical: true,
            });
        }

        return Err(AppError::message(
            "Pane has a managed Arch environment configured, but it is not currently installed in WSL. Re-run `pane init` or pass --distro <arch-distro-name> to override it.",
        ));
    }

    if let Some(distro) = find_supported_arch_distro(inventory)? {
        return Ok(LaunchTarget {
            distro,
            hypothetical: false,
        });
    }

    if !inventory.available {
        return Err(AppError::message(
            "No WSL installation was found. Install WSL2 and Arch Linux first, or rerun with --dry-run --distro <arch-distro-name>.",
        ));
    }

    Err(AppError::message(format!(
        "Pane MVP currently supports Arch Linux + XFCE only. Installed distros: {}",
        available_distros(inventory)
    )))
}

fn resolve_status_distro(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(name.to_string()));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        return Ok(Some(name));
    }

    if let Some(name) = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| launch.distro.name.clone())
    {
        return Ok(Some(name));
    }

    if let Some(distro) = find_supported_arch_distro(inventory)? {
        return Ok(Some(distro.name));
    }

    Ok(inventory
        .default_distro
        .clone()
        .or_else(|| inventory.distros.first().map(|item| item.name.clone())))
}

fn resolve_operational_distro(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<String> {
    resolve_status_distro(explicit, inventory, saved_state)?.ok_or_else(|| {
        AppError::message(
            "No WSL distro could be resolved. Run `pane doctor` or pass --distro <arch-distro-name>.",
        )
    })
}

fn find_supported_arch_distro(inventory: &WslInventory) -> AppResult<Option<DistroRecord>> {
    if !inventory.available {
        return Ok(None);
    }

    let mut candidate_names = Vec::new();
    if let Some(default_distro) = &inventory.default_distro {
        candidate_names.push(default_distro.clone());
    }
    for distro in &inventory.distros {
        if !candidate_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&distro.name))
        {
            candidate_names.push(distro.name.clone());
        }
    }

    for name in candidate_names {
        let distro = wsl::inspect_distro(&name, inventory)?;
        if distro.is_mvp_supported() {
            return Ok(Some(distro));
        }
    }

    Ok(None)
}

fn evaluate_doctor(
    request: &DoctorRequest,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<DoctorReport> {
    let mut checks = Vec::new();
    let workspace = workspace_for(&request.session_name);
    let workspace_health = inspect_workspace(&workspace);
    let windows_host = cfg!(windows);

    push_check(
        &mut checks,
        if windows_host {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "windows-host",
        if windows_host {
            "Pane MVP is running on Windows.".to_string()
        } else {
            "Pane MVP currently supports Windows hosts only.".to_string()
        },
        (!windows_host).then_some(
            "Run Pane from Windows 10 or 11, then use WSL2 for the Linux side.".to_string(),
        ),
    );

    push_check(
        &mut checks,
        if inventory.available {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "wsl-available",
        if inventory.available {
            "wsl.exe is available on the Windows host.".to_string()
        } else {
            "WSL is not available on this host.".to_string()
        },
        (!inventory.available)
            .then_some("Install WSL2, install Arch Linux, then rerun `pane doctor`.".to_string()),
    );

    let workspace_status = if ensure_workspace_writable(&workspace) {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    };
    push_check(
        &mut checks,
        workspace_status,
        "workspace-writable",
        if workspace_status == CheckStatus::Pass {
            format!("Pane can write assets under {}.", workspace.root.display())
        } else {
            format!(
                "Pane could not write assets under {}.",
                workspace.root.display()
            )
        },
        (workspace_status == CheckStatus::Fail).then_some(
            "Ensure your LOCALAPPDATA directory is writable, then rerun `pane doctor`.".to_string(),
        ),
    );

    let shared_directory_status = if ensure_shared_dir_writable(&workspace) {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    };
    push_check(
        &mut checks,
        shared_directory_status,
        "shared-directory-writable",
        if shared_directory_status == CheckStatus::Pass {
            format!(
                "Pane can create the shared directory under {}.",
                shared_dir_for_workspace(&workspace).display()
            )
        } else {
            format!(
                "Pane could not create the shared directory under {}.",
                shared_dir_for_workspace(&workspace).display()
            )
        },
        (shared_directory_status == CheckStatus::Fail).then_some(
            "Ensure your LOCALAPPDATA directory is writable so Pane can create the shared Windows-side workspace."
                .to_string(),
        ),
    );

    if request.connect_requested {
        let mstsc_status = if mstsc_available() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        };
        push_check(
            &mut checks,
            mstsc_status,
            "mstsc-available",
            if mstsc_status == CheckStatus::Pass {
                "mstsc.exe is available for the Windows RDP handoff.".to_string()
            } else {
                "mstsc.exe was not found on the Windows host.".to_string()
            },
            (mstsc_status == CheckStatus::Fail).then_some(
                "Enable the built-in Remote Desktop Connection client or restore mstsc.exe on Windows.".to_string(),
            ),
        );
    }

    let target_name = select_doctor_target(request.distro.as_deref(), inventory, saved_state)?;
    let selected_distro = target_name
        .as_deref()
        .map(|name| build_distro_health(name, inventory, saved_state, Some(request.port)))
        .transpose()?;

    if let Some(name) = &target_name {
        let present_in_inventory = inventory_contains_distro(inventory, name);
        push_check(
            &mut checks,
            if inventory.available && present_in_inventory {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "distro-present",
            if inventory.available && present_in_inventory {
                format!("WSL distro '{name}' is installed.")
            } else {
                format!("WSL distro '{name}' is not installed.")
            },
            (!(inventory.available && present_in_inventory)).then_some(format!(
                "Install or import an Arch Linux distro, then rerun with `--distro {name}` if needed."
            )),
        );
    } else {
        push_check(
            &mut checks,
            CheckStatus::Fail,
            "distro-selected",
            "No WSL distro could be selected for the MVP path.".to_string(),
            Some(
                "Install Arch Linux for WSL, or pass --distro <arch-distro-name> to Pane once it exists.".to_string(),
            ),
        );
    }

    let desktop_supported = request.desktop_environment.is_mvp_supported();
    push_check(
        &mut checks,
        if desktop_supported {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        "desktop-supported",
        if desktop_supported {
            format!(
                "{} is the supported MVP desktop environment.",
                request.desktop_environment.display_name()
            )
        } else {
            format!(
                "{} is not supported in the current MVP.",
                request.desktop_environment.display_name()
            )
        },
        (!desktop_supported).then_some(
            "Use `--de xfce`. KDE and GNOME are intentionally deferred until after the MVP."
                .to_string(),
        ),
    );

    if let Some(health) = &selected_distro {
        push_check(
            &mut checks,
            if health.supported_for_mvp {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "distro-supported",
            if health.supported_for_mvp {
                format!(
                    "{} matches the Arch Linux MVP support boundary.",
                    health.distro.label()
                )
            } else {
                format!(
                    "{} is not supported by the current MVP.",
                    health.distro.label()
                )
            },
            (!health.supported_for_mvp).then_some(
                "Install Arch Linux for WSL and rerun Pane against that distro.".to_string(),
            ),
        );

        let default_user_ok = health
            .distro
            .default_user
            .as_deref()
            .is_some_and(|user| !user.eq_ignore_ascii_case("root"));
        push_check(
            &mut checks,
            if default_user_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "default-user",
            if default_user_ok {
                format!(
                    "The default WSL user is '{}'.",
                    health.distro.default_user.as_deref().unwrap_or_default()
                )
            } else {
                "The default WSL user is missing or still set to root.".to_string()
            },
            (!default_user_ok).then_some(format!(
                "Use the Pane Control Center's Setup User flow or run `pane setup-user --username <linux-user> --password-stdin` for {} before using Pane.",
                health.distro.name
            )),
        );

        let password_status = health.default_user_password_status;
        let password_ok = password_status.is_some_and(PasswordStatus::is_usable);
        push_check(
            &mut checks,
            if password_ok {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "default-user-password",
            if let Some(status) = password_status {
                format!(
                    "The default user password state is {}.",
                    status.display_name()
                )
            } else {
                "Pane could not verify the default user password state.".to_string()
            },
            (!password_ok).then_some(format!(
                "Use the Pane Control Center's Setup User flow or rerun `pane setup-user --username <linux-user> --password-stdin --distro {}` so XRDP has a usable login password.",
                health.distro.name
            )),
        );

        let systemd_configured = health.systemd_configured == Some(true);
        push_check(
            &mut checks,
            if systemd_configured {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "systemd-configured",
            if systemd_configured {
                "`/etc/wsl.conf` advertises systemd=true.".to_string()
            } else {
                "`/etc/wsl.conf` does not yet advertise systemd=true.".to_string()
            },
            (!systemd_configured).then_some(format!(
                "Use the Pane Control Center's Setup User flow or run `pane setup-user --username <linux-user> --password-stdin --distro {}` to write systemd=true and restart WSL.",
                health.distro.name
            )),
        );

        let systemd_active = wsl::distro_systemd_active(&health.distro.name) == Some(true);
        push_check(
            &mut checks,
            if systemd_active {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            "systemd-active",
            if systemd_active {
                "systemd is active in the running WSL instance.".to_string()
            } else {
                "systemd is not active in the running WSL instance.".to_string()
            },
            (!systemd_active).then_some(
                "After enabling systemd in /etc/wsl.conf, run `wsl --shutdown` and start the distro again before retrying Pane.".to_string(),
            ),
        );

        if !request.bootstrap_requested {
            push_check(
                &mut checks,
                if health.xrdp_installed == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-installed",
                if health.xrdp_installed == Some(true) {
                    "XRDP is installed inside the distro.".to_string()
                } else {
                    "XRDP is not installed inside the distro.".to_string()
                },
                Some(
                    "Run `pane launch` without `--skip-bootstrap` to install and configure XRDP."
                        .to_string(),
                ),
            );
            push_check(
                &mut checks,
                if health.pane_session_assets_ready == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "pane-session-assets",
                if health.pane_session_assets_ready == Some(true) {
                    "Pane-managed XRDP session assets are present for the default user.".to_string()
                } else {
                    "Pane-managed XRDP session assets are missing or stale for the default user.".to_string()
                },
                Some("Run `pane repair` or `pane launch` to rewrite the Pane-managed session launcher, XRDP user files, and notifyd override.".to_string()),
            );
            push_check(
                &mut checks,
                if health.user_home_ready == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "user-home-ready",
                if health.user_home_ready == Some(true) {
                    "The default user owns and can write the required XFCE config/cache directories.".to_string()
                } else {
                    "The default user home/config layout is missing required directories or is not writable by the Linux user.".to_string()
                },
                Some("Run `pane repair` to recreate and re-own the Pane-managed XFCE config, cache, and local-state directories.".to_string()),
            );
            push_check(
                &mut checks,
                if health.xrdp_service_active == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-active",
                if health.xrdp_service_active == Some(true) {
                    "The XRDP service is active inside WSL.".to_string()
                } else {
                    "The XRDP service is not active inside WSL.".to_string()
                },
                Some("Run `pane launch` or `pane stop` followed by `pane launch` to restart XRDP cleanly.".to_string()),
            );
            push_check(
                &mut checks,
                if health.xrdp_listening == Some(true) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
                "xrdp-listening",
                if health.xrdp_listening == Some(true) {
                    format!("XRDP is listening on port {} inside WSL.", request.port)
                } else {
                    format!("XRDP is not listening on port {} inside WSL.", request.port)
                },
                Some(
                    "Run `pane logs` and inspect the bootstrap transcript or XRDP service logs."
                        .to_string(),
                ),
            );
            if request.connect_requested {
                if let Some(check) = windows_transport_check(request.port, health) {
                    checks.push(check);
                }
            }
        }
    }

    let supported_for_mvp = selected_distro
        .as_ref()
        .is_some_and(|health| health.supported_for_mvp)
        && request.desktop_environment.is_mvp_supported();
    let ready = !checks.iter().any(|check| check.status == CheckStatus::Fail);

    Ok(DoctorReport {
        target_distro: target_name,
        session_name: request.session_name.clone(),
        desktop_environment: request.desktop_environment,
        port: request.port,
        bootstrap_requested: request.bootstrap_requested,
        connect_requested: request.connect_requested,
        supported_for_mvp,
        ready,
        selected_distro,
        workspace: workspace_health,
        checks,
    })
}

fn select_doctor_target(
    explicit: Option<&str>,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
) -> AppResult<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(name.to_string()));
    }

    if let Some(name) = managed_arch_name(saved_state) {
        return Ok(Some(name));
    }

    if let Some(name) = saved_state
        .and_then(|state| state.last_launch.as_ref())
        .map(|launch| launch.distro.name.clone())
    {
        return Ok(Some(name));
    }

    if let Some(distro) = find_supported_arch_distro(inventory)? {
        return Ok(Some(distro.name));
    }

    Ok(inventory
        .default_distro
        .clone()
        .or_else(|| inventory.distros.first().map(|item| item.name.clone())))
}

fn build_distro_health(
    name: &str,
    inventory: &WslInventory,
    saved_state: Option<&PaneState>,
    explicit_port: Option<u16>,
) -> AppResult<DistroHealth> {
    let present_in_inventory = inventory_contains_distro(inventory, name);
    let checked_port = explicit_port.unwrap_or_else(|| status_port_for(name, saved_state));
    let fallback_record = saved_state
        .and_then(|state| {
            state
                .managed_environment
                .as_ref()
                .filter(|environment| environment.distro_name.eq_ignore_ascii_case(name))
                .map(|environment| DistroRecord {
                    name: environment.distro_name.clone(),
                    family: environment.family,
                    pretty_name: Some(environment.distro_name.clone()),
                    ..DistroRecord::default()
                })
                .or_else(|| {
                    state
                        .last_launch
                        .as_ref()
                        .filter(|launch| launch.distro.name.eq_ignore_ascii_case(name))
                        .map(|launch| launch.distro.clone())
                })
        })
        .unwrap_or_else(|| DistroRecord {
            name: name.to_string(),
            family: DistroFamily::Unknown,
            ..DistroRecord::default()
        });

    if inventory.available && present_in_inventory {
        let distro = wsl::inspect_distro(name, inventory)?;
        let xrdp_listening = wsl::distro_port_listening(name, checked_port);
        let localhost_reachable = Some(local_port_reachable(checked_port));
        let wsl_ip = wsl::distro_ipv4_address(name);
        let wsl_ip_reachable = wsl_ip
            .map(|ip| SocketAddr::from((ip, checked_port)))
            .map(socket_reachable);
        let pane_relay_available = Some(wsl_ip.is_some() || wsl::distro_command_exists(name, "nc"));
        let remembered_transport = last_launch_transport(name, checked_port, saved_state);
        let pane_default_user = distro
            .default_user
            .as_deref()
            .and_then(|user| (!user.eq_ignore_ascii_case("root")).then_some(user));
        return Ok(DistroHealth {
            supported_for_mvp: distro.is_mvp_supported(),
            present_in_inventory,
            checked_port,
            systemd_configured: wsl::distro_systemd_configured(name),
            xrdp_installed: Some(wsl::distro_command_exists(name, "xrdp")),
            xrdp_service_active: wsl::distro_service_active(name, "xrdp"),
            xrdp_listening,
            localhost_reachable,
            pane_relay_available,
            preferred_transport: remembered_transport.or_else(|| {
                preferred_transport(
                    xrdp_listening,
                    localhost_reachable,
                    wsl_ip_reachable,
                    pane_relay_available,
                )
            }),
            xsession_present: Some(wsl::distro_file_exists(name, ".xsession")),
            pane_session_assets_ready: pane_default_user
                .and_then(|user| wsl::distro_pane_session_assets_ready(name, user)),
            user_home_ready: pane_default_user
                .and_then(|user| wsl::distro_user_home_ready(name, user)),
            default_user_password_status: pane_default_user
                .and_then(|user| wsl::distro_user_password_status(name, user)),
            distro,
        });
    }

    Ok(DistroHealth {
        supported_for_mvp: fallback_record.is_mvp_supported(),
        distro: fallback_record,
        present_in_inventory,
        checked_port,
        systemd_configured: None,
        xrdp_installed: None,
        xrdp_service_active: None,
        xrdp_listening: None,
        localhost_reachable: None,
        pane_relay_available: None,
        preferred_transport: None,
        xsession_present: None,
        pane_session_assets_ready: None,
        user_home_ready: None,
        default_user_password_status: None,
    })
}

fn preferred_transport(
    xrdp_listening: Option<bool>,
    localhost_reachable: Option<bool>,
    wsl_ip_reachable: Option<bool>,
    pane_relay_available: Option<bool>,
) -> Option<LaunchTransport> {
    if xrdp_listening != Some(true) {
        return None;
    }

    if localhost_reachable == Some(true) {
        Some(LaunchTransport::DirectLocalhost)
    } else if wsl_ip_reachable == Some(true) {
        Some(LaunchTransport::DirectWslIp)
    } else if pane_relay_available == Some(true) {
        Some(LaunchTransport::PaneRelay)
    } else {
        None
    }
}

fn last_launch_transport(
    name: &str,
    port: u16,
    saved_state: Option<&PaneState>,
) -> Option<LaunchTransport> {
    saved_state
        .and_then(|state| state.last_launch.as_ref())
        .filter(|launch| {
            launch.stage == LaunchStage::RdpLaunched
                && launch.distro.name.eq_ignore_ascii_case(name)
                && launch.port == port
        })
        .and_then(|launch| launch.transport)
}

fn windows_transport_check(port: u16, health: &DistroHealth) -> Option<DoctorCheck> {
    if health.xrdp_listening != Some(true) {
        return None;
    }

    Some(DoctorCheck {
        id: "windows-transport".to_string(),
        status: if health.preferred_transport.is_some() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        summary: match health.preferred_transport {
            Some(LaunchTransport::DirectLocalhost) => {
                format!("Windows can reach localhost:{} directly.", port)
            }
            Some(LaunchTransport::DirectWslIp) => {
                format!("Windows can reach the distro IP directly on port {}.", port)
            }
            Some(LaunchTransport::PaneRelay) => {
                format!("Pane will bridge localhost:{} with pane-relay.", port)
            }
            None => format!(
                "Windows cannot reach localhost:{}, the distro IP directly, or pane-relay inside WSL.",
                port
            ),
        },
        remediation: if health.preferred_transport.is_some() {
            None
        } else {
            Some(
                "Run `pane repair` or `pane launch` to restore the Pane relay path and WSL networking, then retry the connection."
                    .to_string(),
            )
        },
    })
}

fn build_steps(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
    bootstrap_enabled: bool,
    connect_enabled: bool,
) -> Vec<String> {
    let mut steps = vec![
        format!(
            "Generate a {} bootstrap script for {}.",
            desktop_environment.display_name(),
            distro.label()
        ),
        format!(
            "Write an RDP profile that can target localhost, the distro IP, or pane-relay on port {port}."
        ),
        "Prepare a Pane-managed shared directory that appears inside Arch as ~/PaneShared."
            .to_string(),
        "Run preflight diagnostics that block unsupported or broken MVP setups.".to_string(),
    ];

    if bootstrap_enabled {
        steps.push(format!(
            "Run the bootstrap script inside {} as root and write ~/.xsession for the default WSL user.",
            distro.name
        ));
        steps.push("Wait for XRDP to listen before reporting success.".to_string());
    }

    if connect_enabled {
        steps.push("Launch mstsc.exe with the generated RDP profile.".to_string());
    }

    steps
}

fn build_update_steps(
    distro: &DistroRecord,
    desktop_environment: DesktopEnvironment,
    port: u16,
) -> Vec<String> {
    vec![
        format!(
            "Generate a {} update script for {}.",
            desktop_environment.display_name(),
            distro.label()
        ),
        format!(
            "Refresh Arch packages and reapply the Pane-managed desktop integration inside {}.",
            distro.name
        ),
        format!("Write an RDP profile that can target localhost, the distro IP, or pane-relay on port {port}."),
        "Prepare a Pane-managed shared directory that appears inside Arch as ~/PaneShared."
            .to_string(),
        "Run preflight diagnostics that block unsupported or broken MVP setups.".to_string(),
        format!(
            "Run the update script inside {} as root to refresh packages and restore Pane session wiring.",
            distro.name
        ),
        "Wait for XRDP to listen before reporting success.".to_string(),
    ]
}

fn execute_bootstrap(plan: &LaunchPlan) -> AppResult<()> {
    let target_user = plan.distro.default_user.as_deref().unwrap_or("root");
    let script_path = windows_to_wsl_path(&plan.workspace.bootstrap_script);
    let shared_directory = windows_to_wsl_path(&shared_dir_for_workspace(&plan.workspace));
    let command = format!(
        "chmod +x {script} && PANE_TARGET_USER={user} PANE_SHARED_DIR={shared} {script}",
        script = shell_quote(&script_path),
        user = shell_quote(target_user),
        shared = shell_quote(&shared_directory),
    );

    let transcript = run_wsl_shell_as_user_capture(&plan.distro.name, Some("root"), &command)?;
    write_bootstrap_log(&plan.workspace.bootstrap_log, plan, &command, &transcript)?;

    if transcript.success {
        Ok(())
    } else {
        Err(AppError::message(format!(
            "Bootstrap failed for {}. Review {} for details.",
            plan.distro.name,
            plan.workspace.bootstrap_log.display()
        )))
    }
}

fn write_runtime_rdp_profile(
    profile_path: &Path,
    distro: &DistroRecord,
    host: &str,
    port: u16,
) -> AppResult<()> {
    fs::write(profile_path, render_rdp_profile(distro, host, port)).map_err(|error| {
        AppError::message(format!(
            "failed to write the runtime RDP profile at {}: {error}",
            profile_path.display()
        ))
    })
}

fn open_rdp_profile(profile_path: &Path) -> AppResult<()> {
    Command::new("mstsc.exe")
        .arg(profile_path)
        .spawn()
        .map_err(|error| {
            AppError::message(format!(
                "failed to launch mstsc.exe for {}: {error}",
                profile_path.display()
            ))
        })?;

    Ok(())
}

fn open_directory_in_explorer(path: &Path) -> AppResult<()> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| {
            AppError::message(format!(
                "failed to launch explorer.exe for {}: {error}",
                path.display()
            ))
        })?;

    Ok(())
}

fn fail_launch(stored_launch: &mut StoredLaunch, error: AppError) -> AppError {
    stored_launch.mark_failed(error.to_string());
    let _ = save_state_record(stored_launch.clone());
    error
}

fn build_environment_catalog_report() -> EnvironmentCatalogReport {
    EnvironmentCatalogReport {
        product_shape: "Windows-native Linux environment platform, executed through deeply supported managed environments starting with Arch.",
        strategy: "Arch is the current flagship. Ubuntu LTS is next as the second first-class managed environment. Debian follows later as a curated preview environment.",
        environments: managed_environment_catalog(),
        notes: vec![
            "Arch Linux is the current first-class managed environment and the reference path for Pane.".to_string(),
            "Ubuntu LTS is next because it broadens adoption without changing the product model.".to_string(),
            "Debian follows later as a curated preview once distro lifecycle ownership is stronger.".to_string(),
            "Kali and wider distro imports are intentionally not part of the first three managed environments.".to_string(),
        ],
    }
}

fn resolve_saved_launch(
    session_name: Option<&str>,
    saved_state: Option<&PaneState>,
) -> AppResult<StoredLaunch> {
    let Some(launch) = saved_state.and_then(|state| state.last_launch.clone()) else {
        return Err(AppError::message(
            "Pane has no saved launch state yet. Run `pane launch` first.",
        ));
    };

    if let Some(expected_session) = session_name {
        let normalized = crate::plan::sanitize_session_name(expected_session);
        if launch.session_name != normalized {
            return Err(AppError::message(format!(
                "Pane only tracks one active session in the MVP. The saved session is '{}', not '{}'.",
                launch.session_name,
                normalized
            )));
        }
    }

    Ok(launch)
}

fn inventory_contains_distro(inventory: &WslInventory, name: &str) -> bool {
    inventory
        .distros
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case(name))
}

fn available_distros(inventory: &WslInventory) -> String {
    if inventory.distros.is_empty() {
        "none".to_string()
    } else {
        inventory
            .distros
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn status_port_for(name: &str, saved_state: Option<&PaneState>) -> u16 {
    saved_state
        .and_then(|state| state.last_launch.as_ref())
        .filter(|launch| launch.distro.name.eq_ignore_ascii_case(name))
        .map(|launch| launch.port)
        .unwrap_or(3390)
}

fn inspect_workspace(workspace: &WorkspacePaths) -> WorkspaceHealth {
    WorkspaceHealth {
        root_exists: workspace.root.exists(),
        shared_dir_exists: shared_dir_for_workspace(workspace).exists(),
        bootstrap_script_exists: workspace.bootstrap_script.exists(),
        rdp_profile_exists: workspace.rdp_profile.exists(),
        bootstrap_log_exists: workspace.bootstrap_log.exists(),
        transport_log_exists: workspace.transport_log.exists(),
    }
}

fn ensure_workspace_writable(workspace: &WorkspacePaths) -> bool {
    fs::create_dir_all(&workspace.root).is_ok()
}

fn ensure_shared_dir_writable(workspace: &WorkspacePaths) -> bool {
    fs::create_dir_all(shared_dir_for_workspace(workspace)).is_ok()
}

fn validate_setup_username(username: &str) -> AppResult<()> {
    if username.eq_ignore_ascii_case("root") {
        return Err(AppError::message(
            "Pane setup-user only supports regular Linux users. Choose a non-root username.",
        ));
    }

    let mut chars = username.chars();
    let Some(first) = chars.next() else {
        return Err(AppError::message(
            "Pane setup-user requires --username <linux-user>.",
        ));
    };
    if !first.is_ascii_lowercase() && first != '_' {
        return Err(AppError::message(
            "Linux usernames must start with a lowercase letter or underscore.",
        ));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
        return Err(AppError::message(
            "Linux usernames may only contain lowercase letters, digits, underscores, and dashes.",
        ));
    }
    Ok(())
}

fn validate_setup_password(password: &str) -> AppResult<()> {
    if password.is_empty() {
        return Err(AppError::message(
            "Pane setup-user requires a non-empty password.",
        ));
    }
    if password.contains(':') {
        return Err(AppError::message(
            "Pane setup-user passwords cannot contain ':' because the password is passed to chpasswd.",
        ));
    }
    if password.contains('\n') || password.contains('\r') {
        return Err(AppError::message(
            "Pane setup-user passwords cannot contain line breaks.",
        ));
    }
    Ok(())
}

fn resolve_setup_user_password(args: &SetupUserArgs) -> AppResult<Option<String>> {
    if args.dry_run {
        return Ok(args.password.clone());
    }

    if args.password.is_some() && args.password_stdin {
        return Err(AppError::message(
            "Pass the password either with --password or --password-stdin, not both.",
        ));
    }

    let password = if args.password_stdin {
        let mut raw = String::new();
        std::io::stdin().read_to_string(&mut raw)?;
        raw.trim_end_matches(['\r', '\n']).to_string()
    } else {
        args.password.clone().ok_or_else(|| {
            AppError::message(
                "Provide a password with --password-stdin or --password when using pane setup-user.",
            )
        })?
    };

    validate_setup_password(&password)?;
    Ok(Some(password))
}

fn build_setup_user_shell_command(username: &str) -> String {
    format!(
        "set -euo pipefail\n\n\
         if ! id -u {username} >/dev/null 2>&1; then\n\
           useradd -m -G wheel -s /bin/bash {username}\n\
         else\n\
           usermod -aG wheel {username}\n\
           if command -v chsh >/dev/null 2>&1; then\n\
             chsh -s /bin/bash {username} >/dev/null 2>&1 || true\n\
           fi\n\
         fi\n\
         if ! command -v sudo >/dev/null 2>&1; then\n\
           pacman -Sy --noconfirm sudo >/dev/null 2>&1 || true\n\
         fi\n\
         if command -v sudo >/dev/null 2>&1; then\n\
           if grep -Eq '^[[:space:]]*#\\s*%wheel[[:space:]]+ALL=\\(ALL:ALL\\)[[:space:]]+ALL[[:space:]]*$' /etc/sudoers; then\n\
             sed -i 's/^[[:space:]]*#\\s*%wheel[[:space:]]\\+ALL=(ALL:ALL)[[:space:]]\\+ALL[[:space:]]*$/%wheel ALL=(ALL:ALL) ALL/' /etc/sudoers\n\
           elif ! grep -Eq '^[[:space:]]*%wheel[[:space:]]+ALL=\\(ALL:ALL\\)[[:space:]]+ALL[[:space:]]*$' /etc/sudoers; then\n\
             printf '\\n%%wheel ALL=(ALL:ALL) ALL\\n' >> /etc/sudoers\n\
           fi\n\
         fi\n\
         chpasswd\n",
        username = username
    )
}

fn ensure_wsl_conf_setting(raw: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines = Vec::new();
    let mut section_found = false;
    let mut in_target_section = false;
    let mut key_written_in_section = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(section_name) = parse_ini_section(trimmed) {
            if in_target_section && !key_written_in_section {
                lines.push(format!("{key}={value}"));
            }
            in_target_section = section_name.eq_ignore_ascii_case(section);
            if in_target_section {
                section_found = true;
                key_written_in_section = false;
            }
            lines.push(line.to_string());
            continue;
        }

        if in_target_section {
            if let Some((existing_key, _)) = trimmed.split_once('=') {
                if existing_key.trim().eq_ignore_ascii_case(key) {
                    if !key_written_in_section {
                        lines.push(format!("{key}={value}"));
                        key_written_in_section = true;
                    }
                    continue;
                }
            }
        }

        lines.push(line.to_string());
    }

    if section_found {
        if in_target_section && !key_written_in_section {
            lines.push(format!("{key}={value}"));
        }
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(format!("{key}={value}"));
    }

    let mut rendered = lines.join("\n");
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn parse_ini_section(line: &str) -> Option<&str> {
    line.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .map(str::trim)
        .filter(|section| !section.is_empty())
}

fn print_launch_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane MVP Launch Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_repair_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane Repair Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("  Outcome        Reapply Pane-managed Arch integration without opening mstsc.exe");
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_update_summary(plan: &LaunchPlan, stored_launch: &StoredLaunch) {
    println!("Pane Update Plan");
    for line in plan.summary_lines() {
        println!("  {line}");
    }
    println!("  Launch Stage   {}", stored_launch.stage.display_name());
    println!("  Dry Run        {}", yes_no(stored_launch.dry_run));
    println!("  Hypothetical   {}", yes_no(stored_launch.hypothetical));
    println!("  Outcome        Refresh Arch packages and reapply Pane-managed integration without opening mstsc.exe");
    println!("Steps");
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }
}

fn print_init_report(report: &InitReport) {
    println!("Pane Init");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!("  In Inventory   {}", yes_no(report.present_in_inventory));
    println!("Managed Environment");
    println!(
        "  Id             {}",
        report.managed_environment.environment_id
    );
    println!(
        "  Distro         {}",
        report.managed_environment.distro_name
    );
    println!(
        "  Family         {}",
        report.managed_environment.family.display_name()
    );
    println!(
        "  Ownership      {}",
        report.managed_environment.ownership.display_name()
    );
    if let Some(install_dir) = &report.managed_environment.install_dir {
        println!("  Install Dir    {}", install_dir.display());
    }
    if let Some(rootfs) = &report.managed_environment.source_rootfs {
        println!("  Rootfs Tar     {}", rootfs.display());
    }
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}

fn print_onboard_report(report: &OnboardReport) {
    println!("Pane Onboarding");
    println!("  Product Shape  {}", report.product_shape);
    println!("Managed Environment");
    println!(
        "  Distro         {}",
        report.managed_environment.distro_name
    );
    println!(
        "  Family         {}",
        report.managed_environment.family.display_name()
    );
    println!(
        "  Ownership      {}",
        report.managed_environment.ownership.display_name()
    );
    if let Some(install_dir) = &report.managed_environment.install_dir {
        println!("  Install Dir    {}", install_dir.display());
    }
    if let Some(rootfs) = &report.managed_environment.source_rootfs {
        println!("  Rootfs Tar     {}", rootfs.display());
    }
    println!("Setup User");
    println!("  Username       {}", report.setup_user.username);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!(
        "  Password       {}",
        yes_no(report.setup_user.password_updated)
    );
    println!(
        "  Default User   {}",
        yes_no(report.setup_user.default_user_configured)
    );
    println!(
        "  systemd=true   {}",
        yes_no(report.setup_user.systemd_configured)
    );
    println!(
        "  WSL Shutdown   {}",
        yes_no(report.setup_user.wsl_shutdown)
    );
    println!("Ready For Launch {}", yes_no(report.ready_for_launch));
    if let Some(readiness) = &report.launch_readiness {
        println!("Launch Readiness");
        println!("  Ready          {}", yes_no(readiness.ready));
        println!("  Supported MVP  {}", yes_no(readiness.supported_for_mvp));
        println!(
            "  Target Distro  {}",
            readiness.target_distro.as_deref().unwrap_or("unresolved")
        );
        println!("  Bootstrap      {}", yes_no(readiness.bootstrap_requested));
        println!("  Connect        {}", yes_no(readiness.connect_requested));
        for check in readiness
            .checks
            .iter()
            .filter(|check| check.status == CheckStatus::Fail)
        {
            println!("  Failure        [{}] {}", check.id, check.summary);
        }
    }
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}
fn print_setup_user_report(report: &SetupUserReport) {
    println!("Pane Setup User");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Distro         {}", report.distro);
    println!("  Username       {}", report.username);
    println!("  Dry Run        {}", yes_no(report.dry_run));
    println!("  Password       {}", yes_no(report.password_updated));
    println!(
        "  Default User   {}",
        yes_no(report.default_user_configured)
    );
    println!("  systemd=true   {}", yes_no(report.systemd_configured));
    println!("  WSL Shutdown   {}", yes_no(report.wsl_shutdown));
    if !report.notes.is_empty() {
        println!("Notes");
        for note in &report.notes {
            println!("  - {}", note);
        }
    }
}

fn print_status_report(report: &StatusReport) {
    println!("Pane Status");
    println!("  Platform       {}", report.platform);
    println!(
        "  WSL Available  {}",
        if report.wsl_available { "yes" } else { "no" }
    );
    if let Some(version) = &report.wsl_version_banner {
        println!("  WSL Version    {version}");
    }
    println!("  Known Distros  {}", report.known_distros.len());

    if let Some(managed_environment) = &report.managed_environment {
        println!("Managed Environment");
        println!("  Id             {}", managed_environment.environment_id);
        println!("  Distro         {}", managed_environment.distro_name);
        println!(
            "  Family         {}",
            managed_environment.family.display_name()
        );
        println!(
            "  Ownership      {}",
            managed_environment.ownership.display_name()
        );
        if let Some(install_dir) = &managed_environment.install_dir {
            println!("  Install Dir    {}", install_dir.display());
        }
        if let Some(rootfs) = &managed_environment.source_rootfs {
            println!("  Rootfs Tar     {}", rootfs.display());
        }
    }

    if let Some(distro) = &report.selected_distro {
        println!("Selected Distro");
        println!("  Name           {}", distro.distro.label());
        println!("  In Inventory   {}", yes_no(distro.present_in_inventory));
        println!("  Supported MVP  {}", yes_no(distro.supported_for_mvp));
        println!("  Family         {}", distro.distro.family.display_name());
        println!("  Checked Port   {}", distro.checked_port);
        if let Some(user) = &distro.distro.default_user {
            println!("  Default User   {user}");
        }
        if let Some(status) = distro.default_user_password_status {
            println!("  Password       {}", status.display_name());
        }
        if let Some(systemd) = distro.systemd_configured {
            println!("  systemd=true   {}", yes_no(systemd));
        }
        if let Some(installed) = distro.xrdp_installed {
            println!("  XRDP Installed {}", yes_no(installed));
        }
        if let Some(active) = distro.xrdp_service_active {
            println!("  XRDP Active    {}", yes_no(active));
        }
        if let Some(listening) = distro.xrdp_listening {
            println!("  XRDP Listening {}", yes_no(listening));
        }
        if let Some(reachable) = distro.localhost_reachable {
            println!("  localhost Port {}", yes_no(reachable));
        }
        if let Some(relay) = distro.pane_relay_available {
            println!("  Pane Relay     {}", yes_no(relay));
        }
        if let Some(transport) = distro.preferred_transport {
            println!("  Transport      {}", transport.display_name());
        }
        if let Some(xsession) = distro.xsession_present {
            println!("  .xsession      {}", yes_no(xsession));
        }
        if let Some(assets_ready) = distro.pane_session_assets_ready {
            println!("  Session Assets {}", yes_no(assets_ready));
        }
        if let Some(home_ready) = distro.user_home_ready {
            println!("  Home Ready     {}", yes_no(home_ready));
        }
    }

    if let Some(last_launch) = &report.last_launch {
        println!("Last Launch");
        println!("  Session        {}", last_launch.session_name);
        println!("  Distro         {}", last_launch.distro.label());
        println!(
            "  Desktop        {}",
            last_launch.desktop_environment.display_name()
        );
        println!("  Stage          {}", last_launch.stage.display_name());
        println!("  Dry Run        {}", yes_no(last_launch.dry_run));
        println!("  Hypothetical   {}", yes_no(last_launch.hypothetical));
        println!("  Port           {}", last_launch.port);
        if let Some(transport) = last_launch.transport {
            println!("  Transport      {}", transport.display_name());
        }
        if let Some(error) = &last_launch.last_error {
            println!("  Last Error     {error}");
        }
        if let Some(workspace) = &report.last_launch_workspace {
            println!("Workspace Assets");
            println!("  Root           {}", yes_no(workspace.root_exists));
            println!("  Shared Dir     {}", yes_no(workspace.shared_dir_exists));
            println!(
                "  Bootstrap      {}",
                yes_no(workspace.bootstrap_script_exists)
            );
            println!("  RDP Profile    {}", yes_no(workspace.rdp_profile_exists));
            println!(
                "  Bootstrap Log  {}",
                yes_no(workspace.bootstrap_log_exists)
            );
            println!(
                "  Transport Log  {}",
                yes_no(workspace.transport_log_exists)
            );
        }
    }
}

fn print_environment_catalog_report(report: &EnvironmentCatalogReport) {
    println!("Pane Environments");
    println!("  Product Shape  {}", report.product_shape);
    println!("  Strategy       {}", report.strategy);
    println!("Managed Environments");
    for environment in &report.environments {
        println!(
            "  {:<7} {}",
            environment.stage.display_name(),
            environment.display_name
        );
        println!("    Id           {}", environment.id);
        println!("    Family       {}", environment.family.display_name());
        println!("    Tier         {}", environment.tier.display_name());
        println!("    Launchable   {}", yes_no(environment.launchable_now));
        println!(
            "    Profile      {}",
            environment
                .starter_profile
                .as_deref()
                .unwrap_or("not assigned yet")
        );
        println!("    Summary      {}", environment.summary);
    }
    println!("Notes");
    for note in &report.notes {
        println!("  - {}", note);
    }
}

fn print_doctor_report(report: &DoctorReport) {
    println!("Pane Doctor");
    println!(
        "  Target Distro  {}",
        report.target_distro.as_deref().unwrap_or("not selected")
    );
    println!(
        "  Desktop        {}",
        report.desktop_environment.display_name()
    );
    println!("  Session        {}", report.session_name);
    println!("  Port           {}", report.port);
    println!("  Supported MVP  {}", yes_no(report.supported_for_mvp));
    println!("  Ready          {}", yes_no(report.ready));
    println!("Checks");
    for check in &report.checks {
        println!("  {:<4} {}", check.status.display_name(), check.summary);
        if let Some(remediation) = &check.remediation {
            println!("       fix: {remediation}");
        }
    }
}

fn log_transport_event(path: Option<&Path>, message: &str) {
    let Some(path) = path else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {}", current_epoch_seconds(), message);
    }
}

fn relay_connection(
    distro: &str,
    target_port: u16,
    stream: TcpStream,
    log_file: Option<&Path>,
) -> AppResult<()> {
    if let Some(address) =
        wsl::distro_ipv4_address(distro).map(|ip| SocketAddr::from((ip, target_port)))
    {
        log_transport_event(
            log_file,
            &format!("relay targeting {address} via direct WSL TCP"),
        );
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(remote) => {
                relay_tcp_streams(stream, remote)?;
                return Ok(());
            }
            Err(error) => {
                log_transport_event(
                    log_file,
                    &format!(
                        "direct WSL TCP relay path to {address} failed: {error}; falling back to wsl.exe stdio tunnel"
                    ),
                );
            }
        }
    }

    relay_connection_via_stdio(distro, target_port, stream, log_file)
}

fn relay_tcp_streams(stream: TcpStream, remote: TcpStream) -> AppResult<()> {
    let mut upstream_reader = stream.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the local TCP stream: {error}"
        ))
    })?;
    let mut downstream_writer = stream;
    let mut remote_reader = remote.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the WSL TCP stream: {error}"
        ))
    })?;
    let mut remote_writer = remote;

    let _ = downstream_writer.set_nodelay(true);
    let _ = remote_writer.set_nodelay(true);

    let upstream = thread::spawn(move || {
        let result = std::io::copy(&mut upstream_reader, &mut remote_writer);
        let _ = remote_writer.flush();
        let _ = remote_writer.shutdown(Shutdown::Write);
        result
    });
    let downstream = thread::spawn(move || {
        let result = std::io::copy(&mut remote_reader, &mut downstream_writer);
        let _ = downstream_writer.shutdown(Shutdown::Write);
        result
    });

    let _ = upstream.join();
    let _ = downstream.join();
    Ok(())
}

fn relay_connection_via_stdio(
    distro: &str,
    target_port: u16,
    stream: TcpStream,
    log_file: Option<&Path>,
) -> AppResult<()> {
    let relay_command = format!("exec nc 127.0.0.1 {target_port}");
    let mut child = Command::new("wsl.exe");
    child
        .arg("-d")
        .arg(distro)
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg(&relay_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = child.spawn().map_err(|error| {
        AppError::message(format!(
            "failed to start the Pane relay tunnel into {}:{}: {error}",
            distro, target_port
        ))
    })?;
    let mut child_stdin = child.stdin.take().ok_or_else(|| {
        AppError::message("the Pane relay could not capture stdin for the WSL tunnel")
    })?;
    let mut child_stdout = child.stdout.take().ok_or_else(|| {
        AppError::message("the Pane relay could not capture stdout for the WSL tunnel")
    })?;

    let mut upstream_reader = stream.try_clone().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not clone the local TCP stream: {error}"
        ))
    })?;
    let mut downstream_writer = stream;
    let _ = downstream_writer.set_nodelay(true);

    let upstream = thread::spawn(move || {
        let result = std::io::copy(&mut upstream_reader, &mut child_stdin);
        let _ = child_stdin.flush();
        drop(child_stdin);
        result
    });
    let downstream = thread::spawn(move || {
        let result = std::io::copy(&mut child_stdout, &mut downstream_writer);
        let _ = downstream_writer.shutdown(Shutdown::Write);
        result
    });

    let _ = upstream.join();
    let _ = downstream.join();

    let status = child.wait().map_err(|error| {
        AppError::message(format!(
            "the Pane relay could not wait for the WSL tunnel process: {error}"
        ))
    })?;
    if status.success() {
        Ok(())
    } else {
        let exit = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        log_transport_event(
            log_file,
            &format!(
                "relay tunnel exited with {exit} for {}:{}",
                distro, target_port
            ),
        );
        Err(AppError::message(format!(
            "the Pane relay tunnel into {}:{} exited with {}",
            distro, target_port, exit
        )))
    }
}

fn ensure_transport_ready(
    distro: &str,
    port: u16,
    workspace: &WorkspacePaths,
) -> AppResult<PreparedTransport> {
    log_transport_event(
        Some(&workspace.transport_log),
        &format!("checking transport for {} on localhost:{}", distro, port),
    );

    let service_active = wsl::distro_service_active(distro, "xrdp") == Some(true);
    let listening = wsl::distro_port_listening(distro, port) == Some(true);
    if !service_active || !listening {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "XRDP inside {} is not ready for transport: active={}, listening={}",
                distro, service_active, listening
            ),
        );
        return Err(AppError::message(format!(
            "XRDP is not ready inside {} on port {}. Run pane logs or pane repair before reconnecting.",
            distro, port
        )));
    }

    if local_port_reachable(port) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!("using direct-localhost transport on localhost:{}", port),
        );
        return Ok(PreparedTransport::direct_localhost());
    }

    if let Some(address) = wsl::distro_ipv4_address(distro).map(|ip| SocketAddr::from((ip, port))) {
        if socket_reachable(address) {
            log_transport_event(
                Some(&workspace.transport_log),
                &format!("using direct-wsl-ip transport on {address}"),
            );
            return Ok(PreparedTransport::direct_wsl_ip(address.ip().to_string()));
        }

        log_transport_event(
            Some(&workspace.transport_log),
            &format!("Windows could not reach {address} directly; falling back to pane-relay"),
        );
    }

    if !(wsl::distro_ipv4_address(distro).is_some() || wsl::distro_command_exists(distro, "nc")) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "pane-relay is unavailable because Pane could not resolve a WSL IP address and nc is missing inside {}",
                distro
            ),
        );
        return Err(AppError::message(format!(
            "Windows could not reach localhost:{}, the distro IP directly, or a Pane relay path into {}. Run pane repair or pane launch to restore the relay helper and WSL network state.",
            port, distro
        )));
    }

    log_transport_event(
        Some(&workspace.transport_log),
        &format!(
            "localhost:{} and the distro IP are not reachable from Windows; starting pane-relay",
            port
        ),
    );
    spawn_pane_relay(distro, port, workspace)?;
    let ready_file = relay_ready_path(workspace);
    if wait_for_path(&ready_file, Duration::from_secs(5)) {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!("pane-relay is serving localhost:{}", port),
        );
        Ok(PreparedTransport::pane_relay())
    } else {
        log_transport_event(
            Some(&workspace.transport_log),
            &format!(
                "pane-relay did not publish readiness for localhost:{}",
                port
            ),
        );
        Err(AppError::message(format!(
            "Windows could not reach localhost:{}, the distro IP directly, and the Pane relay did not come up in time. Review {} or run pane logs.",
            port,
            workspace.transport_log.display()
        )))
    }
}
fn relay_ready_path(workspace: &WorkspacePaths) -> PathBuf {
    workspace.root.join("transport.ready")
}

fn spawn_pane_relay(distro: &str, port: u16, workspace: &WorkspacePaths) -> AppResult<()> {
    let executable = std::env::current_exe().map_err(|error| {
        AppError::message(format!(
            "failed to locate the Pane executable for pane-relay startup: {error}"
        ))
    })?;
    let ready_file = relay_ready_path(workspace);
    match fs::remove_file(&ready_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AppError::message(format!(
                "failed to clear the stale Pane relay readiness file at {}: {error}",
                ready_file.display()
            )));
        }
    }

    let mut command = Command::new(executable);
    command
        .arg("relay")
        .arg("--distro")
        .arg(distro)
        .arg("--listen-port")
        .arg(port.to_string())
        .arg("--target-port")
        .arg(port.to_string())
        .arg("--startup-timeout-seconds")
        .arg("90")
        .arg("--log-file")
        .arg(&workspace.transport_log)
        .arg("--ready-file")
        .arg(&ready_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        command.creation_flags(0x0000_0008 | 0x0800_0000);
    }

    command.spawn().map_err(|error| {
        AppError::message(format!(
            "failed to start the Pane relay for {} on localhost:{}: {error}",
            distro, port
        ))
    })?;

    Ok(())
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }

    false
}

fn socket_reachable(address: SocketAddr) -> bool {
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

fn local_port_reachable(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    socket_reachable(address)
}

fn mstsc_available() -> bool {
    if !cfg!(windows) {
        return false;
    }

    Command::new("where.exe")
        .arg("mstsc.exe")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn wait_for_runtime_ready(distro: &str, port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let service_active = wsl::distro_service_active(distro, "xrdp") == Some(true);
        let listening = wsl::distro_port_listening(distro, port) == Some(true);
        if service_active && listening {
            return true;
        }

        thread::sleep(Duration::from_millis(500));
    }

    false
}

fn write_bootstrap_log(
    path: &Path,
    plan: &LaunchPlan,
    command: &str,
    transcript: &wsl::CommandTranscript,
) -> AppResult<()> {
    let payload = format!(
        "Pane Bootstrap Transcript\n\nSession: {}\nDistro: {}\nDesktop: {}\nPort: {}\nCommand: {}\nSuccess: {}\n\n--- STDOUT ---\n{}\n\n--- STDERR ---\n{}\n",
        plan.session_name,
        plan.distro.name,
        plan.desktop_environment.display_name(),
        plan.port,
        command,
        yes_no(transcript.success),
        transcript.stdout.trim_end(),
        transcript.stderr.trim_end(),
    );

    fs::write(path, payload)?;
    Ok(())
}

fn format_doctor_blockers(command: &str, report: &DoctorReport) -> String {
    let mut lines = vec![format!("{command} is blocked by the following checks:")];
    for check in report
        .checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
    {
        lines.push(format!("- {}", check.summary));
        if let Some(remediation) = &check.remediation {
            lines.push(format!("  fix: {remediation}"));
        }
    }
    lines.push("Run `pane doctor` for the full report.".to_string());
    lines.join("\n")
}

fn push_check(
    checks: &mut Vec<DoctorCheck>,
    status: CheckStatus,
    id: impl Into<String>,
    summary: impl Into<String>,
    remediation: Option<String>,
) {
    checks.push(DoctorCheck {
        id: id.into(),
        status,
        summary: summary.into(),
        remediation,
    });
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{InitArgs, ResetArgs},
        model::{DesktopEnvironment, DistroFamily, DistroRecord},
        plan::{LaunchPlan, WorkspacePaths},
        state::{
            LaunchStage, LaunchTransport, ManagedEnvironmentOwnership, ManagedEnvironmentState,
            StoredLaunch,
        },
    };

    use super::{
        build_bundle_doctor_request, build_distro_health, build_environment_catalog_report,
        build_steps, build_update_steps, ensure_wsl_conf_setting, format_doctor_blockers,
        initialize_managed_arch_environment, inspect_workspace, inventory_contains_distro,
        last_launch_transport, preferred_transport, resolve_bundle_output_path,
        resolve_init_source, resolve_managed_environment_for_reset, resolve_saved_launch,
        resolve_session_context, resolve_status_distro, status_port_for, validate_setup_password,
        validate_setup_username, windows_transport_check, CheckStatus, DistroHealth, DoctorCheck,
        DoctorReport, InitSource, WorkspaceHealth, WslInventory,
    };

    #[test]
    fn build_steps_omits_rdp_handoff_when_connect_is_disabled() {
        let steps = build_steps(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
            true,
            false,
        );

        assert!(steps
            .iter()
            .any(|step| step.contains("Run the bootstrap script")));
        assert!(!steps.iter().any(|step| step.contains("Launch mstsc.exe")));
    }

    #[test]
    fn build_update_steps_refresh_packages_and_omit_rdp_handoff() {
        let steps = build_update_steps(
            &DistroRecord {
                name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            },
            DesktopEnvironment::Xfce,
            3390,
        );

        assert!(steps
            .iter()
            .any(|step| step.contains("Refresh Arch packages")));
        assert!(!steps.iter().any(|step| step.contains("Launch mstsc.exe")));
    }

    #[test]
    fn preferred_transport_uses_direct_wsl_ip_when_localhost_is_unreachable() {
        assert_eq!(
            preferred_transport(Some(true), Some(false), Some(true), Some(true)),
            Some(LaunchTransport::DirectWslIp)
        );
    }

    #[test]
    fn preferred_transport_uses_pane_relay_when_only_relay_is_available() {
        assert_eq!(
            preferred_transport(Some(true), Some(false), Some(false), Some(true)),
            Some(LaunchTransport::PaneRelay)
        );
    }

    #[test]
    fn last_launch_transport_prefers_recorded_rdp_launch() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(StoredLaunch {
                session_name: "pane".to_string(),
                distro: DistroRecord {
                    name: "pane-arch".to_string(),
                    ..DistroRecord::default()
                },
                desktop_environment: DesktopEnvironment::Xfce,
                port: 3390,
                workspace: WorkspacePaths {
                    root: "root".into(),
                    bootstrap_script: "bootstrap".into(),
                    rdp_profile: "rdp".into(),
                    bootstrap_log: "bootstrap.log".into(),
                    transport_log: "transport.log".into(),
                },
                stage: LaunchStage::RdpLaunched,
                dry_run: false,
                hypothetical: false,
                bootstrap_requested: true,
                connect_requested: true,
                transport: Some(LaunchTransport::PaneRelay),
                generated_at_epoch_seconds: 1,
                bootstrapped_at_epoch_seconds: Some(2),
                rdp_launched_at_epoch_seconds: Some(3),
                last_error: None,
            }),
        };

        assert_eq!(
            last_launch_transport("pane-arch", 3390, Some(&state)),
            Some(LaunchTransport::PaneRelay)
        );
        assert_eq!(last_launch_transport("pane-arch", 4489, Some(&state)), None);
    }

    #[test]
    fn windows_transport_check_passes_when_pane_relay_can_bridge() {
        let health = DistroHealth {
            distro: DistroRecord::default(),
            supported_for_mvp: true,
            present_in_inventory: true,
            checked_port: 3390,
            systemd_configured: Some(true),
            xrdp_installed: Some(true),
            xrdp_service_active: Some(true),
            xrdp_listening: Some(true),
            localhost_reachable: Some(false),
            pane_relay_available: Some(true),
            preferred_transport: Some(LaunchTransport::PaneRelay),
            xsession_present: Some(true),
            pane_session_assets_ready: Some(true),
            user_home_ready: Some(true),
            default_user_password_status: None,
        };

        let check = windows_transport_check(3390, &health).unwrap();
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(check.summary.contains("pane-relay"));
    }

    #[test]
    fn resolve_saved_launch_rejects_mismatched_session() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: true,
            hypothetical: true,
            bootstrap_requested: true,
            connect_requested: false,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let error = resolve_saved_launch(Some("other"), Some(&state)).unwrap_err();
        assert!(error.to_string().contains("only tracks one active session"));
    }

    #[test]
    fn status_prefers_saved_port_for_matching_distro() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: true,
            hypothetical: true,
            bootstrap_requested: true,
            connect_requested: false,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        assert_eq!(status_port_for("archlinux", Some(&state)), 4489);
        assert_eq!(status_port_for("ubuntu", Some(&state)), 3390);
    }

    #[test]
    fn resolve_status_distro_prefers_managed_environment() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                install_dir: None,
                source_rootfs: None,
                created_at_epoch_seconds: 1,
            }),
            last_launch: Some(StoredLaunch {
                session_name: "pane".to_string(),
                distro: DistroRecord {
                    name: "archlinux".to_string(),
                    ..DistroRecord::default()
                },
                desktop_environment: DesktopEnvironment::Xfce,
                port: 4489,
                workspace: WorkspacePaths {
                    root: "root".into(),
                    bootstrap_script: "bootstrap".into(),
                    rdp_profile: "rdp".into(),
                    bootstrap_log: "bootstrap.log".into(),
                    transport_log: "transport.log".into(),
                },
                stage: LaunchStage::Planned,
                dry_run: false,
                hypothetical: false,
                bootstrap_requested: true,
                connect_requested: true,
                transport: None,
                generated_at_epoch_seconds: 1,
                bootstrapped_at_epoch_seconds: None,
                rdp_launched_at_epoch_seconds: None,
                last_error: None,
            }),
        };

        let resolved = resolve_status_distro(None, &WslInventory::default(), Some(&state)).unwrap();
        assert_eq!(resolved.as_deref(), Some("pane-arch"));
    }

    #[test]
    fn resolve_init_source_defaults_to_online_provisioning() {
        let args = InitArgs {
            distro_name: "pane-arch".to_string(),
            existing_distro: None,
            rootfs_tar: None,
            install_dir: None,
            dry_run: true,
            json: false,
        };

        match resolve_init_source(&args, &WslInventory::default()).unwrap() {
            InitSource::InstallOnline {
                distro_name,
                install_dir,
            } => {
                assert_eq!(distro_name, "pane-arch");
                assert!(install_dir.ends_with(std::path::Path::new("distros").join("pane-arch")));
            }
            _ => panic!("expected Pane-owned online provisioning source"),
        }
    }

    #[test]
    fn init_preserves_existing_managed_environment_ownership_for_same_distro() {
        let args = InitArgs {
            distro_name: "pane-arch".to_string(),
            existing_distro: None,
            rootfs_tar: None,
            install_dir: None,
            dry_run: true,
            json: false,
        };
        let inventory = WslInventory {
            available: true,
            distros: vec![DistroRecord {
                name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ..DistroRecord::default()
            }],
            ..WslInventory::default()
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::InstalledOnline,
                install_dir: Some("D:/Pane/distros/pane-arch".into()),
                source_rootfs: None,
                created_at_epoch_seconds: 42,
            }),
            last_launch: None,
        };

        let report = initialize_managed_arch_environment(&args, &inventory, Some(&state)).unwrap();
        assert_eq!(
            report.managed_environment.ownership,
            ManagedEnvironmentOwnership::InstalledOnline
        );
        assert_eq!(report.managed_environment.created_at_epoch_seconds, 42);
        assert_eq!(
            report.managed_environment.install_dir,
            Some(std::path::PathBuf::from("D:/Pane/distros/pane-arch"))
        );
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("Preserved Pane ownership metadata")));
    }
    #[test]
    fn factory_reset_rejects_adopted_managed_distro() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "archlinux".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::AdoptedExisting,
                install_dir: None,
                source_rootfs: None,
                created_at_epoch_seconds: 1,
            }),
            last_launch: None,
        };
        let args = ResetArgs {
            session_name: None,
            distro: None,
            purge_wsl: false,
            release_managed_environment: false,
            factory_reset: true,
            dry_run: false,
        };

        let error = resolve_managed_environment_for_reset(&args, Some(&state)).unwrap_err();
        assert!(error
            .to_string()
            .contains("Factory reset is only supported for Pane-provisioned distros"));
    }

    #[test]
    fn build_distro_health_prefers_managed_environment_family_when_inventory_is_missing() {
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::ImportedRootfs,
                install_dir: Some("D:/Pane/distros/pane-arch".into()),
                source_rootfs: Some("D:/Downloads/archlinux.tar".into()),
                created_at_epoch_seconds: 1,
            }),
            last_launch: None,
        };

        let health =
            build_distro_health("pane-arch", &WslInventory::default(), Some(&state), None).unwrap();
        assert_eq!(health.distro.family, DistroFamily::Arch);
        assert!(health.supported_for_mvp);
        assert!(!health.present_in_inventory);
    }

    #[test]
    fn inspects_workspace_asset_presence() {
        let temp = std::env::temp_dir().join("pane-workspace-health-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let bootstrap = temp.join("pane-bootstrap.sh");
        let rdp = temp.join("pane.rdp");
        let log = temp.join("bootstrap.log");
        let shared = temp.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(&bootstrap, "echo ok").unwrap();
        std::fs::write(&log, "ok").unwrap();

        let health = inspect_workspace(&WorkspacePaths {
            root: temp.clone(),
            bootstrap_script: bootstrap,
            rdp_profile: rdp,
            bootstrap_log: log.clone(),
            transport_log: log.with_file_name("transport.log"),
        });

        assert!(health.root_exists);
        assert!(health.shared_dir_exists);
        assert!(health.bootstrap_script_exists);
        assert!(!health.rdp_profile_exists);
        assert!(health.bootstrap_log_exists);

        let _ = std::fs::remove_dir_all(temp);
    }

    #[test]
    fn planned_launch_summary_state_starts_at_planned() {
        let plan = LaunchPlan {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            connect_after_bootstrap: false,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
            },
            bootstrap_script: "script".to_string(),
            rdp_profile: "profile".to_string(),
            steps: vec!["one".to_string()],
        };

        let launch = StoredLaunch::planned_from_plan(&plan, false, false, true, false);
        assert_eq!(launch.stage, LaunchStage::Planned);
        assert!(launch.bootstrap_requested);
        assert!(!launch.connect_requested);
    }

    #[test]
    fn blocker_formatting_includes_remediation() {
        let report = DoctorReport {
            target_distro: Some("archlinux".to_string()),
            session_name: "pane".to_string(),
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            bootstrap_requested: true,
            connect_requested: true,
            supported_for_mvp: false,
            ready: false,
            selected_distro: Some(DistroHealth {
                distro: DistroRecord::default(),
                supported_for_mvp: false,
                present_in_inventory: false,
                checked_port: 3390,
                systemd_configured: None,
                xrdp_installed: None,
                xrdp_service_active: None,
                xrdp_listening: None,
                localhost_reachable: None,
                pane_relay_available: None,
                preferred_transport: None,
                xsession_present: None,
                pane_session_assets_ready: None,
                user_home_ready: None,
                default_user_password_status: None,
            }),
            workspace: WorkspaceHealth {
                root_exists: true,
                shared_dir_exists: true,
                bootstrap_script_exists: true,
                rdp_profile_exists: true,
                bootstrap_log_exists: false,
                transport_log_exists: false,
            },
            checks: vec![DoctorCheck {
                id: "xrdp-active".to_string(),
                status: CheckStatus::Fail,
                summary: "XRDP is not active.".to_string(),
                remediation: Some("Run pane launch again.".to_string()),
            }],
        };

        let rendered = format_doctor_blockers("pane connect", &report);
        assert!(rendered.contains("XRDP is not active."));
        assert!(rendered.contains("Run pane launch again."));
    }

    #[test]
    fn resolve_session_context_prefers_saved_launch_workspace() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
            },
            stage: LaunchStage::Planned,
            dry_run: false,
            hypothetical: false,
            bootstrap_requested: true,
            connect_requested: true,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let (session_name, saved_launch, workspace) = resolve_session_context(None, Some(&state));
        assert_eq!(session_name, "pane");
        assert_eq!(saved_launch.unwrap().session_name, "pane");
        assert_eq!(workspace.root, std::path::PathBuf::from("root"));
    }

    #[test]
    fn bundle_output_path_uses_directory_targets_and_zip_extension() {
        let base = std::env::temp_dir().join(format!("pane-bundle-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let from_dir = resolve_bundle_output_path(Some(base.as_path()), "pane");
        assert_eq!(from_dir.parent(), Some(base.as_path()));
        assert_eq!(
            from_dir.extension().and_then(|value| value.to_str()),
            Some("zip")
        );

        let stem = base.join("support-bundle");
        let from_stem = resolve_bundle_output_path(Some(stem.as_path()), "pane");
        assert_eq!(
            from_stem.extension().and_then(|value| value.to_str()),
            Some("zip")
        );
        assert!(from_stem.ends_with("support-bundle.zip"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn bundle_doctor_request_switches_to_reconnect_after_bootstrap() {
        let launch = StoredLaunch {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 4489,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
            },
            stage: LaunchStage::Bootstrapped,
            dry_run: false,
            hypothetical: false,
            bootstrap_requested: true,
            connect_requested: true,
            transport: None,
            generated_at_epoch_seconds: 1,
            bootstrapped_at_epoch_seconds: Some(2),
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        };
        let state = crate::state::PaneState {
            updated_at_epoch_seconds: 2,
            managed_environment: None,
            last_launch: Some(launch),
        };

        let request = build_bundle_doctor_request(
            "pane",
            None,
            state.last_launch.as_ref(),
            Some(&state),
            &WslInventory::default(),
        )
        .unwrap();

        assert_eq!(request.distro.as_deref(), Some("archlinux"));
        assert_eq!(request.port, 4489);
        assert!(!request.bootstrap_requested);
        assert!(request.connect_requested);
    }

    #[test]
    fn environment_catalog_report_reflects_first_three_managed_environments() {
        let report = build_environment_catalog_report();
        assert_eq!(report.environments.len(), 3);
        assert_eq!(report.environments[0].id, "arch");
        assert!(report.environments[0].launchable_now);
        assert_eq!(report.environments[1].id, "ubuntu-lts");
        assert_eq!(report.environments[2].id, "debian");
        assert!(report.notes.iter().any(|note| note.contains("Kali")));
    }

    #[test]
    fn inventory_contains_distro_is_case_insensitive() {
        let inventory = WslInventory {
            available: true,
            distros: vec![DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            }],
            ..WslInventory::default()
        };

        assert!(inventory_contains_distro(&inventory, "ARCHLINUX"));
    }

    #[test]
    fn ensure_wsl_conf_setting_replaces_existing_key_and_appends_missing_sections() {
        let raw = "[boot]\nsystemd=false\n[user]\ndefault=root\n";
        let updated = ensure_wsl_conf_setting(
            &ensure_wsl_conf_setting(raw, "boot", "systemd", "true"),
            "user",
            "default",
            "archuser",
        );
        assert!(updated.contains("[boot]\nsystemd=true\n"));
        assert!(updated.contains("[user]\ndefault=archuser\n"));

        let appended = ensure_wsl_conf_setting(
            "[network]\ngenerateResolvConf=false\n",
            "boot",
            "systemd",
            "true",
        );
        assert!(appended.contains("[network]\ngenerateResolvConf=false\n\n[boot]\nsystemd=true\n"));
    }

    #[test]
    fn setup_user_validation_rejects_unsafe_values() {
        assert!(validate_setup_username("root").is_err());
        assert!(validate_setup_username("ArchUser").is_err());
        assert!(validate_setup_password("bad:password").is_err());
        assert!(validate_setup_password("line\nbreak").is_err());
        assert!(validate_setup_username("arch-user").is_ok());
        assert!(validate_setup_password("safe-password").is_ok());
    }
}
