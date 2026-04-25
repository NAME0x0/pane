use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopEnvironment {
    #[default]
    Xfce,
}

impl DesktopEnvironment {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Xfce => "XFCE",
        }
    }

    pub fn is_mvp_supported(self) -> bool {
        matches!(self, Self::Xfce)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SharedStorageMode {
    #[default]
    Durable,
    Scratch,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    #[default]
    WslBridge,
    PaneOwned,
}

impl RuntimeMode {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::WslBridge => "WSL2 + XRDP bridge",
            Self::PaneOwned => "Pane-owned OS runtime",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistroFamily {
    Ubuntu,
    Debian,
    Fedora,
    Arch,
    #[default]
    Unknown,
}

impl DistroFamily {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Ubuntu => "Ubuntu",
            Self::Debian => "Debian",
            Self::Fedora => "Fedora",
            Self::Arch => "Arch",
            Self::Unknown => "Unknown",
        }
    }

    pub fn is_mvp_supported(self) -> bool {
        matches!(self, Self::Arch)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedEnvironmentStage {
    Current,
    Next,
    Later,
}

impl ManagedEnvironmentStage {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Current => "Current",
            Self::Next => "Next",
            Self::Later => "Later",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedEnvironmentTier {
    FirstClass,
    CuratedPreview,
}

impl ManagedEnvironmentTier {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::FirstClass => "First-Class",
            Self::CuratedPreview => "Curated Preview",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManagedEnvironment {
    pub id: String,
    pub display_name: String,
    pub family: DistroFamily,
    pub stage: ManagedEnvironmentStage,
    pub tier: ManagedEnvironmentTier,
    pub launchable_now: bool,
    pub starter_profile: Option<String>,
    pub summary: String,
}

pub fn managed_environment_catalog() -> Vec<ManagedEnvironment> {
    vec![
        ManagedEnvironment {
            id: "arch".to_string(),
            display_name: "Arch Linux".to_string(),
            family: DistroFamily::Arch,
            stage: ManagedEnvironmentStage::Current,
            tier: ManagedEnvironmentTier::FirstClass,
            launchable_now: true,
            starter_profile: Some("XFCE".to_string()),
            summary: "Flagship managed environment. Current first-class path and reference distro for Pane."
                .to_string(),
        },
        ManagedEnvironment {
            id: "ubuntu-lts".to_string(),
            display_name: "Ubuntu LTS".to_string(),
            family: DistroFamily::Ubuntu,
            stage: ManagedEnvironmentStage::Next,
            tier: ManagedEnvironmentTier::FirstClass,
            launchable_now: false,
            starter_profile: None,
            summary: "Next first-class managed environment for broader familiarity and mainstream adoption."
                .to_string(),
        },
        ManagedEnvironment {
            id: "debian".to_string(),
            display_name: "Debian".to_string(),
            family: DistroFamily::Debian,
            stage: ManagedEnvironmentStage::Later,
            tier: ManagedEnvironmentTier::CuratedPreview,
            launchable_now: false,
            starter_profile: None,
            summary: "Later curated environment for users who want a conservative, lower-churn base once lifecycle ownership is mature."
                .to_string(),
        },
    ]
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistroRecord {
    pub name: String,
    pub family: DistroFamily,
    pub pretty_name: Option<String>,
    pub state: Option<String>,
    pub version: Option<String>,
    pub default: bool,
    pub default_user: Option<String>,
}

impl DistroRecord {
    pub fn label(&self) -> String {
        self.pretty_name
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.name.clone())
    }

    pub fn is_mvp_supported(&self) -> bool {
        self.family.is_mvp_supported()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        managed_environment_catalog, DistroFamily, ManagedEnvironmentStage, ManagedEnvironmentTier,
    };

    #[test]
    fn managed_environment_catalog_is_ordered_and_curated() {
        let catalog = managed_environment_catalog();
        assert_eq!(catalog.len(), 3);

        assert_eq!(catalog[0].id, "arch");
        assert_eq!(catalog[0].family, DistroFamily::Arch);
        assert_eq!(catalog[0].stage, ManagedEnvironmentStage::Current);
        assert_eq!(catalog[0].tier, ManagedEnvironmentTier::FirstClass);
        assert!(catalog[0].launchable_now);

        assert_eq!(catalog[1].id, "ubuntu-lts");
        assert_eq!(catalog[1].family, DistroFamily::Ubuntu);
        assert_eq!(catalog[1].stage, ManagedEnvironmentStage::Next);
        assert_eq!(catalog[1].tier, ManagedEnvironmentTier::FirstClass);
        assert!(!catalog[1].launchable_now);

        assert_eq!(catalog[2].id, "debian");
        assert_eq!(catalog[2].family, DistroFamily::Debian);
        assert_eq!(catalog[2].stage, ManagedEnvironmentStage::Later);
        assert_eq!(catalog[2].tier, ManagedEnvironmentTier::CuratedPreview);
        assert!(!catalog[2].launchable_now);
    }
}
