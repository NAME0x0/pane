use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    model::{DesktopEnvironment, DistroRecord, SharedStorageMode},
};

pub const DEFAULT_RUNTIME_CAPACITY_GIB: u64 = 8;
pub const MINIMUM_RUNTIME_CAPACITY_GIB: u64 = 8;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePaths {
    pub root: PathBuf,
    pub bootstrap_script: PathBuf,
    pub rdp_profile: PathBuf,
    pub bootstrap_log: PathBuf,
    pub transport_log: PathBuf,
    pub shared_dir: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub downloads: PathBuf,
    pub images: PathBuf,
    pub disks: PathBuf,
    pub snapshots: PathBuf,
    pub state: PathBuf,
    pub engines: PathBuf,
    pub logs: PathBuf,
    pub base_os_image: PathBuf,
    pub serial_boot_image: PathBuf,
    pub boot_loader_image: PathBuf,
    pub kernel_image: PathBuf,
    pub initramfs_image: PathBuf,
    pub user_disk: PathBuf,
    pub base_os_metadata: PathBuf,
    pub serial_boot_metadata: PathBuf,
    pub boot_loader_metadata: PathBuf,
    pub kernel_boot_metadata: PathBuf,
    pub user_disk_metadata: PathBuf,
    pub runtime_config: PathBuf,
    pub native_manifest: PathBuf,
    pub kernel_boot_layout: PathBuf,
    pub manifest: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub session_name: String,
    pub distro: DistroRecord,
    pub desktop_environment: DesktopEnvironment,
    pub port: u16,
    pub connect_after_bootstrap: bool,
    pub workspace: WorkspacePaths,
    pub bootstrap_script: String,
    pub rdp_profile: String,
    pub steps: Vec<String>,
}

impl LaunchPlan {
    pub fn summary_lines(&self) -> Vec<String> {
        let shared_root = shared_dir_for_workspace(&self.workspace);
        vec![
            format!("Session          {}", self.session_name),
            format!("Distro           {}", self.distro.label()),
            format!(
                "Desktop          {}",
                self.desktop_environment.display_name()
            ),
            format!("Family           {}", self.distro.family.display_name()),
            format!("XRDP Port        {}", self.port),
            format!("Shared Dir       {}", shared_root.display()),
            format!(
                "Bootstrap Script {}",
                self.workspace.bootstrap_script.display()
            ),
            format!("RDP Profile      {}", self.workspace.rdp_profile.display()),
            format!(
                "Bootstrap Log    {}",
                self.workspace.bootstrap_log.display()
            ),
            format!(
                "Transport Log    {}",
                self.workspace.transport_log.display()
            ),
        ]
    }
}

pub fn app_root() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("Pane")
}

pub fn managed_distro_install_root(distro_name: &str) -> PathBuf {
    app_root()
        .join("distros")
        .join(sanitize_session_name(distro_name))
}

pub fn workspace_for(session_name: &str) -> WorkspacePaths {
    workspace_for_with_shared_storage(session_name, SharedStorageMode::Durable)
}

pub fn workspace_for_with_shared_storage(
    session_name: &str,
    shared_storage: SharedStorageMode,
) -> WorkspacePaths {
    let normalized = sanitize_session_name(session_name);
    let root = app_root().join("sessions").join(&normalized);
    let shared_dir = match shared_storage {
        SharedStorageMode::Durable => app_root().join("shared").join(&normalized),
        SharedStorageMode::Scratch => root.join("shared"),
    };
    WorkspacePaths {
        bootstrap_script: root.join("pane-bootstrap.sh"),
        rdp_profile: root.join("pane.rdp"),
        bootstrap_log: root.join("bootstrap.log"),
        transport_log: root.join("transport.log"),
        shared_dir,
        root,
    }
}

pub fn runtime_for(session_name: &str) -> RuntimePaths {
    let normalized = sanitize_session_name(session_name);
    let root = app_root().join("runtime").join(&normalized);
    let downloads = root.join("downloads");
    let images = root.join("images");
    let disks = root.join("disks");
    let snapshots = root.join("snapshots");
    let state = root.join("state");
    let engines = root.join("engines");
    let logs = root.join("logs");

    RuntimePaths {
        base_os_image: images.join("arch-base.paneimg"),
        serial_boot_image: engines.join("serial-boot.paneimg"),
        boot_loader_image: engines.join("boot-to-serial-loader.paneimg"),
        kernel_image: engines.join("linux-kernel.paneimg"),
        initramfs_image: engines.join("initramfs.paneinitrd"),
        user_disk: disks.join("user-data.panedisk"),
        base_os_metadata: state.join("base-os-image.json"),
        serial_boot_metadata: state.join("serial-boot-image.json"),
        boot_loader_metadata: state.join("boot-to-serial-loader.json"),
        kernel_boot_metadata: state.join("kernel-boot.json"),
        user_disk_metadata: state.join("user-disk.json"),
        runtime_config: root.join("pane-runtime.config.json"),
        native_manifest: root.join("pane-native-runtime.json"),
        kernel_boot_layout: state.join("kernel-boot-layout.json"),
        manifest: root.join("pane-runtime.json"),
        downloads,
        images,
        disks,
        snapshots,
        state,
        engines,
        logs,
        root,
    }
}

pub fn shared_dir_for_workspace(workspace: &WorkspacePaths) -> PathBuf {
    if workspace.shared_dir.as_os_str().is_empty() {
        workspace.root.join("shared")
    } else {
        workspace.shared_dir.clone()
    }
}

pub fn shared_readme_for_workspace(workspace: &WorkspacePaths) -> PathBuf {
    shared_dir_for_workspace(workspace).join("README.txt")
}

pub fn write_workspace(plan: &LaunchPlan) -> AppResult<()> {
    fs::create_dir_all(&plan.workspace.root)?;
    fs::write(&plan.workspace.bootstrap_script, &plan.bootstrap_script)?;
    fs::write(&plan.workspace.rdp_profile, &plan.rdp_profile)?;
    fs::write(&plan.workspace.transport_log, "")?;

    let shared_root = shared_dir_for_workspace(&plan.workspace);
    fs::create_dir_all(&shared_root)?;
    fs::write(
        shared_readme_for_workspace(&plan.workspace),
        format!(
            "Pane Shared Directory\n\nSession: {}\nWindows Path: {}\n\nAnything placed here is available to the Linux session through ~/PaneShared after bootstrap.\n",
            plan.session_name,
            shared_root.display()
        ),
    )?;

    Ok(())
}

pub fn sanitize_session_name(raw: &str) -> String {
    let mut value = String::with_capacity(raw.len());

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            value.push(ch.to_ascii_lowercase());
            continue;
        }

        if matches!(ch, '-' | '_' | ' ') && !value.ends_with('-') {
            value.push('-');
        }
    }

    value = value.trim_matches('-').to_string();

    if value.is_empty() {
        "pane".to_string()
    } else {
        value
    }
}

pub fn windows_to_wsl_path(path: &Path) -> String {
    let rendered = path.to_string_lossy().replace('\\', "/");

    if rendered.len() > 2 && rendered.as_bytes()[1] == b':' {
        let drive = rendered
            .chars()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let rest = rendered[2..].trim_start_matches('/');
        format!("/mnt/{drive}/{rest}")
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        managed_distro_install_root, runtime_for, sanitize_session_name, shared_dir_for_workspace,
        windows_to_wsl_path, workspace_for, workspace_for_with_shared_storage,
    };

    fn suffix(components: &[&str]) -> PathBuf {
        let mut path = PathBuf::new();
        for component in components {
            path.push(component);
        }
        path
    }

    fn assert_path_ends_with(path: &Path, components: &[&str]) {
        assert!(
            path.ends_with(suffix(components)),
            "expected path '{}' to end with '{}'",
            path.display(),
            suffix(components).display()
        );
    }

    #[test]
    fn normalizes_session_names() {
        assert_eq!(sanitize_session_name("Pane Session"), "pane-session");
        assert_eq!(sanitize_session_name("pane___alpha"), "pane-alpha");
        assert_eq!(sanitize_session_name("!!!"), "pane");
    }

    #[test]
    fn converts_windows_paths_to_wsl() {
        assert_eq!(
            windows_to_wsl_path("C:\\Users\\Afsah\\Pane\\pane-bootstrap.sh".as_ref()),
            "/mnt/c/Users/Afsah/Pane/pane-bootstrap.sh"
        );
    }

    #[test]
    fn workspace_includes_bootstrap_and_transport_log_paths() {
        let workspace = workspace_for("Pane Session");
        assert_path_ends_with(
            &workspace.bootstrap_log,
            &["sessions", "pane-session", "bootstrap.log"],
        );
        assert_path_ends_with(
            &workspace.transport_log,
            &["sessions", "pane-session", "transport.log"],
        );
        assert_path_ends_with(&workspace.shared_dir, &["Pane", "shared", "pane-session"]);
    }

    #[test]
    fn shared_directory_is_durable_by_default() {
        let workspace = workspace_for("Pane Session");
        assert_path_ends_with(
            &shared_dir_for_workspace(&workspace),
            &["Pane", "shared", "pane-session"],
        );
    }

    #[test]
    fn shared_directory_can_be_scoped_to_the_session_workspace() {
        let workspace = workspace_for_with_shared_storage(
            "Pane Session",
            crate::model::SharedStorageMode::Scratch,
        );
        assert_path_ends_with(
            &shared_dir_for_workspace(&workspace),
            &["sessions", "pane-session", "shared"],
        );
    }

    #[test]
    fn managed_distro_root_is_scoped_under_local_app_data() {
        let root = managed_distro_install_root("Pane Arch");
        assert_path_ends_with(&root, &["Pane", "distros", "pane-arch"]);
    }

    #[test]
    fn runtime_paths_include_native_engine_boundaries() {
        let runtime = runtime_for("Pane Session");

        assert_path_ends_with(&runtime.root, &["Pane", "runtime", "pane-session"]);
        assert_path_ends_with(&runtime.base_os_image, &["images", "arch-base.paneimg"]);
        assert_path_ends_with(
            &runtime.serial_boot_image,
            &["engines", "serial-boot.paneimg"],
        );
        assert_path_ends_with(
            &runtime.boot_loader_image,
            &["engines", "boot-to-serial-loader.paneimg"],
        );
        assert_path_ends_with(&runtime.kernel_image, &["engines", "linux-kernel.paneimg"]);
        assert_path_ends_with(
            &runtime.initramfs_image,
            &["engines", "initramfs.paneinitrd"],
        );
        assert_path_ends_with(&runtime.user_disk, &["disks", "user-data.panedisk"]);
        assert_path_ends_with(
            &runtime.runtime_config,
            &["runtime", "pane-session", "pane-runtime.config.json"],
        );
        assert_path_ends_with(
            &runtime.base_os_metadata,
            &["runtime", "pane-session", "state", "base-os-image.json"],
        );
        assert_path_ends_with(
            &runtime.serial_boot_metadata,
            &["runtime", "pane-session", "state", "serial-boot-image.json"],
        );
        assert_path_ends_with(
            &runtime.boot_loader_metadata,
            &[
                "runtime",
                "pane-session",
                "state",
                "boot-to-serial-loader.json",
            ],
        );
        assert_path_ends_with(
            &runtime.kernel_boot_metadata,
            &["runtime", "pane-session", "state", "kernel-boot.json"],
        );
        assert_path_ends_with(
            &runtime.user_disk_metadata,
            &["runtime", "pane-session", "state", "user-disk.json"],
        );
        assert_path_ends_with(
            &runtime.native_manifest,
            &["runtime", "pane-session", "pane-native-runtime.json"],
        );
        assert_path_ends_with(
            &runtime.kernel_boot_layout,
            &[
                "runtime",
                "pane-session",
                "state",
                "kernel-boot-layout.json",
            ],
        );
        assert_path_ends_with(&runtime.engines, &["runtime", "pane-session", "engines"]);
        assert_path_ends_with(&runtime.logs, &["runtime", "pane-session", "logs"]);
    }
}
