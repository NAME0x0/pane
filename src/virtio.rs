use serde::Serialize;

pub(crate) const PANE_VIRTIO_MMIO_BASE_GPA: u64 = 0x0dfc_0000;
pub(crate) const PANE_VIRTIO_MMIO_SIZE_BYTES: u64 = 0x0000_1000;
pub(crate) const PANE_VIRTIO_MMIO_IRQ: u32 = 5;
pub(crate) const VIRTIO_MMIO_MAGIC_VALUE: u32 = 0x7472_6976;
pub(crate) const VIRTIO_MMIO_VERSION_MODERN: u32 = 2;
pub(crate) const VIRTIO_DEVICE_ID_BLOCK: u32 = 2;
pub(crate) const VIRTIO_DEVICE_ID_GPU: u32 = 16;
pub(crate) const VIRTIO_DEVICE_ID_INPUT: u32 = 18;
pub(crate) const VIRTIO_MMIO_CONFIG_OFFSET: u64 = 0x100;
pub(crate) const VIRTIO_BLK_SECTOR_SIZE_BYTES: u64 = 512;
pub(crate) const PANE_VIRTIO_BLK_QUEUE_SIZE: u16 = 256;
pub(crate) const VIRTIO_BLK_STATUS_OK: u8 = 0;
pub(crate) const VIRTIO_BLK_STATUS_IOERR: u8 = 1;
pub(crate) const VIRTIO_BLK_STATUS_UNSUPP: u8 = 2;

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
const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;
const VIRTIO_BLK_T_GET_ID: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioWindow {
    pub(crate) label: &'static str,
    pub(crate) base_gpa: String,
    pub(crate) size_bytes: u64,
    pub(crate) transport: &'static str,
    pub(crate) handshake_smoke: PaneVirtioMmioHandshakeSmoke,
    pub(crate) execution_smoke: PaneVirtioBlkExecutionSmoke,
    pub(crate) service_smoke: PaneVirtioMmioServiceSmoke,
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
pub(crate) struct PaneVirtioBlkExecutionSmoke {
    pub(crate) status: &'static str,
    pub(crate) request_type: PaneVirtioBlkRequestType,
    pub(crate) sector: u64,
    pub(crate) bytes_transferred: u32,
    pub(crate) used_len: u32,
    pub(crate) interrupt_status: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioServiceSmoke {
    pub(crate) status: &'static str,
    pub(crate) register_read_status: &'static str,
    pub(crate) queue_notify_status: &'static str,
    pub(crate) queue_execution_status: u8,
    pub(crate) bytes_transferred: u32,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioBlkRequest {
    pub(crate) request_type: PaneVirtioBlkRequestType,
    pub(crate) sector: u64,
    pub(crate) data_descriptors: Vec<PaneVirtioDescriptor>,
    pub(crate) status_addr: u64,
    pub(crate) status_len: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PaneVirtioBlkRequestType {
    In,
    Out,
    Flush,
    GetId,
    Unsupported(u32),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioDescriptor {
    pub(crate) addr: u64,
    pub(crate) len: u32,
    pub(crate) flags: u16,
    pub(crate) next: u16,
}

impl PaneVirtioDescriptor {
    fn has_next(self) -> bool {
        (self.flags & VIRTQ_DESC_F_NEXT) != 0
    }

    fn writable(self) -> bool {
        (self.flags & VIRTQ_DESC_F_WRITE) != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioBlkExecution {
    pub(crate) request: Option<PaneVirtioBlkRequest>,
    pub(crate) status: u8,
    pub(crate) bytes_transferred: u32,
    pub(crate) used_head_index: u16,
    pub(crate) used_len: u32,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioAccess {
    pub(crate) kind: PaneVirtioMmioAccessKind,
    pub(crate) gpa: u64,
    pub(crate) data: Vec<u8>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PaneVirtioMmioAccessKind {
    Read,
    Write,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioAccessOutcome {
    pub(crate) accepted: bool,
    pub(crate) status: &'static str,
    pub(crate) offset: Option<u64>,
    pub(crate) read_data: Vec<u8>,
    pub(crate) write_result: Option<PaneVirtioMmioWriteResult>,
    pub(crate) queue_execution: Option<PaneVirtioBlkExecution>,
    pub(crate) detail: String,
}

pub(crate) trait PaneGuestMemory {
    fn read(&self, gpa: u64, bytes: &mut [u8]) -> Result<(), String>;
    fn write(&mut self, gpa: u64, bytes: &[u8]) -> Result<(), String>;
}

pub(crate) fn service_virtio_mmio_access<M, F>(
    device: &mut PaneVirtioMmioBlockDevice,
    memory: &mut M,
    access: PaneVirtioMmioAccess,
    service: F,
) -> PaneVirtioMmioAccessOutcome
where
    M: PaneGuestMemory,
    F: FnOnce(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
{
    let Some(offset) = pane_virtio_mmio_offset(access.gpa) else {
        return PaneVirtioMmioAccessOutcome {
            accepted: false,
            status: "outside-virtio-mmio-window",
            offset: None,
            read_data: Vec::new(),
            write_result: None,
            queue_execution: None,
            detail: format!(
                "GPA {} is outside the Pane virtio-MMIO window.",
                format_guest_physical_address(access.gpa)
            ),
        };
    };

    match access.kind {
        PaneVirtioMmioAccessKind::Read => {
            if !access.data.is_empty() && access.data.len() != 4 {
                return unsupported_mmio_width(offset, access.data.len());
            }
            match device.read_u32(offset) {
                Some(value) => PaneVirtioMmioAccessOutcome {
                    accepted: true,
                    status: "register-read",
                    offset: Some(offset),
                    read_data: value.to_le_bytes().to_vec(),
                    write_result: None,
                    queue_execution: None,
                    detail: format!("Read virtio-MMIO register offset 0x{offset:03x}."),
                },
                None => PaneVirtioMmioAccessOutcome {
                    accepted: false,
                    status: "unknown-register",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: None,
                    queue_execution: None,
                    detail: format!("No virtio-MMIO register exists at offset 0x{offset:03x}."),
                },
            }
        }
        PaneVirtioMmioAccessKind::Write => {
            if access.data.len() != 4 {
                return unsupported_mmio_width(offset, access.data.len());
            }
            let value =
                u32::from_le_bytes(access.data[..4].try_into().expect("checked write width"));
            let write_result = device.write_u32(offset, value);
            match write_result {
                PaneVirtioMmioWriteResult::Accepted => PaneVirtioMmioAccessOutcome {
                    accepted: true,
                    status: "register-write",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: Some(write_result),
                    queue_execution: None,
                    detail: format!("Wrote virtio-MMIO register offset 0x{offset:03x}."),
                },
                PaneVirtioMmioWriteResult::QueueNotified(queue_index) => {
                    let execution = device.execute_available_block_request(memory, service);
                    PaneVirtioMmioAccessOutcome {
                        accepted: execution.status == VIRTIO_BLK_STATUS_OK,
                        status: "queue-notify-executed",
                        offset: Some(offset),
                        read_data: Vec::new(),
                        write_result: Some(write_result),
                        queue_execution: Some(execution.clone()),
                        detail: format!(
                            "Queue notify {queue_index} executed with status {}.",
                            execution.status
                        ),
                    }
                }
                PaneVirtioMmioWriteResult::Rejected(reason) => PaneVirtioMmioAccessOutcome {
                    accepted: false,
                    status: "register-write-rejected",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: Some(write_result),
                    queue_execution: None,
                    detail: reason.to_string(),
                },
                PaneVirtioMmioWriteResult::Ignored => PaneVirtioMmioAccessOutcome {
                    accepted: false,
                    status: "register-write-ignored",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: Some(write_result),
                    queue_execution: None,
                    detail: format!("Ignored virtio-MMIO write at offset 0x{offset:03x}."),
                },
            }
        }
    }
}

fn unsupported_mmio_width(offset: u64, width: usize) -> PaneVirtioMmioAccessOutcome {
    PaneVirtioMmioAccessOutcome {
        accepted: false,
        status: "unsupported-width",
        offset: Some(offset),
        read_data: Vec::new(),
        write_result: None,
        queue_execution: None,
        detail: format!("Pane virtio-MMIO currently supports 4-byte accesses, got {width}."),
    }
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

    pub(crate) fn execute_available_block_request<M, F>(
        &mut self,
        memory: &mut M,
        service: F,
    ) -> PaneVirtioBlkExecution
    where
        M: PaneGuestMemory,
        F: FnOnce(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        if !self.queue.ready || self.queue.size == 0 {
            return PaneVirtioBlkExecution {
                request: None,
                status: VIRTIO_BLK_STATUS_IOERR,
                bytes_transferred: 0,
                used_head_index: 0,
                used_len: 0,
                detail: "virtqueue is not ready".to_string(),
            };
        }

        let Some(head_index) = read_avail_head(memory, &self.queue) else {
            return PaneVirtioBlkExecution {
                request: None,
                status: VIRTIO_BLK_STATUS_IOERR,
                bytes_transferred: 0,
                used_head_index: 0,
                used_len: 0,
                detail: "available ring does not contain a request".to_string(),
            };
        };

        match parse_virtio_blk_request(memory, &self.queue, head_index) {
            Ok(request) => self.execute_parsed_block_request(memory, request, head_index, service),
            Err(error) => PaneVirtioBlkExecution {
                request: None,
                status: VIRTIO_BLK_STATUS_IOERR,
                bytes_transferred: 0,
                used_head_index: head_index,
                used_len: 0,
                detail: error,
            },
        }
    }

    fn execute_parsed_block_request<M, F>(
        &mut self,
        memory: &mut M,
        request: PaneVirtioBlkRequest,
        head_index: u16,
        service: F,
    ) -> PaneVirtioBlkExecution
    where
        M: PaneGuestMemory,
        F: FnOnce(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        let write_payload = match request.request_type {
            PaneVirtioBlkRequestType::Out => match read_request_payload(memory, &request) {
                Ok(payload) => Some(payload),
                Err(error) => {
                    let _ = memory.write(request.status_addr, &[VIRTIO_BLK_STATUS_IOERR]);
                    return PaneVirtioBlkExecution {
                        request: Some(request),
                        status: VIRTIO_BLK_STATUS_IOERR,
                        bytes_transferred: 0,
                        used_head_index: head_index,
                        used_len: 0,
                        detail: error,
                    };
                }
            },
            _ => None,
        };

        let service_result = match request.request_type {
            PaneVirtioBlkRequestType::In | PaneVirtioBlkRequestType::Out => {
                service(&request, write_payload.as_deref())
            }
            PaneVirtioBlkRequestType::Flush => Ok(Vec::new()),
            PaneVirtioBlkRequestType::GetId => Ok(b"pane-virtio-blk\0".to_vec()),
            PaneVirtioBlkRequestType::Unsupported(value) => {
                Err(format!("unsupported virtio-blk request type {value}"))
            }
        };

        match service_result {
            Ok(bytes) => {
                let status = match request.request_type {
                    PaneVirtioBlkRequestType::In | PaneVirtioBlkRequestType::GetId => {
                        write_read_response(memory, &request, &bytes)
                    }
                    PaneVirtioBlkRequestType::Out | PaneVirtioBlkRequestType::Flush => {
                        VIRTIO_BLK_STATUS_OK
                    }
                    PaneVirtioBlkRequestType::Unsupported(_) => VIRTIO_BLK_STATUS_UNSUPP,
                };
                let _ = memory.write(request.status_addr, &[status]);
                let used_len = if status == VIRTIO_BLK_STATUS_OK {
                    bytes.len() as u32 + 1
                } else {
                    1
                };
                let _ = write_used_entry(memory, &self.queue, head_index, used_len);
                self.interrupt_status |= 1;
                PaneVirtioBlkExecution {
                    request: Some(request),
                    status,
                    bytes_transferred: used_len.saturating_sub(1),
                    used_head_index: head_index,
                    used_len,
                    detail: if status == VIRTIO_BLK_STATUS_OK {
                        "virtio-blk request serviced".to_string()
                    } else {
                        "virtio-blk request failed while writing response".to_string()
                    },
                }
            }
            Err(error) => {
                let status = if error.contains("unsupported") {
                    VIRTIO_BLK_STATUS_UNSUPP
                } else {
                    VIRTIO_BLK_STATUS_IOERR
                };
                let _ = memory.write(request.status_addr, &[status]);
                let _ = write_used_entry(memory, &self.queue, head_index, 1);
                self.interrupt_status |= 1;
                PaneVirtioBlkExecution {
                    request: Some(request),
                    status,
                    bytes_transferred: 0,
                    used_head_index: head_index,
                    used_len: 1,
                    detail: error,
                }
            }
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
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
        execution_smoke: pane_virtio_blk_execution_smoke(),
        service_smoke: pane_virtio_mmio_service_smoke(),
        primary_device: PaneVirtioDeviceSummary {
            id: "vda",
            virtio_device_id: VIRTIO_DEVICE_ID_BLOCK,
            purpose: "read-only Arch base disk first, then writable user disk queue support",
            status: "mmio-service-boundary-ready-whp-emulator-callbacks-pending",
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

pub(crate) fn pane_virtio_mmio_kernel_arg() -> String {
    format!("virtio_mmio.device=4K@0x{PANE_VIRTIO_MMIO_BASE_GPA:x}:{PANE_VIRTIO_MMIO_IRQ}")
}

pub(crate) fn pane_virtio_mmio_service_smoke() -> PaneVirtioMmioServiceSmoke {
    let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
    let mut memory = PaneSmokeGuestMemory::new(0x5000);
    configure_smoke_queue(&mut device);

    let read_outcome = service_virtio_mmio_access(
        &mut device,
        &mut memory,
        PaneVirtioMmioAccess {
            kind: PaneVirtioMmioAccessKind::Read,
            gpa: PANE_VIRTIO_MMIO_BASE_GPA,
            data: vec![0; 4],
        },
        |_request, _payload| Err("read should not execute queue".to_string()),
    );

    memory.write_u32(0x4000, VIRTIO_BLK_T_IN);
    memory.write_u32(0x4004, 0);
    memory.write_u64(0x4008, 8);
    write_smoke_descriptor(&mut memory, 0, 0x4000, 16, VIRTQ_DESC_F_NEXT, 1);
    write_smoke_descriptor(
        &mut memory,
        1,
        0x4100,
        512,
        VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
        2,
    );
    write_smoke_descriptor(&mut memory, 2, 0x4300, 1, VIRTQ_DESC_F_WRITE, 0);
    memory.write_u16(0x2002, 1);
    memory.write_u16(0x2004, 0);

    let notify_outcome = service_virtio_mmio_access(
        &mut device,
        &mut memory,
        PaneVirtioMmioAccess {
            kind: PaneVirtioMmioAccessKind::Write,
            gpa: PANE_VIRTIO_MMIO_BASE_GPA + VIRTIO_MMIO_QUEUE_NOTIFY_OFFSET,
            data: 0_u32.to_le_bytes().to_vec(),
        },
        |request, _payload| {
            if request.request_type == PaneVirtioBlkRequestType::In && request.sector == 8 {
                Ok(vec![0x3c; 512])
            } else {
                Err("unexpected service smoke request".to_string())
            }
        },
    );
    let execution = notify_outcome.queue_execution.as_ref();

    PaneVirtioMmioServiceSmoke {
        status: if read_outcome.accepted && notify_outcome.accepted {
            "mmio-service-boundary-ready"
        } else {
            "mmio-service-boundary-failed"
        },
        register_read_status: read_outcome.status,
        queue_notify_status: notify_outcome.status,
        queue_execution_status: execution
            .map(|execution| execution.status)
            .unwrap_or(VIRTIO_BLK_STATUS_IOERR),
        bytes_transferred: execution
            .map(|execution| execution.bytes_transferred)
            .unwrap_or(0),
    }
}

pub(crate) fn pane_virtio_blk_execution_smoke() -> PaneVirtioBlkExecutionSmoke {
    let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
    let mut memory = PaneSmokeGuestMemory::new(0x5000);
    configure_smoke_queue(&mut device);

    memory.write_u32(0x4000, VIRTIO_BLK_T_IN);
    memory.write_u32(0x4004, 0);
    memory.write_u64(0x4008, 4);
    write_smoke_descriptor(&mut memory, 0, 0x4000, 16, VIRTQ_DESC_F_NEXT, 1);
    write_smoke_descriptor(
        &mut memory,
        1,
        0x4100,
        512,
        VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
        2,
    );
    write_smoke_descriptor(&mut memory, 2, 0x4300, 1, VIRTQ_DESC_F_WRITE, 0);
    memory.write_u16(0x2002, 1);
    memory.write_u16(0x2004, 0);

    let execution = device.execute_available_block_request(&mut memory, |request, _payload| {
        if request.request_type == PaneVirtioBlkRequestType::In && request.sector == 4 {
            Ok(vec![0xa5; 512])
        } else {
            Err("unexpected smoke request".to_string())
        }
    });
    let request = execution.request.as_ref();

    PaneVirtioBlkExecutionSmoke {
        status: if execution.status == VIRTIO_BLK_STATUS_OK {
            "descriptor-chain-execution-ready"
        } else {
            "descriptor-chain-execution-failed"
        },
        request_type: request
            .map(|request| request.request_type)
            .unwrap_or(PaneVirtioBlkRequestType::Unsupported(u32::MAX)),
        sector: request.map(|request| request.sector).unwrap_or(0),
        bytes_transferred: execution.bytes_transferred,
        used_len: execution.used_len,
        interrupt_status: device.interrupt_status,
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

struct PaneSmokeGuestMemory {
    bytes: Vec<u8>,
}

impl PaneSmokeGuestMemory {
    fn new(size: usize) -> Self {
        Self {
            bytes: vec![0_u8; size],
        }
    }

    fn write_u16(&mut self, gpa: u64, value: u16) {
        let _ = self.write(gpa, &value.to_le_bytes());
    }

    fn write_u32(&mut self, gpa: u64, value: u32) {
        let _ = self.write(gpa, &value.to_le_bytes());
    }

    fn write_u64(&mut self, gpa: u64, value: u64) {
        let _ = self.write(gpa, &value.to_le_bytes());
    }
}

impl PaneGuestMemory for PaneSmokeGuestMemory {
    fn read(&self, gpa: u64, bytes: &mut [u8]) -> Result<(), String> {
        let start = gpa as usize;
        let end = start + bytes.len();
        if end > self.bytes.len() {
            return Err("smoke memory read out of bounds".to_string());
        }
        bytes.copy_from_slice(&self.bytes[start..end]);
        Ok(())
    }

    fn write(&mut self, gpa: u64, bytes: &[u8]) -> Result<(), String> {
        let start = gpa as usize;
        let end = start + bytes.len();
        if end > self.bytes.len() {
            return Err("smoke memory write out of bounds".to_string());
        }
        self.bytes[start..end].copy_from_slice(bytes);
        Ok(())
    }
}

fn configure_smoke_queue(device: &mut PaneVirtioMmioBlockDevice) {
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_NUM_OFFSET, 8);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET, 0x1000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET, 0x2000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET, 0x3000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_READY_OFFSET, 1);
}

fn write_smoke_descriptor(
    memory: &mut PaneSmokeGuestMemory,
    index: u16,
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
) {
    let descriptor = 0x1000 + u64::from(index) * 16;
    memory.write_u64(descriptor, addr);
    memory.write_u32(descriptor + 8, len);
    memory.write_u16(descriptor + 12, flags);
    memory.write_u16(descriptor + 14, next);
}

pub(crate) fn pane_virtio_mmio_contains_gpa(gpa: u64) -> bool {
    (PANE_VIRTIO_MMIO_BASE_GPA..PANE_VIRTIO_MMIO_BASE_GPA + PANE_VIRTIO_MMIO_SIZE_BYTES)
        .contains(&gpa)
}

pub(crate) fn pane_virtio_mmio_offset(gpa: u64) -> Option<u64> {
    pane_virtio_mmio_contains_gpa(gpa).then_some(gpa - PANE_VIRTIO_MMIO_BASE_GPA)
}

pub(crate) fn format_guest_physical_address(gpa: u64) -> String {
    format!("0x{gpa:08x}")
}

fn read_avail_head<M: PaneGuestMemory>(memory: &M, queue: &PaneVirtioQueueState) -> Option<u16> {
    let idx = read_u16(memory, queue.avail_ring_gpa + 2).ok()?;
    if idx == 0 {
        return None;
    }
    read_u16(memory, queue.avail_ring_gpa + 4).ok()
}

fn parse_virtio_blk_request<M: PaneGuestMemory>(
    memory: &M,
    queue: &PaneVirtioQueueState,
    head_index: u16,
) -> Result<PaneVirtioBlkRequest, String> {
    if head_index >= queue.size {
        return Err(format!(
            "descriptor head index {head_index} exceeds queue size {}",
            queue.size
        ));
    }

    let mut descriptors = Vec::new();
    let mut index = head_index;
    for _ in 0..queue.size {
        let descriptor = read_descriptor(memory, queue, index)?;
        let has_next = descriptor.has_next();
        descriptors.push(descriptor);
        if !has_next {
            break;
        }
        index = descriptor.next;
        if index >= queue.size {
            return Err(format!(
                "descriptor next index {index} exceeds queue size {}",
                queue.size
            ));
        }
    }

    if descriptors.len() < 2 {
        return Err("virtio-blk descriptor chain is too short".to_string());
    }

    let header = descriptors[0];
    if header.writable() || header.len < 16 {
        return Err("virtio-blk header descriptor is invalid".to_string());
    }

    let status = *descriptors
        .last()
        .expect("chain length checked before status descriptor");
    if !status.writable() || status.len < 1 {
        return Err("virtio-blk status descriptor is invalid".to_string());
    }

    let request_type = match read_u32(memory, header.addr)? {
        VIRTIO_BLK_T_IN => PaneVirtioBlkRequestType::In,
        VIRTIO_BLK_T_OUT => PaneVirtioBlkRequestType::Out,
        VIRTIO_BLK_T_FLUSH => PaneVirtioBlkRequestType::Flush,
        VIRTIO_BLK_T_GET_ID => PaneVirtioBlkRequestType::GetId,
        value => PaneVirtioBlkRequestType::Unsupported(value),
    };
    let sector = read_u64(memory, header.addr + 8)?;

    Ok(PaneVirtioBlkRequest {
        request_type,
        sector,
        data_descriptors: descriptors[1..descriptors.len() - 1].to_vec(),
        status_addr: status.addr,
        status_len: status.len,
    })
}

fn read_descriptor<M: PaneGuestMemory>(
    memory: &M,
    queue: &PaneVirtioQueueState,
    index: u16,
) -> Result<PaneVirtioDescriptor, String> {
    let addr = queue.desc_table_gpa + u64::from(index) * 16;
    Ok(PaneVirtioDescriptor {
        addr: read_u64(memory, addr)?,
        len: read_u32(memory, addr + 8)?,
        flags: read_u16(memory, addr + 12)?,
        next: read_u16(memory, addr + 14)?,
    })
}

fn read_request_payload<M: PaneGuestMemory>(
    memory: &M,
    request: &PaneVirtioBlkRequest,
) -> Result<Vec<u8>, String> {
    let mut payload = Vec::new();
    for descriptor in &request.data_descriptors {
        if descriptor.writable() {
            return Err("virtio-blk OUT data descriptor must be device-readable".to_string());
        }
        let mut bytes = vec![0_u8; descriptor.len as usize];
        memory.read(descriptor.addr, &mut bytes)?;
        payload.extend(bytes);
    }
    Ok(payload)
}

fn write_read_response<M: PaneGuestMemory>(
    memory: &mut M,
    request: &PaneVirtioBlkRequest,
    bytes: &[u8],
) -> u8 {
    let mut cursor = 0_usize;
    for descriptor in &request.data_descriptors {
        if !descriptor.writable() {
            return VIRTIO_BLK_STATUS_IOERR;
        }
        let len = descriptor.len as usize;
        let available = bytes.len().saturating_sub(cursor);
        let transfer = len.min(available);
        let mut buffer = vec![0_u8; len];
        buffer[..transfer].copy_from_slice(&bytes[cursor..cursor + transfer]);
        if memory.write(descriptor.addr, &buffer).is_err() {
            return VIRTIO_BLK_STATUS_IOERR;
        }
        cursor += transfer;
    }
    VIRTIO_BLK_STATUS_OK
}

fn write_used_entry<M: PaneGuestMemory>(
    memory: &mut M,
    queue: &PaneVirtioQueueState,
    head_index: u16,
    len: u32,
) -> Result<(), String> {
    let used_elem = queue.used_ring_gpa + 4;
    memory.write(used_elem, &u32::from(head_index).to_le_bytes())?;
    memory.write(used_elem + 4, &len.to_le_bytes())?;
    memory.write(queue.used_ring_gpa + 2, &1_u16.to_le_bytes())?;
    Ok(())
}

fn read_u16<M: PaneGuestMemory>(memory: &M, gpa: u64) -> Result<u16, String> {
    let mut bytes = [0_u8; 2];
    memory.read(gpa, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<M: PaneGuestMemory>(memory: &M, gpa: u64) -> Result<u32, String> {
    let mut bytes = [0_u8; 4];
    memory.read(gpa, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64<M: PaneGuestMemory>(memory: &M, gpa: u64) -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    memory.read(gpa, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
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
        pane_virtio_mmio_contains_gpa, pane_virtio_mmio_kernel_arg, pane_virtio_mmio_offset,
        pane_virtio_mmio_window, service_virtio_mmio_access, PaneGuestMemory,
        PaneVirtioBlkRequestType, PaneVirtioMmioAccess, PaneVirtioMmioAccessKind,
        PaneVirtioMmioBlockDevice, PaneVirtioMmioWriteResult, PANE_VIRTIO_BLK_QUEUE_SIZE,
        PANE_VIRTIO_MMIO_BASE_GPA, PANE_VIRTIO_MMIO_SIZE_BYTES, VIRTIO_BLK_STATUS_IOERR,
        VIRTIO_BLK_STATUS_OK, VIRTIO_DEVICE_ID_BLOCK, VIRTIO_MMIO_MAGIC_VALUE,
        VIRTIO_MMIO_VERSION_MODERN,
    };

    struct TestGuestMemory {
        base: u64,
        bytes: Vec<u8>,
    }

    impl TestGuestMemory {
        fn new(size: usize) -> Self {
            Self {
                base: 0,
                bytes: vec![0_u8; size],
            }
        }

        fn write_u16(&mut self, gpa: u64, value: u16) {
            self.write(gpa, &value.to_le_bytes()).unwrap();
        }

        fn write_u32(&mut self, gpa: u64, value: u32) {
            self.write(gpa, &value.to_le_bytes()).unwrap();
        }

        fn write_u64(&mut self, gpa: u64, value: u64) {
            self.write(gpa, &value.to_le_bytes()).unwrap();
        }

        fn read_bytes(&self, gpa: u64, len: usize) -> Vec<u8> {
            let mut bytes = vec![0_u8; len];
            self.read(gpa, &mut bytes).unwrap();
            bytes
        }
    }

    impl PaneGuestMemory for TestGuestMemory {
        fn read(&self, gpa: u64, bytes: &mut [u8]) -> Result<(), String> {
            let start = gpa
                .checked_sub(self.base)
                .ok_or_else(|| "guest read before memory base".to_string())?
                as usize;
            let end = start + bytes.len();
            if end > self.bytes.len() {
                return Err("guest read out of bounds".to_string());
            }
            bytes.copy_from_slice(&self.bytes[start..end]);
            Ok(())
        }

        fn write(&mut self, gpa: u64, bytes: &[u8]) -> Result<(), String> {
            let start = gpa
                .checked_sub(self.base)
                .ok_or_else(|| "guest write before memory base".to_string())?
                as usize;
            let end = start + bytes.len();
            if end > self.bytes.len() {
                return Err("guest write out of bounds".to_string());
            }
            self.bytes[start..end].copy_from_slice(bytes);
            Ok(())
        }
    }

    fn configure_queue(device: &mut PaneVirtioMmioBlockDevice) {
        assert_eq!(
            device.write_u32(0x038, 8),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x080, 0x1000),
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
    }

    fn write_descriptor(
        memory: &mut TestGuestMemory,
        index: u16,
        addr: u64,
        len: u32,
        flags: u16,
        next: u16,
    ) {
        let descriptor = 0x1000 + u64::from(index) * 16;
        memory.write_u64(descriptor, addr);
        memory.write_u32(descriptor + 8, len);
        memory.write_u16(descriptor + 12, flags);
        memory.write_u16(descriptor + 14, next);
    }

    fn publish_head(memory: &mut TestGuestMemory, head: u16) {
        memory.write_u16(0x2002, 1);
        memory.write_u16(0x2004, head);
    }

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
    fn virtio_mmio_access_service_reads_registers_by_guest_address() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Read,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA,
                data: vec![0; 4],
            },
            |_request, _payload| unreachable!("register reads must not execute queues"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "register-read");
        assert_eq!(outcome.offset, Some(0));
        assert_eq!(outcome.read_data, VIRTIO_MMIO_MAGIC_VALUE.to_le_bytes());
        assert_eq!(
            pane_virtio_mmio_offset(PANE_VIRTIO_MMIO_BASE_GPA + 0x034),
            Some(0x034)
        );
    }

    #[test]
    fn virtio_mmio_access_service_rejects_non_register_widths() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x038,
                data: vec![8],
            },
            |_request, _payload| unreachable!("invalid widths must not execute queues"),
        );

        assert!(!outcome.accepted);
        assert_eq!(outcome.status, "unsupported-width");
        assert_eq!(outcome.offset, Some(0x038));
    }

    #[test]
    fn virtio_mmio_access_service_executes_queue_notify() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 2);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 1 | 2, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x050,
                data: 0_u32.to_le_bytes().to_vec(),
            },
            |request, _payload| {
                assert_eq!(request.request_type, PaneVirtioBlkRequestType::In);
                assert_eq!(request.sector, 2);
                Ok(vec![0x5a; 512])
            },
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-executed");
        assert_eq!(
            outcome.write_result,
            Some(PaneVirtioMmioWriteResult::QueueNotified(0))
        );
        assert_eq!(
            outcome
                .queue_execution
                .as_ref()
                .map(|execution| execution.status),
            Some(VIRTIO_BLK_STATUS_OK)
        );
        assert_eq!(memory.read_bytes(0x4100, 4), vec![0x5a; 4]);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_OK]);
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
    fn virtio_blk_executes_split_queue_read_request() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 4);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let execution = device.execute_available_block_request(&mut memory, |request, payload| {
            assert_eq!(request.request_type, PaneVirtioBlkRequestType::In);
            assert_eq!(request.sector, 4);
            assert!(payload.is_none());
            Ok(vec![0x5a; 512])
        });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_OK);
        assert_eq!(execution.bytes_transferred, 512);
        assert_eq!(execution.used_head_index, 0);
        assert_eq!(memory.read_bytes(0x4100, 4), vec![0x5a; 4]);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_OK]);
        assert_eq!(memory.read_bytes(0x3004, 4), 0_u32.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3008, 4), 513_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
    }

    #[test]
    fn virtio_blk_executes_split_queue_write_request() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 1);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 7);
        memory.write(0x4100, &[0x33; 512]).unwrap();
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 1, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let execution = device.execute_available_block_request(&mut memory, |request, payload| {
            assert_eq!(request.request_type, PaneVirtioBlkRequestType::Out);
            assert_eq!(request.sector, 7);
            assert_eq!(payload.expect("write payload")[..4], [0x33; 4]);
            Ok(Vec::new())
        });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_OK);
        assert_eq!(execution.bytes_transferred, 0);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_OK]);
        assert_eq!(memory.read_bytes(0x3008, 4), 1_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
    }

    #[test]
    fn virtio_blk_writes_error_status_for_bad_service_result() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 4);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let execution = device
            .execute_available_block_request(&mut memory, |_request, _payload| {
                Err("storage backend unavailable".to_string())
            });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_IOERR);
        assert_eq!(execution.bytes_transferred, 0);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_IOERR]);
        assert_eq!(memory.read_bytes(0x3008, 4), 1_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
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
            window.execution_smoke.status,
            "descriptor-chain-execution-ready"
        );
        assert_eq!(
            window.execution_smoke.request_type,
            PaneVirtioBlkRequestType::In
        );
        assert_eq!(window.execution_smoke.bytes_transferred, 512);
        assert_eq!(window.execution_smoke.interrupt_status, 1);
        assert_eq!(window.service_smoke.status, "mmio-service-boundary-ready");
        assert_eq!(window.service_smoke.register_read_status, "register-read");
        assert_eq!(
            window.service_smoke.queue_notify_status,
            "queue-notify-executed"
        );
        assert_eq!(window.service_smoke.bytes_transferred, 512);
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

    #[test]
    fn pane_virtio_mmio_kernel_arg_matches_linux_discovery_syntax() {
        assert_eq!(
            pane_virtio_mmio_kernel_arg(),
            "virtio_mmio.device=4K@0xdfc0000:5"
        );
    }
}
