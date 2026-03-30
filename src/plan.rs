use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    model::{DesktopEnvironment, DistroRecord},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePaths {
    pub root: PathBuf,
    pub bootstrap_script: PathBuf,
    pub rdp_profile: PathBuf,
    pub bootstrap_log: PathBuf,
    pub transport_log: PathBuf,
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
    let normalized = sanitize_session_name(session_name);
    let root = app_root().join("sessions").join(&normalized);
    WorkspacePaths {
        bootstrap_script: root.join("pane-bootstrap.sh"),
        rdp_profile: root.join("pane.rdp"),
        bootstrap_log: root.join("bootstrap.log"),
        transport_log: root.join("transport.log"),
        root,
    }
}

pub fn shared_dir_for_workspace(workspace: &WorkspacePaths) -> PathBuf {
    workspace.root.join("shared")
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
    use super::{
        managed_distro_install_root, sanitize_session_name, shared_dir_for_workspace,
        windows_to_wsl_path, workspace_for,
    };

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
        assert!(workspace
            .bootstrap_log
            .ends_with("sessions\\pane-session\\bootstrap.log"));
        assert!(workspace
            .transport_log
            .ends_with("sessions\\pane-session\\transport.log"));
    }

    #[test]
    fn shared_directory_is_scoped_to_the_session_workspace() {
        let workspace = workspace_for("Pane Session");
        assert!(shared_dir_for_workspace(&workspace).ends_with("sessions\\pane-session\\shared"));
    }

    #[test]
    fn managed_distro_root_is_scoped_under_local_app_data() {
        let root = managed_distro_install_root("Pane Arch");
        assert!(root.ends_with("Pane\\distros\\pane-arch"));
    }
}
