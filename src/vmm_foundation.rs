use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VmmFoundationReport {
    pub(crate) selected_strategy: &'static str,
    pub(crate) implementation_rule: &'static str,
    pub(crate) reference_vmm: VmmFoundationComponent,
    pub(crate) adopted_crates: Vec<VmmFoundationComponent>,
    pub(crate) rejected_paths: Vec<VmmRejectedPath>,
    pub(crate) migration_milestones: Vec<VmmMigrationMilestone>,
    pub(crate) immediate_next_steps: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VmmFoundationComponent {
    pub(crate) name: &'static str,
    pub(crate) role: &'static str,
    pub(crate) license: &'static str,
    pub(crate) source: &'static str,
    pub(crate) adoption: VmmAdoptionMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VmmAdoptionMode {
    ReferenceArchitecture,
    DirectDependencyCandidate,
    RuntimeDeviceModel,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VmmRejectedPath {
    pub(crate) name: &'static str,
    pub(crate) reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct VmmMigrationMilestone {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) objective: &'static str,
    pub(crate) replaces: &'static str,
    pub(crate) acceptance_gate: &'static str,
}

pub(crate) fn build_vmm_foundation_report() -> VmmFoundationReport {
    VmmFoundationReport {
        selected_strategy: "crosvm/rust-vmm foundation",
        implementation_rule: "Pane remains the Windows app/runtime owner, but Linux loading and guest devices must move to proven linux-loader and virtio/crosvm-style components instead of expanding Pane's bespoke port-device model.",
        reference_vmm: VmmFoundationComponent {
            name: "crosvm",
            role: "Reference architecture for a Rust VMM with Windows WHPX support, virtio devices, display/input precedent, and Linux guests.",
            license: "BSD-3-Clause",
            source: "https://github.com/google/crosvm",
            adoption: VmmAdoptionMode::ReferenceArchitecture,
        },
        adopted_crates: vec![
            VmmFoundationComponent {
                name: "rust-vmm/linux-loader",
                role: "Primary path for bzImage parsing/loading, command line placement, and Linux boot-parameter construction.",
                license: "Apache-2.0 OR BSD-3-Clause",
                source: "https://github.com/rust-vmm/linux-loader",
                adoption: VmmAdoptionMode::DirectDependencyCandidate,
            },
            VmmFoundationComponent {
                name: "rust-vmm/vm-virtio",
                role: "Primary path for replacing Pane's custom block-port protocol with standard virtio queues and device semantics.",
                license: "Apache-2.0 OR BSD-3-Clause",
                source: "https://github.com/rust-vmm/vm-virtio",
                adoption: VmmAdoptionMode::DirectDependencyCandidate,
            },
            VmmFoundationComponent {
                name: "virtio-blk",
                role: "Guest storage model for the immutable Arch base disk plus writable Pane user disk.",
                license: "Linux guest standard / rust-vmm implementation license",
                source: "https://github.com/rust-vmm/vm-virtio",
                adoption: VmmAdoptionMode::RuntimeDeviceModel,
            },
            VmmFoundationComponent {
                name: "virtio-gpu/input reference path",
                role: "Display and input model to replace the current fixed framebuffer/input queue contracts once boot is deterministic.",
                license: "crosvm/rust-vmm compatible reference path",
                source: "https://github.com/google/crosvm",
                adoption: VmmAdoptionMode::RuntimeDeviceModel,
            },
        ],
        rejected_paths: vec![
            VmmRejectedPath {
                name: "copying crosvm wholesale",
                reason: "Too large for an uncontrolled import; Pane should adopt architecture and audited components incrementally while preserving its app/runtime boundary.",
            },
            VmmRejectedPath {
                name: "QEMU as the default engine",
                reason: "Useful as a comparison/proof tool, but GPL and wrapper-style product shape do not match Pane's intended native app-owned runtime.",
            },
            VmmRejectedPath {
                name: "expanding Pane block-port into a full disk/display stack",
                reason: "The current live probes show bespoke port I/O is fragile around Linux root mount. Standard virtio semantics reduce unknowns and align with real Linux guest expectations.",
            },
        ],
        migration_milestones: vec![
            VmmMigrationMilestone {
                id: "foundation-1",
                title: "linux-loader boot-plan adapter",
                objective: "Replace Pane's hand-built bzImage boot-param construction with a linux-loader-backed adapter while keeping existing kernel-layout reports stable.",
                replaces: "manual setup-header copying and ad hoc boot parameter writes",
                acceptance_gate: "`pane native-kernel-plan --materialize` emits the same Pane storage/display contract plus a linux-loader provenance section and passes existing kernel-layout tests.",
            },
            VmmMigrationMilestone {
                id: "foundation-2",
                title: "virtio block backend contract",
                objective: "Model the Arch base image and Pane user disk as virtio-blk devices before removing the custom pane-block.ko dependency from the boot path.",
                replaces: "Pane block-port submit/status protocol and generated pane-block.ko root device",
                acceptance_gate: "An Arch initramfs can discover standard virtio block devices without a Pane-specific kernel module.",
            },
            VmmMigrationMilestone {
                id: "foundation-3",
                title: "crosvm-style device loop",
                objective: "Route WHP exits through a reusable virtio/MMIO/device event loop instead of per-port special cases.",
                replaces: "monolithic WHP exit handling in native.rs",
                acceptance_gate: "Block, serial, timer, and interrupt exits are dispatched through a typed device model with deterministic trace records.",
            },
            VmmMigrationMilestone {
                id: "foundation-4",
                title: "native display/input transport",
                objective: "Promote the framebuffer/input contracts to a virtio-gpu/input-inspired Pane renderer path.",
                replaces: "fixed framebuffer snapshot reporting and placeholder input queue",
                acceptance_gate: "Pane can render guest-owned pixels and inject keyboard/pointer events without mstsc.exe or XRDP.",
            },
        ],
        immediate_next_steps: vec![
            "Keep current WHP probes only as diagnostics, not the final architecture.",
            "Add linux-loader behind a narrow adapter once dependency licensing and MSRV compatibility are verified in CI.",
            "Design the virtio-blk backend around Pane's existing base-image read-only and user-disk writable policies.",
            "Use crosvm as the reference for WHPX/device-loop/display decisions without making Pane a crosvm wrapper.",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::{build_vmm_foundation_report, VmmAdoptionMode};

    #[test]
    fn foundation_selects_crosvm_and_rust_vmm_without_qemu_default() {
        let report = build_vmm_foundation_report();

        assert_eq!(report.selected_strategy, "crosvm/rust-vmm foundation");
        assert_eq!(report.reference_vmm.name, "crosvm");
        assert_eq!(
            report.reference_vmm.adoption,
            VmmAdoptionMode::ReferenceArchitecture
        );
        assert!(report
            .adopted_crates
            .iter()
            .any(|component| component.name == "rust-vmm/linux-loader"));
        assert!(report
            .adopted_crates
            .iter()
            .any(|component| component.name == "rust-vmm/vm-virtio"));
        assert!(report
            .rejected_paths
            .iter()
            .any(|path| path.name == "QEMU as the default engine"));
    }

    #[test]
    fn foundation_milestones_replace_bespoke_block_path() {
        let report = build_vmm_foundation_report();

        assert!(report.migration_milestones.iter().any(|milestone| {
            milestone.id == "foundation-2" && milestone.replaces.contains("pane-block.ko")
        }));
        assert!(report
            .immediate_next_steps
            .iter()
            .any(|step| step.contains("virtio-blk")));
    }
}
