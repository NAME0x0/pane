use std::{fs, path::PathBuf, time::UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    model::{DesktopEnvironment, DistroFamily, DistroRecord},
    plan::{app_root, LaunchPlan, WorkspacePaths},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneState {
    pub updated_at_epoch_seconds: u64,
    pub managed_environment: Option<ManagedEnvironmentState>,
    pub last_launch: Option<StoredLaunch>,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedEnvironmentOwnership {
    ImportedRootfs,
    InstalledOnline,
    #[default]
    AdoptedExisting,
}

impl ManagedEnvironmentOwnership {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::ImportedRootfs => "imported-rootfs",
            Self::InstalledOnline => "installed-online",
            Self::AdoptedExisting => "adopted-existing",
        }
    }

    pub fn can_factory_reset(self) -> bool {
        matches!(self, Self::ImportedRootfs | Self::InstalledOnline)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagedEnvironmentState {
    pub environment_id: String,
    pub distro_name: String,
    pub family: DistroFamily,
    pub ownership: ManagedEnvironmentOwnership,
    pub install_dir: Option<PathBuf>,
    pub source_rootfs: Option<PathBuf>,
    pub created_at_epoch_seconds: u64,
}

impl ManagedEnvironmentState {
    pub fn is_arch(&self) -> bool {
        self.family == DistroFamily::Arch
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchStage {
    #[default]
    Planned,
    Bootstrapped,
    RdpLaunched,
    Failed,
}

impl LaunchStage {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Bootstrapped => "bootstrapped",
            Self::RdpLaunched => "rdp-launched",
            Self::Failed => "failed",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchTransport {
    #[default]
    DirectLocalhost,
    DirectWslIp,
    PaneRelay,
}

impl LaunchTransport {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::DirectLocalhost => "direct-localhost",
            Self::DirectWslIp => "direct-wsl-ip",
            Self::PaneRelay => "pane-relay",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredLaunch {
    pub session_name: String,
    pub distro: DistroRecord,
    pub desktop_environment: DesktopEnvironment,
    pub port: u16,
    pub workspace: WorkspacePaths,
    pub stage: LaunchStage,
    pub dry_run: bool,
    pub hypothetical: bool,
    pub bootstrap_requested: bool,
    pub connect_requested: bool,
    pub transport: Option<LaunchTransport>,
    pub generated_at_epoch_seconds: u64,
    pub bootstrapped_at_epoch_seconds: Option<u64>,
    pub rdp_launched_at_epoch_seconds: Option<u64>,
    pub last_error: Option<String>,
}

impl StoredLaunch {
    pub fn planned_from_plan(
        plan: &LaunchPlan,
        dry_run: bool,
        hypothetical: bool,
        bootstrap_requested: bool,
        connect_requested: bool,
    ) -> Self {
        Self {
            session_name: plan.session_name.clone(),
            distro: plan.distro.clone(),
            desktop_environment: plan.desktop_environment,
            port: plan.port,
            workspace: plan.workspace.clone(),
            stage: LaunchStage::Planned,
            dry_run,
            hypothetical,
            bootstrap_requested,
            connect_requested,
            transport: None,
            generated_at_epoch_seconds: now_epoch_seconds(),
            bootstrapped_at_epoch_seconds: None,
            rdp_launched_at_epoch_seconds: None,
            last_error: None,
        }
    }

    pub fn mark_bootstrapped(&mut self) {
        self.stage = LaunchStage::Bootstrapped;
        self.bootstrapped_at_epoch_seconds = Some(now_epoch_seconds());
        self.last_error = None;
    }

    pub fn mark_rdp_launched(&mut self, transport: LaunchTransport) {
        self.stage = LaunchStage::RdpLaunched;
        self.transport = Some(transport);
        self.rdp_launched_at_epoch_seconds = Some(now_epoch_seconds());
        self.last_error = None;
    }

    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.stage = LaunchStage::Failed;
        self.last_error = Some(error.into());
    }
}

pub fn state_path() -> PathBuf {
    app_root().join("state.json")
}

pub fn load_state() -> AppResult<Option<PaneState>> {
    let path = state_path();
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    let state = serde_json::from_str(&raw)?;
    Ok(Some(state))
}

pub fn save_state_record(last_launch: StoredLaunch) -> AppResult<()> {
    let mut state = load_state()?.unwrap_or_default();
    state.updated_at_epoch_seconds = now_epoch_seconds();
    state.last_launch = Some(last_launch);
    save_state(&state)
}

pub fn save_managed_environment(managed_environment: ManagedEnvironmentState) -> AppResult<()> {
    let mut state = load_state()?.unwrap_or_default();
    state.updated_at_epoch_seconds = now_epoch_seconds();
    state.managed_environment = Some(managed_environment);
    save_state(&state)
}

pub fn clear_managed_environment(distro_name: Option<&str>) -> AppResult<()> {
    let Some(mut state) = load_state()? else {
        return Ok(());
    };

    state.updated_at_epoch_seconds = now_epoch_seconds();
    state.managed_environment = None;
    if let Some(name) = distro_name {
        if state
            .last_launch
            .as_ref()
            .is_some_and(|launch| launch.distro.name.eq_ignore_ascii_case(name))
        {
            state.last_launch = None;
        }
    }

    if state.managed_environment.is_none() && state.last_launch.is_none() {
        remove_state_file()?;
    } else {
        save_state(&state)?;
    }

    Ok(())
}

pub fn save_state(state: &PaneState) -> AppResult<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let payload = serde_json::to_string_pretty(state)?;
    fs::write(path, payload)?;
    Ok(())
}

pub fn clear_state() -> AppResult<()> {
    let Some(mut state) = load_state()? else {
        return Ok(());
    };

    state.updated_at_epoch_seconds = now_epoch_seconds();
    state.last_launch = None;

    if state.managed_environment.is_some() {
        save_state(&state)?;
    } else {
        remove_state_file()?;
    }

    Ok(())
}

fn remove_state_file() -> AppResult<()> {
    let path = state_path();
    if path.exists() {
        fs::remove_file(path)?;
    }

    Ok(())
}

fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{DesktopEnvironment, DistroFamily, DistroRecord},
        plan::{LaunchPlan, WorkspacePaths},
    };

    use super::{
        LaunchStage, LaunchTransport, ManagedEnvironmentOwnership, ManagedEnvironmentState,
        PaneState, StoredLaunch,
    };

    #[test]
    fn launch_stage_transitions_track_progress() {
        let plan = LaunchPlan {
            session_name: "pane".to_string(),
            distro: DistroRecord {
                name: "archlinux".to_string(),
                ..DistroRecord::default()
            },
            desktop_environment: DesktopEnvironment::Xfce,
            port: 3390,
            connect_after_bootstrap: true,
            workspace: WorkspacePaths {
                root: "root".into(),
                bootstrap_script: "bootstrap".into(),
                rdp_profile: "rdp".into(),
                bootstrap_log: "bootstrap.log".into(),
                transport_log: "transport.log".into(),
                shared_dir: "shared".into(),
            },
            bootstrap_script: "script".to_string(),
            rdp_profile: "profile".to_string(),
            steps: vec!["one".to_string()],
        };

        let mut launch = StoredLaunch::planned_from_plan(&plan, false, false, true, true);
        assert_eq!(launch.stage, LaunchStage::Planned);

        launch.mark_bootstrapped();
        assert_eq!(launch.stage, LaunchStage::Bootstrapped);
        assert!(launch.bootstrapped_at_epoch_seconds.is_some());

        launch.mark_rdp_launched(LaunchTransport::PaneRelay);
        assert_eq!(launch.stage, LaunchStage::RdpLaunched);
        assert!(launch.rdp_launched_at_epoch_seconds.is_some());
        assert_eq!(launch.transport, Some(LaunchTransport::PaneRelay));

        launch.mark_failed("connection failed");
        assert_eq!(launch.stage, LaunchStage::Failed);
        assert_eq!(launch.last_error.as_deref(), Some("connection failed"));
    }

    #[test]
    fn deserializes_legacy_launch_state_without_stage_fields() {
        let legacy = r#"{
  "updated_at_epoch_seconds": 1,
  "last_launch": {
    "session_name": "pane",
    "distro": { "name": "archlinux", "family": "arch", "pretty_name": null, "state": null, "version": null, "default": false, "default_user": null },
    "desktop_environment": "xfce",
    "port": 3390,
    "workspace": { "root": "root", "bootstrap_script": "bootstrap", "rdp_profile": "rdp" }
  }
}"#;

        let parsed: PaneState = serde_json::from_str(legacy).unwrap();
        let launch = parsed.last_launch.unwrap();

        assert_eq!(launch.stage, LaunchStage::Planned);
        assert!(!launch.dry_run);
        assert!(!launch.hypothetical);
        assert!(parsed.managed_environment.is_none());
    }

    #[test]
    fn serializes_managed_environment_state() {
        let state = PaneState {
            updated_at_epoch_seconds: 1,
            managed_environment: Some(ManagedEnvironmentState {
                environment_id: "arch".to_string(),
                distro_name: "pane-arch".to_string(),
                family: DistroFamily::Arch,
                ownership: ManagedEnvironmentOwnership::ImportedRootfs,
                install_dir: Some("D:/Pane/distros/pane-arch".into()),
                source_rootfs: Some("D:/Downloads/archlinux.tar".into()),
                created_at_epoch_seconds: 2,
            }),
            last_launch: None,
        };

        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("pane-arch"));
        assert!(json.contains("imported-rootfs"));
    }
}
