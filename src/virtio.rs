use serde::Serialize;

pub(crate) const PANE_VIRTIO_MMIO_BASE_GPA: u64 = 0x0dfc_0000;
pub(crate) const PANE_VIRTIO_MMIO_SIZE_BYTES: u64 = 0x0000_1000;
pub(crate) const VIRTIO_MMIO_MAGIC_VALUE: u32 = 0x7472_6976;
pub(crate) const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
pub(crate) const VIRTIO_DEVICE_ID_BLOCK: u32 = 2;
pub(crate) const VIRTIO_DEVICE_ID_GPU: u32 = 16;
pub(crate) const VIRTIO_DEVICE_ID_INPUT: u32 = 18;
pub(crate) const VIRTIO_MMIO_CONFIG_OFFSET: u64 = 0x100;
pub(crate) const VIRTIO_BLK_SECTOR_SIZE_BYTES: u64 = 512;
pub(crate) const PANE_VIRTIO_BLK_QUEUE_SIZE: u16 = 256;

const VIRTIO_MMIO_MAGIC_VALUE_OFFSET: u64 = 0x000;
const VIRTIO_MMIO_VERSION_OFFSET: u64 = 0x004;
const VIRTIO_MMIO_DEVICE_ID_OFFSET: u64 = 0x008;
const VIRTIO_MMIO_VENDOR_ID_OFFSET: u64 = 0x00c;
const VIRTIO_MMIO_DEVICE_FEATURES_OFFSET: u64 = 0x010;
const VIRTIO_MMIO_DEVICE_FEATURES_SEL_OFFSET: u64 = 0x014;
const VIRTIO_MMIO_DRIVER_FEATURES_OFFSET: u64 = 0x020;
const VIRTIO_MMIO_DRIVER_FEATURES_SEL_OFFSET: u64 = 0x024;
const VIRTIO_MMIO_QUEUE_SEL_OFFSET: u64 = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX_OFFSET: u64 = 0x034;
const VIRTIO_MMIO_QUEUE_NUM_OFFSET: u64 = 0x038;
const VIRTIO_MMIO_QUEUE_READY_OFFSET: u64 = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY_OFFSET: u64 = 0x050;
const VIRTIO_MMIO_INTERRUPT_STATUS_OFFSET: u64 = 0x060;
const VIRTIO_MMIO_INTERRUPT_ACK_OFFSET: u64 = 0x064;
const VIRTIO_MMIO_STATUS_OFFSET: u64 = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET: u64 = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH_OFFSET: u64 = 0x084;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET: u64 = 0x090;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH_OFFSET: u64 = 0x094;
const VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET: u64 = 0x0a0;
const VIRTIO_MMIO_QUEUE_USED_HIGH_OFFSET: u64 = 0x0a4;
const VIRTIO_MMIO_CONFIG_GENERATION_OFFSET: u64 = 0x0fc;
const PANE_VENDOR_ID: u32 = 0x7061_6e65;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioWindow {
    pub(crate) label: &'static str,
    pub(crate) base_gpa: String,
    pub(crate) size_bytes: u64,
    pub(crate) transport: &'static str,
    pub(crate) handshake_smoke: PaneVirtioMmioHandshakeSmoke,
    pub(crate) primary_device: PaneVirtioDeviceSummary,
    pub(crate) future_devices: Vec<PaneVirtioDeviceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioDeviceSummary {
    pub(crate) id: &'static str,
    pub(crate) virtio_device_id: u32,
    pub(crate) purpose: &'static str,
    pub(crate) status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioBlkConfig {
    pub(crate) capacity_sectors: u64,
    pub(crate) sector_size_bytes: u64,
    pub(crate) readonly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioHandshakeSmoke {
    pub(crate) status: &'static str,
    pub(crate) queue_size: u16,
    pub(crate) desc_table_gpa: String,
    pub(crate) avail_ring_gpa: String,
    pub(crate) used_ring_gpa: String,
    pub(crate) last_notify: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioQueueState {
    pub(crate) max_size: u16,
    pub(crate) size: u16,
    pub(crate) ready: bool,
    pub(crate) desc_table_gpa: u64,
    pub(crate) avail_ring_gpa: u64,
    pub(crate) used_ring_gpa: u64,
    pub(crate) last_notify: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioBlockDevice {
    pub(crate) device_id: u32,
    pub(crate) vendor_id: u32,
    pub(crate) device_features: u64,
    pub(crate) driver_features: u64,
    pub(crate) device_features_select: u32,
    pub(crate) driver_features_select: u32,
    pub(crate) status: u32,
    pub(crate) interrupt_status: u32,
    pub(crate) config_generation: u32,
    pub(crate) queue_select: u16,
    pub(crate) queue: PaneVirtioQueueState,
    pub(crate) config: PaneVirtioBlkConfig,
}

impl PaneVirtioMmioBlockDevice {
    pub(crate) fn new(logical_size_bytes: u64, readonly: bool) -> Self {
        Self {
            device_id: VIRTIO_DEVICE_ID_BLOCK,
            vendor_id: PANE_VENDOR_ID,
            device_features: 0,
            driver_features: 0,
            device_features_select: 0,
            driver_features_select: 0,
            status: 0,
            interrupt_status: 0,
            config_generation: 0,
            queue_select: 0,
            queue: PaneVirtioQueueState {
                max_size: PANE_VIRTIO_BLK_QUEUE_SIZE,
                size: 0,
                ready: false,
                desc_table_gpa: 0,
                avail_ring_gpa: 0,
                used_ring_gpa: 0,
                last_notify: None,
            },
            config: PaneVirtioBlkConfig {
                capacity_sectors: logical_size_bytes / VIRTIO_BLK_SECTOR_SIZE_BYTES,
                sector_size_bytes: VIRTIO_BLK_SECTOR_SIZE_BYTES,
                readonly,
            },
        }
    }

    pub(crate) fn read_u32(&self, offset: u64) -> Option<u32> {
        match offset {
            VIRTIO_MMIO_MAGIC_VALUE_OFFSET => Some(VIRTIO_MMIO_MAGIC_VALUE),
            VIRTIO_MMIO_VERSION_OFFSET => Some(VIRTIO_MMIO_VERSION_MODERN),
            VIRTIO_MMIO_DEVICE_ID_OFFSET => Some(self.device_id),
            VIRTIO_MMIO_VENDOR_ID_OFFSET => Some(self.vendor_id),
            VIRTIO_MMIO_DEVICE_FEATURES_OFFSET => match self.device_features_select {
                0 => Some(self.device_features as u32),
                1 => Some((self.device_features >> 32) as u32),
                _ => Some(0),
            },
            VIRTIO_MMIO_QUEUE_NUM_MAX_OFFSET => Some(u32::from(self.queue.max_size)),
            VIRTIO_MMIO_QUEUE_NUM_OFFSET => Some(u32::from(self.queue.size)),
            VIRTIO_MMIO_QUEUE_READY_OFFSET => Some(u32::from(self.queue.ready)),
            VIRTIO_MMIO_INTERRUPT_STATUS_OFFSET => Some(self.interrupt_status),
            VIRTIO_MMIO_STATUS_OFFSET => Some(self.status),
            VIRTIO_MMIO_CONFIG_GENERATION_OFFSET => Some(self.config_generation),
            offset if offset >= VIRTIO_MMIO_CONFIG_OFFSET => self.read_config_u32(offset),
            _ => None,
        }
    }

    pub(crate) fn write_u32(&mut self, offset: u64, value: u32) -> PaneVirtioMmioWriteResult {
        match offset {
            VIRTIO_MMIO_DEVICE_FEATURES_SEL_OFFSET => {
                self.device_features_select = value;
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_DRIVER_FEATURES_OFFSET => {
                self.set_driver_features(value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_DRIVER_FEATURES_SEL_OFFSET => {
                self.driver_features_select = value;
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_SEL_OFFSET => {
                self.queue_select = value as u16;
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_NUM_OFFSET => {
                if value == 0 || value > u32::from(self.queue.max_size) || !value.is_power_of_two()
                {
                    PaneVirtioMmioWriteResult::Rejected("invalid virtqueue size")
                } else {
                    self.queue.size = value as u16;
                    PaneVirtioMmioWriteResult::Accepted
                }
            }
            VIRTIO_MMIO_QUEUE_READY_OFFSET => {
                self.queue.ready = value == 1;
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_NOTIFY_OFFSET => {
                self.queue.last_notify = Some(value);
                PaneVirtioMmioWriteResult::QueueNotified(value)
            }
            VIRTIO_MMIO_INTERRUPT_ACK_OFFSET => {
                self.interrupt_status &= !value;
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_STATUS_OFFSET => {
                if value == 0 {
                    self.reset_driver_state();
                } else {
                    self.status |= value;
                }
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET => {
                self.queue.desc_table_gpa = combine_addr_low(self.queue.desc_table_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_DESC_HIGH_OFFSET => {
                self.queue.desc_table_gpa = combine_addr_high(self.queue.desc_table_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET => {
                self.queue.avail_ring_gpa = combine_addr_low(self.queue.avail_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_AVAIL_HIGH_OFFSET => {
                self.queue.avail_ring_gpa = combine_addr_high(self.queue.avail_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET => {
                self.queue.used_ring_gpa = combine_addr_low(self.queue.used_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_USED_HIGH_OFFSET => {
                self.queue.used_ring_gpa = combine_addr_high(self.queue.used_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            _ => PaneVirtioMmioWriteResult::Ignored,
        }
    }

    fn set_driver_features(&mut self, value: u32) {
        let low = self.driver_features & 0xffff_ffff_0000_0000;
        let high = self.driver_features & 0x0000_0000_ffff_ffff;
        self.driver_features = match self.driver_features_select {
            0 => low | u64::from(value),
            1 => high | (u64::from(value) << 32),
            _ => self.driver_features,
        };
    }

    fn read_config_u32(&self, offset: u64) -> Option<u32> {
        let config_offset = offset - VIRTIO_MMIO_CONFIG_OFFSET;
        match config_offset {
            0x00 => Some(self.config.capacity_sectors as u32),
            0x04 => Some((self.config.capacity_sectors >> 32) as u32),
            0x14 => Some(self.config.sector_size_bytes as u32),
            _ => Some(0),
        }
    }

    fn reset_driver_state(&mut self) {
        self.driver_features = 0;
        self.driver_features_select = 0;
        self.status = 0;
        self.interrupt_status = 0;
        self.queue_select = 0;
        self.queue.size = 0;
        self.queue.ready = false;
        self.queue.desc_table_gpa = 0;
        self.queue.avail_ring_gpa = 0;
        self.queue.used_ring_gpa = 0;
        self.queue.last_notify = None;
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PaneVirtioMmioWriteResult {
    Accepted,
    QueueNotified(u32),
    Rejected(&'static str),
    Ignored,
}

pub(crate) fn pane_virtio_mmio_window() -> PaneVirtioMmioWindow {
    PaneVirtioMmioWindow {
        label: "pane-virtio-mmio",
        base_gpa: format_guest_physical_address(PANE_VIRTIO_MMIO_BASE_GPA),
        size_bytes: PANE_VIRTIO_MMIO_SIZE_BYTES,
        transport: "virtio-mmio",
        handshake_smoke: pane_virtio_mmio_handshake_smoke(),
        primary_device: PaneVirtioDeviceSummary {
            id: "vda",
            virtio_device_id: VIRTIO_DEVICE_ID_BLOCK,
            purpose: "read-only Arch base disk first, then writable user disk queue support",
            status: "mmio-register-model-ready-queue-execution-pending",
        },
        future_devices: vec![
            PaneVirtioDeviceSummary {
                id: "virtio-gpu",
                virtio_device_id: VIRTIO_DEVICE_ID_GPU,
                purpose: "Pane app-window guest display surface",
                status: "planned",
            },
            PaneVirtioDeviceSummary {
                id: "virtio-input",
                virtio_device_id: VIRTIO_DEVICE_ID_INPUT,
                purpose: "Pane app-window keyboard and pointer injection",
                status: "planned",
            },
        ],
    }
}

pub(crate) fn pane_virtio_mmio_handshake_smoke() -> PaneVirtioMmioHandshakeSmoke {
    let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
    let writes = [
        (VIRTIO_MMIO_DEVICE_FEATURES_SEL_OFFSET, 0),
        (VIRTIO_MMIO_DRIVER_FEATURES_SEL_OFFSET, 0),
        (VIRTIO_MMIO_DRIVER_FEATURES_OFFSET, 0),
        (VIRTIO_MMIO_STATUS_OFFSET, 1),
        (VIRTIO_MMIO_STATUS_OFFSET, 2),
        (VIRTIO_MMIO_STATUS_OFFSET, 8),
        (VIRTIO_MMIO_QUEUE_SEL_OFFSET, 0),
        (
            VIRTIO_MMIO_QUEUE_NUM_OFFSET,
            u32::from(PANE_VIRTIO_BLK_QUEUE_SIZE),
        ),
        (VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET, 0x0010_0000),
        (VIRTIO_MMIO_QUEUE_DESC_HIGH_OFFSET, 0),
        (VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET, 0x0011_0000),
        (VIRTIO_MMIO_QUEUE_AVAIL_HIGH_OFFSET, 0),
        (VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET, 0x0012_0000),
        (VIRTIO_MMIO_QUEUE_USED_HIGH_OFFSET, 0),
        (VIRTIO_MMIO_QUEUE_READY_OFFSET, 1),
        (VIRTIO_MMIO_QUEUE_NOTIFY_OFFSET, 0),
        (VIRTIO_MMIO_INTERRUPT_ACK_OFFSET, 1),
    ];

    for (offset, value) in writes {
        let _ = device.write_u32(offset, value);
    }

    PaneVirtioMmioHandshakeSmoke {
        status: if device.queue.ready && device.queue.last_notify == Some(0) {
            "register-handshake-ready"
        } else {
            "register-handshake-incomplete"
        },
        queue_size: device.queue.size,
        desc_table_gpa: format_guest_physical_address(device.queue.desc_table_gpa),
        avail_ring_gpa: format_guest_physical_address(device.queue.avail_ring_gpa),
        used_ring_gpa: format_guest_physical_address(device.queue.used_ring_gpa),
        last_notify: device.queue.last_notify,
    }
}

pub(crate) fn pane_virtio_mmio_contains_gpa(gpa: u64) -> bool {
    (PANE_VIRTIO_MMIO_BASE_GPA..PANE_VIRTIO_MMIO_BASE_GPA + PANE_VIRTIO_MMIO_SIZE_BYTES)
        .contains(&gpa)
}

pub(crate) fn format_guest_physical_address(gpa: u64) -> String {
    format!("0x{gpa:08x}")
}

fn combine_addr_low(current: u64, low: u32) -> u64 {
    (current & 0xffff_ffff_0000_0000) | u64::from(low)
}

fn combine_addr_high(current: u64, high: u32) -> u64 {
    (current & 0x0000_0000_ffff_ffff) | (u64::from(high) << 32)
}

#[cfg(test)]
mod tests {
    use super::{
        pane_virtio_mmio_contains_gpa, pane_virtio_mmio_window, PaneVirtioMmioBlockDevice,
        PaneVirtioMmioWriteResult, PANE_VIRTIO_BLK_QUEUE_SIZE, PANE_VIRTIO_MMIO_BASE_GPA,
        PANE_VIRTIO_MMIO_SIZE_BYTES, VIRTIO_DEVICE_ID_BLOCK, VIRTIO_MMIO_MAGIC_VALUE,
        VIRTIO_MMIO_VERSION_MODERN,
    };

    #[test]
    fn virtio_mmio_block_device_reports_modern_block_identity() {
        let device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);

        assert_eq!(device.read_u32(0x000), Some(VIRTIO_MMIO_MAGIC_VALUE));
        assert_eq!(device.read_u32(0x004), Some(VIRTIO_MMIO_VERSION_MODERN));
        assert_eq!(device.read_u32(0x008), Some(VIRTIO_DEVICE_ID_BLOCK));
        assert_eq!(
            device.read_u32(0x034),
            Some(u32::from(PANE_VIRTIO_BLK_QUEUE_SIZE))
        );
        assert_eq!(device.read_u32(0x100), Some(2048));
        assert_eq!(device.read_u32(0x104), Some(0));
        assert_eq!(device.read_u32(0x114), Some(512));
    }

    #[test]
    fn virtio_mmio_block_device_tracks_queue_registers_and_notify() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);

        assert_eq!(
            device.write_u32(0x038, 128),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x080, 0x1000),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x084, 0x1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x090, 0x2000),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x0a0, 0x3000),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x044, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x050, 0),
            PaneVirtioMmioWriteResult::QueueNotified(0)
        );

        assert_eq!(device.queue.size, 128);
        assert!(device.queue.ready);
        assert_eq!(device.queue.desc_table_gpa, 0x1_0000_1000);
        assert_eq!(device.queue.avail_ring_gpa, 0x2000);
        assert_eq!(device.queue.used_ring_gpa, 0x3000);
        assert_eq!(device.queue.last_notify, Some(0));
    }

    #[test]
    fn virtio_mmio_block_device_rejects_invalid_queue_size() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);

        assert_eq!(
            device.write_u32(0x038, 127),
            PaneVirtioMmioWriteResult::Rejected("invalid virtqueue size")
        );
        assert_eq!(device.queue.size, 0);
    }

    #[test]
    fn pane_virtio_mmio_window_declares_storage_display_and_input_path() {
        let window = pane_virtio_mmio_window();

        assert_eq!(window.base_gpa, "0x0dfc0000");
        assert_eq!(window.size_bytes, PANE_VIRTIO_MMIO_SIZE_BYTES);
        assert_eq!(window.handshake_smoke.status, "register-handshake-ready");
        assert_eq!(
            window.handshake_smoke.queue_size,
            PANE_VIRTIO_BLK_QUEUE_SIZE
        );
        assert_eq!(window.handshake_smoke.desc_table_gpa, "0x00100000");
        assert_eq!(window.handshake_smoke.avail_ring_gpa, "0x00110000");
        assert_eq!(window.handshake_smoke.used_ring_gpa, "0x00120000");
        assert_eq!(window.handshake_smoke.last_notify, Some(0));
        assert_eq!(
            window.primary_device.virtio_device_id,
            VIRTIO_DEVICE_ID_BLOCK
        );
        assert!(window
            .future_devices
            .iter()
            .any(|device| device.id == "virtio-gpu"));
        assert!(window
            .future_devices
            .iter()
            .any(|device| device.id == "virtio-input"));
        assert!(pane_virtio_mmio_contains_gpa(PANE_VIRTIO_MMIO_BASE_GPA));
        assert!(pane_virtio_mmio_contains_gpa(
            PANE_VIRTIO_MMIO_BASE_GPA + PANE_VIRTIO_MMIO_SIZE_BYTES - 1
        ));
        assert!(!pane_virtio_mmio_contains_gpa(
            PANE_VIRTIO_MMIO_BASE_GPA + PANE_VIRTIO_MMIO_SIZE_BYTES
        ));
    }
}
