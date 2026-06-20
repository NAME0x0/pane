use serde::Serialize;
use std::sync::Arc;
use virtio_queue::{Queue, QueueT};
use vm_memory::{Bytes, GuestAddress, GuestMemory, GuestMemoryMmap};

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
pub(crate) const VIRTIO_BLK_F_RO: u64 = 1 << 5;
pub(crate) const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
pub(crate) const VIRTIO_BLK_F_FLUSH: u64 = 1 << 9;
pub(crate) const VIRTIO_F_VERSION_1: u64 = 1 << 32;
pub(crate) const VIRTIO_CONFIG_S_DRIVER_OK: u32 = 4;
pub(crate) const VIRTIO_CONFIG_S_FEATURES_OK: u32 = 8;

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
    pub(crate) next_avail_index: u16,
    pub(crate) used_index: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PaneVirtioMmioBlockDevice {
    pub(crate) device_id: u32,
    pub(crate) vendor_id: u32,
    pub(crate) device_features: u64,
    pub(crate) driver_features: u64,
    pub(crate) unsupported_driver_features: u64,
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
    pub(crate) queue_execution_count: usize,
    pub(crate) detail: String,
}

pub(crate) trait PaneGuestMemory {
    fn read(&self, gpa: u64, bytes: &mut [u8]) -> Result<(), String>;
    fn write(&mut self, gpa: u64, bytes: &[u8]) -> Result<(), String>;
    fn rust_vmm_memory(&self) -> Option<&GuestMemoryMmap<()>> {
        None
    }
}

pub(crate) struct PaneMmapGuestMemory {
    memory: Arc<GuestMemoryMmap<()>>,
    base: GuestAddress,
    len: usize,
}

impl PaneMmapGuestMemory {
    pub(crate) fn new(base: u64, len: usize) -> Result<Self, String> {
        Self::from_ranges(&[(base, len)])?.view(base, len)
    }

    pub(crate) fn from_ranges(ranges: &[(u64, usize)]) -> Result<Self, String> {
        let mut ranges = ranges
            .iter()
            .map(|(base, len)| (GuestAddress(*base), *len))
            .collect::<Vec<_>>();
        ranges.sort_by_key(|(base, _)| base.0);
        let memory = GuestMemoryMmap::from_ranges(&ranges)
            .map_err(|error| format!("failed to allocate guest memory: {error}"))?;
        let (base, len) = ranges
            .first()
            .copied()
            .ok_or_else(|| "guest memory requires at least one range".to_string())?;
        Ok(Self {
            memory: Arc::new(memory),
            base,
            len,
        })
    }

    pub(crate) fn view(&self, base: u64, len: usize) -> Result<Self, String> {
        let base = GuestAddress(base);
        if !self.memory.check_range(base, len) {
            return Err("guest memory view is outside mapped ranges".to_string());
        }
        Ok(Self {
            memory: Arc::clone(&self.memory),
            base,
            len,
        })
    }

    pub(crate) fn rust_vmm_memory(&self) -> &GuestMemoryMmap<()> {
        &self.memory
    }

    pub(crate) fn host_address(&self) -> *mut u8 {
        self.memory
            .get_host_address(self.base)
            .expect("Pane guest-memory base must remain mapped")
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.host_address(), self.len) }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.host_address(), self.len) }
    }
}

impl PaneGuestMemory for PaneMmapGuestMemory {
    fn read(&self, gpa: u64, bytes: &mut [u8]) -> Result<(), String> {
        self.memory
            .read_slice(bytes, GuestAddress(gpa))
            .map_err(|error| format!("guest memory read failed: {error}"))
    }

    fn write(&mut self, gpa: u64, bytes: &[u8]) -> Result<(), String> {
        self.memory
            .write_slice(bytes, GuestAddress(gpa))
            .map_err(|error| format!("guest memory write failed: {error}"))
    }

    fn rust_vmm_memory(&self) -> Option<&GuestMemoryMmap<()>> {
        Some(&self.memory)
    }
}

pub(crate) fn service_virtio_mmio_access<M, F>(
    device: &mut PaneVirtioMmioBlockDevice,
    memory: &mut M,
    access: PaneVirtioMmioAccess,
    mut service: F,
) -> PaneVirtioMmioAccessOutcome
where
    M: PaneGuestMemory,
    F: FnMut(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
{
    let Some(offset) = pane_virtio_mmio_offset(access.gpa) else {
        return PaneVirtioMmioAccessOutcome {
            accepted: false,
            status: "outside-virtio-mmio-window",
            offset: None,
            read_data: Vec::new(),
            write_result: None,
            queue_execution: None,
            queue_execution_count: 0,
            detail: format!(
                "GPA {} is outside the Pane virtio-MMIO window.",
                format_guest_physical_address(access.gpa)
            ),
        };
    };

    match access.kind {
        PaneVirtioMmioAccessKind::Read => {
            let access_width = if access.data.is_empty() {
                4
            } else {
                access.data.len()
            };
            match device.read_access_bytes(offset, access_width) {
                Some(read_data) => PaneVirtioMmioAccessOutcome {
                    accepted: true,
                    status: if offset >= VIRTIO_MMIO_CONFIG_OFFSET {
                        "config-read"
                    } else {
                        "register-read"
                    },
                    offset: Some(offset),
                    read_data,
                    write_result: None,
                    queue_execution: None,
                    queue_execution_count: 0,
                    detail: format!(
                        "Read {access_width}-byte virtio-MMIO access at offset 0x{offset:03x}."
                    ),
                },
                None if !matches!(access_width, 1 | 2 | 4 | 8) => {
                    unsupported_mmio_width(offset, access_width)
                }
                None if offset < VIRTIO_MMIO_CONFIG_OFFSET && access_width != 4 => {
                    unsupported_mmio_width(offset, access_width)
                }
                None => PaneVirtioMmioAccessOutcome {
                    accepted: false,
                    status: "unknown-register",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: None,
                    queue_execution: None,
                    queue_execution_count: 0,
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
                PaneVirtioMmioWriteResult::Accepted => {
                    let status = if offset == VIRTIO_MMIO_INTERRUPT_ACK_OFFSET {
                        "interrupt-ack"
                    } else {
                        "register-write"
                    };
                    PaneVirtioMmioAccessOutcome {
                        accepted: true,
                        status,
                        offset: Some(offset),
                        read_data: Vec::new(),
                        write_result: Some(write_result),
                        queue_execution: None,
                        queue_execution_count: 0,
                        detail: format!("Wrote virtio-MMIO register offset 0x{offset:03x}."),
                    }
                }
                PaneVirtioMmioWriteResult::QueueNotified(queue_index) => {
                    if queue_index != 0 {
                        return PaneVirtioMmioAccessOutcome {
                            accepted: true,
                            status: "queue-notify-ignored",
                            offset: Some(offset),
                            read_data: Vec::new(),
                            write_result: Some(write_result),
                            queue_execution: None,
                            queue_execution_count: 0,
                            detail: format!(
                                "Ignored queue notify {queue_index}; Pane virtio-blk currently exposes only queue 0."
                            ),
                        };
                    }
                    if !device.driver_ready_for_queue_service() {
                        return PaneVirtioMmioAccessOutcome {
                            accepted: true,
                            status: "queue-notify-not-ready",
                            offset: Some(offset),
                            read_data: Vec::new(),
                            write_result: Some(write_result),
                            queue_execution: None,
                            queue_execution_count: 0,
                            detail: "Ignored queue notify 0 because the virtio driver has not reached DRIVER_OK with a ready queue.".to_string(),
                        };
                    }
                    let executions = device.execute_available_block_requests(memory, &mut service);
                    let execution = executions.last().cloned();
                    let queue_execution_count = executions.len();
                    let status = if queue_execution_count == 0 {
                        "queue-notify-empty"
                    } else if executions
                        .iter()
                        .all(|execution| execution.status == VIRTIO_BLK_STATUS_OK)
                    {
                        "queue-notify-executed"
                    } else {
                        "queue-notify-completed-with-guest-error"
                    };
                    PaneVirtioMmioAccessOutcome {
                        accepted: true,
                        status,
                        offset: Some(offset),
                        read_data: Vec::new(),
                        write_result: Some(write_result),
                        queue_execution: execution,
                        queue_execution_count,
                        detail: format!(
                            "Queue notify {queue_index} executed {queue_execution_count} request(s)."
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
                    queue_execution_count: 0,
                    detail: reason.to_string(),
                },
                PaneVirtioMmioWriteResult::Ignored => PaneVirtioMmioAccessOutcome {
                    accepted: false,
                    status: "register-write-ignored",
                    offset: Some(offset),
                    read_data: Vec::new(),
                    write_result: Some(write_result),
                    queue_execution: None,
                    queue_execution_count: 0,
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
        queue_execution_count: 0,
        detail: format!("Pane virtio-MMIO currently supports 4-byte accesses, got {width}."),
    }
}

impl PaneVirtioMmioBlockDevice {
    pub(crate) fn new(logical_size_bytes: u64, readonly: bool) -> Self {
        let device_features = VIRTIO_F_VERSION_1
            | VIRTIO_BLK_F_BLK_SIZE
            | VIRTIO_BLK_F_FLUSH
            | if readonly { VIRTIO_BLK_F_RO } else { 0 };
        Self {
            device_id: VIRTIO_DEVICE_ID_BLOCK,
            vendor_id: PANE_VENDOR_ID,
            device_features,
            driver_features: 0,
            unsupported_driver_features: 0,
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
                next_avail_index: 0,
                used_index: 0,
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
        F: FnMut(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
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

        let head_index = match read_available_head(memory, &self.queue) {
            Ok(Some(head_index)) => head_index,
            Ok(None) => {
                return PaneVirtioBlkExecution {
                    request: None,
                    status: VIRTIO_BLK_STATUS_IOERR,
                    bytes_transferred: 0,
                    used_head_index: 0,
                    used_len: 0,
                    detail: "available ring does not contain a new request".to_string(),
                };
            }
            Err(error) => {
                return PaneVirtioBlkExecution {
                    request: None,
                    status: VIRTIO_BLK_STATUS_IOERR,
                    bytes_transferred: 0,
                    used_head_index: 0,
                    used_len: 0,
                    detail: error,
                };
            }
        };

        match parse_virtio_blk_request(memory, &self.queue, head_index) {
            Ok(request) => self.execute_parsed_block_request(memory, request, head_index, service),
            Err(error) => {
                let _ = write_used_entry(memory, &self.queue, head_index, 0);
                self.queue.next_avail_index = self.queue.next_avail_index.wrapping_add(1);
                self.queue.used_index = self.queue.used_index.wrapping_add(1);
                self.interrupt_status |= 1;
                PaneVirtioBlkExecution {
                    request: None,
                    status: VIRTIO_BLK_STATUS_IOERR,
                    bytes_transferred: 0,
                    used_head_index: head_index,
                    used_len: 0,
                    detail: error,
                }
            }
        }
    }

    pub(crate) fn execute_available_block_requests<M, F>(
        &mut self,
        memory: &mut M,
        service: &mut F,
    ) -> Vec<PaneVirtioBlkExecution>
    where
        M: PaneGuestMemory,
        F: FnMut(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        let mut executions = Vec::new();
        for _ in 0..self.queue.size {
            let execution = self.execute_available_block_request(memory, &mut *service);
            if execution.request.is_none() {
                if execution.used_len > 0 || execution.detail.contains("descriptor") {
                    executions.push(execution);
                }
                break;
            }
            let terminal_error = execution.status != VIRTIO_BLK_STATUS_OK;
            executions.push(execution);
            if terminal_error {
                break;
            }
        }
        executions
    }

    fn execute_parsed_block_request<M, F>(
        &mut self,
        memory: &mut M,
        request: PaneVirtioBlkRequest,
        head_index: u16,
        mut service: F,
    ) -> PaneVirtioBlkExecution
    where
        M: PaneGuestMemory,
        F: FnMut(&PaneVirtioBlkRequest, Option<&[u8]>) -> Result<Vec<u8>, String>,
    {
        if request.request_type == PaneVirtioBlkRequestType::Out && self.config.readonly {
            let _ = memory.write(request.status_addr, &[VIRTIO_BLK_STATUS_IOERR]);
            let _ = write_used_entry(memory, &self.queue, head_index, 1);
            self.queue.next_avail_index = self.queue.next_avail_index.wrapping_add(1);
            self.queue.used_index = self.queue.used_index.wrapping_add(1);
            self.interrupt_status |= 1;
            return PaneVirtioBlkExecution {
                request: Some(request),
                status: VIRTIO_BLK_STATUS_IOERR,
                bytes_transferred: 0,
                used_head_index: head_index,
                used_len: 1,
                detail: "virtio-blk write denied on read-only device".to_string(),
            };
        }

        let write_payload = match request.request_type {
            PaneVirtioBlkRequestType::Out => match read_request_payload(memory, &request) {
                Ok(payload) => Some(payload),
                Err(error) => {
                    let _ = memory.write(request.status_addr, &[VIRTIO_BLK_STATUS_IOERR]);
                    let _ = write_used_entry(memory, &self.queue, head_index, 1);
                    self.queue.next_avail_index = self.queue.next_avail_index.wrapping_add(1);
                    self.queue.used_index = self.queue.used_index.wrapping_add(1);
                    self.interrupt_status |= 1;
                    return PaneVirtioBlkExecution {
                        request: Some(request),
                        status: VIRTIO_BLK_STATUS_IOERR,
                        bytes_transferred: 0,
                        used_head_index: head_index,
                        used_len: 1,
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
                let (status, bytes_transferred) = match request.request_type {
                    PaneVirtioBlkRequestType::In | PaneVirtioBlkRequestType::GetId => {
                        write_read_response(memory, &request, &bytes)
                    }
                    PaneVirtioBlkRequestType::Out | PaneVirtioBlkRequestType::Flush => {
                        (VIRTIO_BLK_STATUS_OK, 0)
                    }
                    PaneVirtioBlkRequestType::Unsupported(_) => (VIRTIO_BLK_STATUS_UNSUPP, 0),
                };
                let _ = memory.write(request.status_addr, &[status]);
                let used_len = if status == VIRTIO_BLK_STATUS_OK {
                    bytes_transferred + 1
                } else {
                    1
                };
                let _ = write_used_entry(memory, &self.queue, head_index, used_len);
                self.queue.next_avail_index = self.queue.next_avail_index.wrapping_add(1);
                self.queue.used_index = self.queue.used_index.wrapping_add(1);
                self.interrupt_status |= 1;
                PaneVirtioBlkExecution {
                    request: Some(request),
                    status,
                    bytes_transferred,
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
                self.queue.next_avail_index = self.queue.next_avail_index.wrapping_add(1);
                self.queue.used_index = self.queue.used_index.wrapping_add(1);
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
            VIRTIO_MMIO_DEVICE_FEATURES_SEL_OFFSET => Some(self.device_features_select),
            VIRTIO_MMIO_DRIVER_FEATURES_OFFSET => match self.driver_features_select {
                0 => Some(self.driver_features as u32),
                1 => Some((self.driver_features >> 32) as u32),
                _ => Some(0),
            },
            VIRTIO_MMIO_DRIVER_FEATURES_SEL_OFFSET => Some(self.driver_features_select),
            VIRTIO_MMIO_QUEUE_SEL_OFFSET => Some(u32::from(self.queue_select)),
            VIRTIO_MMIO_QUEUE_NUM_MAX_OFFSET => Some(u32::from(self.selected_queue_max_size())),
            VIRTIO_MMIO_QUEUE_NUM_OFFSET => Some(u32::from(self.selected_queue_size())),
            VIRTIO_MMIO_QUEUE_READY_OFFSET => Some(u32::from(self.selected_queue_ready())),
            VIRTIO_MMIO_QUEUE_NOTIFY_OFFSET => self.queue.last_notify,
            VIRTIO_MMIO_INTERRUPT_STATUS_OFFSET => Some(self.interrupt_status),
            VIRTIO_MMIO_STATUS_OFFSET => Some(self.status),
            VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET => Some(self.selected_queue_desc_table_gpa() as u32),
            VIRTIO_MMIO_QUEUE_DESC_HIGH_OFFSET => {
                Some((self.selected_queue_desc_table_gpa() >> 32) as u32)
            }
            VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET => Some(self.selected_queue_avail_ring_gpa() as u32),
            VIRTIO_MMIO_QUEUE_AVAIL_HIGH_OFFSET => {
                Some((self.selected_queue_avail_ring_gpa() >> 32) as u32)
            }
            VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET => Some(self.selected_queue_used_ring_gpa() as u32),
            VIRTIO_MMIO_QUEUE_USED_HIGH_OFFSET => {
                Some((self.selected_queue_used_ring_gpa() >> 32) as u32)
            }
            VIRTIO_MMIO_CONFIG_GENERATION_OFFSET => Some(self.config_generation),
            offset if offset >= VIRTIO_MMIO_CONFIG_OFFSET => self.read_config_u32(offset),
            _ => None,
        }
    }

    pub(crate) fn read_access_bytes(&self, offset: u64, width: usize) -> Option<Vec<u8>> {
        if !matches!(width, 1 | 2 | 4 | 8) {
            return None;
        }

        if offset >= VIRTIO_MMIO_CONFIG_OFFSET {
            return self.read_config_bytes(offset - VIRTIO_MMIO_CONFIG_OFFSET, width);
        }

        if width != 4 {
            return None;
        }

        self.read_u32(offset)
            .map(|value| value.to_le_bytes().to_vec())
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
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                if value == 0 || value > u32::from(self.queue.max_size) || !value.is_power_of_two()
                {
                    PaneVirtioMmioWriteResult::Rejected("invalid virtqueue size")
                } else {
                    self.queue.size = value as u16;
                    PaneVirtioMmioWriteResult::Accepted
                }
            }
            VIRTIO_MMIO_QUEUE_READY_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                match value {
                    0 => self.reset_selected_queue_runtime(),
                    1 => self.queue.ready = true,
                    _ => return PaneVirtioMmioWriteResult::Rejected("invalid queue ready value"),
                }
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
                    self.apply_driver_status(value);
                }
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.desc_table_gpa = combine_addr_low(self.queue.desc_table_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_DESC_HIGH_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.desc_table_gpa = combine_addr_high(self.queue.desc_table_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.avail_ring_gpa = combine_addr_low(self.queue.avail_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_AVAIL_HIGH_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.avail_ring_gpa = combine_addr_high(self.queue.avail_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.used_ring_gpa = combine_addr_low(self.queue.used_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            VIRTIO_MMIO_QUEUE_USED_HIGH_OFFSET => {
                if !self.selected_queue_exists() {
                    return PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index");
                }
                if !self.selected_queue_configuration_mutable() {
                    return PaneVirtioMmioWriteResult::Rejected("virtqueue is ready");
                }
                self.queue.used_ring_gpa = combine_addr_high(self.queue.used_ring_gpa, value);
                PaneVirtioMmioWriteResult::Accepted
            }
            _ => PaneVirtioMmioWriteResult::Ignored,
        }
    }

    fn set_driver_features(&mut self, value: u32) {
        let Some((bank_mask, raw_features)) =
            selected_feature_bank(self.driver_features_select, value)
        else {
            return;
        };
        let accepted = raw_features & self.device_features & bank_mask;
        let unsupported = raw_features & !self.device_features & bank_mask;
        self.driver_features = (self.driver_features & !bank_mask) | accepted;
        self.unsupported_driver_features =
            (self.unsupported_driver_features & !bank_mask) | unsupported;
    }

    fn apply_driver_status(&mut self, value: u32) {
        let mut accepted_status = value;
        if (value & VIRTIO_CONFIG_S_FEATURES_OK) != 0 && self.unsupported_driver_features != 0 {
            accepted_status &= !VIRTIO_CONFIG_S_FEATURES_OK;
        }

        let status_after_features = self.status | accepted_status;
        if (value & VIRTIO_CONFIG_S_DRIVER_OK) != 0
            && (status_after_features & VIRTIO_CONFIG_S_FEATURES_OK) == 0
        {
            accepted_status &= !VIRTIO_CONFIG_S_DRIVER_OK;
        }

        self.status |= accepted_status;
    }

    fn read_config_u32(&self, offset: u64) -> Option<u32> {
        let bytes = self.read_config_bytes(offset - VIRTIO_MMIO_CONFIG_OFFSET, 4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_config_bytes(&self, config_offset: u64, width: usize) -> Option<Vec<u8>> {
        let mut config = [0_u8; 0x40];
        config[0x00..0x08].copy_from_slice(&self.config.capacity_sectors.to_le_bytes());
        config[0x14..0x18].copy_from_slice(&(self.config.sector_size_bytes as u32).to_le_bytes());

        let start = usize::try_from(config_offset).ok()?;
        let end = start.checked_add(width)?;
        config.get(start..end).map(|bytes| bytes.to_vec())
    }

    fn selected_queue_exists(&self) -> bool {
        self.queue_select == 0
    }

    fn selected_queue_max_size(&self) -> u16 {
        if self.selected_queue_exists() {
            self.queue.max_size
        } else {
            0
        }
    }

    fn selected_queue_size(&self) -> u16 {
        if self.selected_queue_exists() {
            self.queue.size
        } else {
            0
        }
    }

    fn selected_queue_ready(&self) -> bool {
        self.selected_queue_exists() && self.queue.ready
    }

    fn selected_queue_configuration_mutable(&self) -> bool {
        self.selected_queue_exists() && !self.queue.ready
    }

    fn selected_queue_desc_table_gpa(&self) -> u64 {
        if self.selected_queue_exists() {
            self.queue.desc_table_gpa
        } else {
            0
        }
    }

    fn selected_queue_avail_ring_gpa(&self) -> u64 {
        if self.selected_queue_exists() {
            self.queue.avail_ring_gpa
        } else {
            0
        }
    }

    fn selected_queue_used_ring_gpa(&self) -> u64 {
        if self.selected_queue_exists() {
            self.queue.used_ring_gpa
        } else {
            0
        }
    }

    fn reset_selected_queue_runtime(&mut self) {
        self.queue.ready = false;
        self.queue.last_notify = None;
        self.queue.next_avail_index = 0;
        self.queue.used_index = 0;
    }

    fn driver_ready_for_queue_service(&self) -> bool {
        (self.status & VIRTIO_CONFIG_S_DRIVER_OK) != 0 && self.selected_queue_ready()
    }

    fn reset_driver_state(&mut self) {
        self.driver_features = 0;
        self.unsupported_driver_features = 0;
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
        self.queue.next_avail_index = 0;
        self.queue.used_index = 0;
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
    let mut memory = PaneMmapGuestMemory::new(0, 0x5000)
        .expect("fixed virtio execution smoke memory must allocate");
    configure_smoke_queue(&mut device);
    let _ = device.write_u32(
        VIRTIO_MMIO_STATUS_OFFSET,
        1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK,
    );

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
    let mut memory = PaneMmapGuestMemory::new(0, 0x5000)
        .expect("fixed virtio service smoke memory must allocate");
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

impl PaneMmapGuestMemory {
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

fn configure_smoke_queue(device: &mut PaneVirtioMmioBlockDevice) {
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_NUM_OFFSET, 8);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_DESC_LOW_OFFSET, 0x1000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_AVAIL_LOW_OFFSET, 0x2000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_USED_LOW_OFFSET, 0x3000);
    let _ = device.write_u32(VIRTIO_MMIO_QUEUE_READY_OFFSET, 1);
}

fn write_smoke_descriptor(
    memory: &mut PaneMmapGuestMemory,
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

fn read_available_head<M: PaneGuestMemory>(
    memory: &M,
    queue: &PaneVirtioQueueState,
) -> Result<Option<u16>, String> {
    if let Some(memory) = memory.rust_vmm_memory() {
        let mut queue = rust_vmm_queue(queue)?;
        return Ok(queue
            .pop_descriptor_chain(memory)
            .map(|chain| chain.head_index()));
    }
    if queue.avail_ring_gpa == 0 {
        return Ok(None);
    }
    let available_index = read_u16(memory, queue.avail_ring_gpa + 2)?;
    if available_index == queue.next_avail_index {
        return Ok(None);
    }
    let ring_index = queue.next_avail_index % queue.size;
    read_u16(memory, queue.avail_ring_gpa + 4 + u64::from(ring_index) * 2).map(Some)
}

fn parse_virtio_blk_request<M: PaneGuestMemory>(
    memory: &M,
    queue: &PaneVirtioQueueState,
    head_index: u16,
) -> Result<PaneVirtioBlkRequest, String> {
    if let Some(guest_memory) = memory.rust_vmm_memory() {
        let mut rust_queue = rust_vmm_queue(queue)?;
        let chain = rust_queue
            .pop_descriptor_chain(guest_memory)
            .ok_or_else(|| "available ring does not contain a new request".to_string())?;
        if chain.head_index() != head_index {
            return Err("virtio-queue returned a different descriptor head".to_string());
        }
        let descriptors = chain
            .map(|descriptor| PaneVirtioDescriptor {
                addr: descriptor.addr().0,
                len: descriptor.len(),
                flags: descriptor.flags(),
                next: descriptor.next(),
            })
            .collect::<Vec<_>>();
        return parse_virtio_blk_descriptors(memory, descriptors);
    }

    if head_index >= queue.size {
        return Err(format!(
            "descriptor head index {head_index} exceeds queue size {}",
            queue.size
        ));
    }

    let mut descriptors = Vec::new();
    let mut seen_indexes = Vec::new();
    let mut index = head_index;
    for _ in 0..queue.size {
        if seen_indexes.contains(&index) {
            return Err(format!(
                "virtio-blk descriptor chain contains a cycle at index {index}"
            ));
        }
        seen_indexes.push(index);
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
    if descriptors
        .last()
        .is_some_and(|descriptor| descriptor.has_next())
    {
        return Err("virtio-blk descriptor chain is unterminated".to_string());
    }

    parse_virtio_blk_descriptors(memory, descriptors)
}

fn parse_virtio_blk_descriptors<M: PaneGuestMemory>(
    memory: &M,
    descriptors: Vec<PaneVirtioDescriptor>,
) -> Result<PaneVirtioBlkRequest, String> {
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
) -> (u8, u32) {
    let mut cursor = 0_usize;
    for descriptor in &request.data_descriptors {
        if !descriptor.writable() {
            return (VIRTIO_BLK_STATUS_IOERR, cursor as u32);
        }
        let len = descriptor.len as usize;
        let available = bytes.len().saturating_sub(cursor);
        let transfer = len.min(available);
        let mut buffer = vec![0_u8; len];
        buffer[..transfer].copy_from_slice(&bytes[cursor..cursor + transfer]);
        if memory.write(descriptor.addr, &buffer).is_err() {
            return (VIRTIO_BLK_STATUS_IOERR, cursor as u32);
        }
        cursor += transfer;
    }
    (VIRTIO_BLK_STATUS_OK, cursor as u32)
}

fn write_used_entry<M: PaneGuestMemory>(
    memory: &mut M,
    queue: &PaneVirtioQueueState,
    head_index: u16,
    len: u32,
) -> Result<(), String> {
    if let Some(memory) = memory.rust_vmm_memory() {
        let mut queue = rust_vmm_queue(queue)?;
        return queue
            .add_used(memory, head_index, len)
            .map_err(|error| format!("virtio-queue could not publish used entry: {error}"));
    }
    let ring_index = queue.used_index % queue.size;
    let used_elem = queue.used_ring_gpa + 4 + u64::from(ring_index) * 8;
    memory.write(used_elem, &u32::from(head_index).to_le_bytes())?;
    memory.write(used_elem + 4, &len.to_le_bytes())?;
    memory.write(
        queue.used_ring_gpa + 2,
        &queue.used_index.wrapping_add(1).to_le_bytes(),
    )?;
    Ok(())
}

fn rust_vmm_queue(state: &PaneVirtioQueueState) -> Result<Queue, String> {
    let mut queue = Queue::new(state.max_size)
        .map_err(|error| format!("virtio-queue rejected max size: {error}"))?;
    queue.set_size(state.size);
    queue.set_ready(state.ready);
    queue.set_desc_table_address(
        Some(state.desc_table_gpa as u32),
        Some((state.desc_table_gpa >> 32) as u32),
    );
    queue.set_avail_ring_address(
        Some(state.avail_ring_gpa as u32),
        Some((state.avail_ring_gpa >> 32) as u32),
    );
    queue.set_used_ring_address(
        Some(state.used_ring_gpa as u32),
        Some((state.used_ring_gpa >> 32) as u32),
    );
    queue.set_next_avail(state.next_avail_index);
    queue.set_next_used(state.used_index);
    Ok(queue)
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

fn selected_feature_bank(select: u32, value: u32) -> Option<(u64, u64)> {
    match select {
        0 => Some((0x0000_0000_ffff_ffff, u64::from(value))),
        1 => Some((0xffff_ffff_0000_0000, u64::from(value) << 32)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        pane_virtio_mmio_contains_gpa, pane_virtio_mmio_kernel_arg, pane_virtio_mmio_offset,
        pane_virtio_mmio_window, service_virtio_mmio_access, PaneGuestMemory, PaneMmapGuestMemory,
        PaneVirtioBlkRequestType, PaneVirtioMmioAccess, PaneVirtioMmioAccessKind,
        PaneVirtioMmioBlockDevice, PaneVirtioMmioWriteResult, PANE_VIRTIO_BLK_QUEUE_SIZE,
        PANE_VIRTIO_MMIO_BASE_GPA, PANE_VIRTIO_MMIO_SIZE_BYTES, VIRTIO_BLK_F_BLK_SIZE,
        VIRTIO_BLK_F_FLUSH, VIRTIO_BLK_F_RO, VIRTIO_BLK_STATUS_IOERR, VIRTIO_BLK_STATUS_OK,
        VIRTIO_CONFIG_S_DRIVER_OK, VIRTIO_CONFIG_S_FEATURES_OK, VIRTIO_DEVICE_ID_BLOCK,
        VIRTIO_F_VERSION_1, VIRTIO_MMIO_CONFIG_OFFSET, VIRTIO_MMIO_INTERRUPT_ACK_OFFSET,
        VIRTIO_MMIO_MAGIC_VALUE, VIRTIO_MMIO_VERSION_MODERN,
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

    #[test]
    fn mmap_guest_memory_honors_nonzero_guest_base_and_bounds() {
        let mut memory = PaneMmapGuestMemory::new(0x2000, 0x1000).expect("mmap guest memory");

        memory.write(0x2020, &[1, 2, 3, 4]).unwrap();
        let mut bytes = [0; 4];
        memory.read(0x2020, &mut bytes).unwrap();

        assert_eq!(bytes, [1, 2, 3, 4]);
        assert!(!memory.host_address().is_null());
        assert_eq!(memory.len(), 0x1000);
        assert!(memory.read(0x1fff, &mut [0]).is_err());
        assert!(memory.write(0x3000, &[0]).is_err());
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

    fn activate_driver(device: &mut PaneVirtioMmioBlockDevice) {
        assert_eq!(
            device.write_u32(0x020, VIRTIO_BLK_F_BLK_SIZE as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x024, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x020, (VIRTIO_F_VERSION_1 >> 32) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x070, 1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x070, VIRTIO_CONFIG_S_DRIVER_OK),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.read_u32(0x070),
            Some(1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK)
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

    fn publish_heads(memory: &mut TestGuestMemory, heads: &[u16]) {
        memory.write_u16(0x2002, heads.len() as u16);
        for (index, head) in heads.iter().enumerate() {
            memory.write_u16(0x2004 + (index as u64 * 2), *head);
        }
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
    fn virtio_mmio_block_device_advertises_linux_compatible_features() {
        let mut readonly = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut writable = PaneVirtioMmioBlockDevice::new(1024 * 1024, false);

        assert_eq!(
            readonly.read_u32(0x010),
            Some((VIRTIO_BLK_F_RO | VIRTIO_BLK_F_BLK_SIZE | VIRTIO_BLK_F_FLUSH) as u32)
        );
        assert_eq!(
            writable.read_u32(0x010),
            Some((VIRTIO_BLK_F_BLK_SIZE | VIRTIO_BLK_F_FLUSH) as u32)
        );

        assert_eq!(
            readonly.write_u32(0x014, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            readonly.read_u32(0x010),
            Some((VIRTIO_F_VERSION_1 >> 32) as u32)
        );

        assert_eq!(
            writable.write_u32(0x024, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            writable.write_u32(0x020, (VIRTIO_F_VERSION_1 >> 32) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(writable.driver_features, VIRTIO_F_VERSION_1);
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
        assert_eq!(device.read_u32(0x030), Some(0));
        assert_eq!(device.read_u32(0x038), Some(128));
        assert_eq!(device.read_u32(0x044), Some(1));
        assert_eq!(device.read_u32(0x050), Some(0));
        assert_eq!(device.read_u32(0x080), Some(0x1000));
        assert_eq!(device.read_u32(0x084), Some(0x1));
        assert_eq!(device.read_u32(0x090), Some(0x2000));
        assert_eq!(device.read_u32(0x0a0), Some(0x3000));
    }

    #[test]
    fn virtio_mmio_block_device_reports_nonexistent_selected_queues_as_absent() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        configure_queue(&mut device);

        assert_eq!(
            device.write_u32(0x030, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x030), Some(1));
        assert_eq!(device.read_u32(0x034), Some(0));
        assert_eq!(device.read_u32(0x038), Some(0));
        assert_eq!(device.read_u32(0x044), Some(0));
        assert_eq!(device.read_u32(0x080), Some(0));
        assert_eq!(device.read_u32(0x090), Some(0));
        assert_eq!(device.read_u32(0x0a0), Some(0));
        assert_eq!(
            device.write_u32(0x038, 8),
            PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index")
        );
        assert_eq!(
            device.write_u32(0x044, 1),
            PaneVirtioMmioWriteResult::Rejected("unsupported virtqueue index")
        );

        assert_eq!(
            device.write_u32(0x030, 0),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.read_u32(0x034),
            Some(u32::from(PANE_VIRTIO_BLK_QUEUE_SIZE))
        );
        assert_eq!(device.read_u32(0x038), Some(8));
        assert_eq!(device.read_u32(0x044), Some(1));
        assert_eq!(device.read_u32(0x080), Some(0x1000));
        assert_eq!(device.read_u32(0x090), Some(0x2000));
        assert_eq!(device.read_u32(0x0a0), Some(0x3000));
    }

    #[test]
    fn virtio_mmio_block_device_resets_queue_runtime_when_queue_ready_is_cleared() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        configure_queue(&mut device);
        device.queue.last_notify = Some(0);
        device.queue.next_avail_index = 7;
        device.queue.used_index = 5;

        assert_eq!(
            device.write_u32(0x044, 0),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert!(!device.queue.ready);
        assert_eq!(device.queue.last_notify, None);
        assert_eq!(device.queue.next_avail_index, 0);
        assert_eq!(device.queue.used_index, 0);
        assert_eq!(device.read_u32(0x038), Some(8));
        assert_eq!(device.read_u32(0x080), Some(0x1000));
        assert_eq!(device.read_u32(0x090), Some(0x2000));
        assert_eq!(device.read_u32(0x0a0), Some(0x3000));

        assert_eq!(
            device.write_u32(0x044, 2),
            PaneVirtioMmioWriteResult::Rejected("invalid queue ready value")
        );
    }

    #[test]
    fn virtio_mmio_block_device_rejects_queue_reconfiguration_while_ready() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        configure_queue(&mut device);

        assert_eq!(
            device.write_u32(0x038, 16),
            PaneVirtioMmioWriteResult::Rejected("virtqueue is ready")
        );
        assert_eq!(
            device.write_u32(0x080, 0x5000),
            PaneVirtioMmioWriteResult::Rejected("virtqueue is ready")
        );
        assert_eq!(
            device.write_u32(0x090, 0x6000),
            PaneVirtioMmioWriteResult::Rejected("virtqueue is ready")
        );
        assert_eq!(
            device.write_u32(0x0a0, 0x7000),
            PaneVirtioMmioWriteResult::Rejected("virtqueue is ready")
        );
        assert_eq!(device.read_u32(0x038), Some(8));
        assert_eq!(device.read_u32(0x080), Some(0x1000));
        assert_eq!(device.read_u32(0x090), Some(0x2000));
        assert_eq!(device.read_u32(0x0a0), Some(0x3000));

        assert_eq!(
            device.write_u32(0x044, 0),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x038, 16),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x080, 0x5000),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x038), Some(16));
        assert_eq!(device.read_u32(0x080), Some(0x5000));
    }

    #[test]
    fn virtio_mmio_block_device_reads_back_feature_negotiation_registers() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);

        assert_eq!(
            device.write_u32(0x014, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x014), Some(1));
        assert_eq!(
            device.write_u32(0x024, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x020, (VIRTIO_F_VERSION_1 >> 32) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );

        assert_eq!(device.read_u32(0x024), Some(1));
        assert_eq!(
            device.read_u32(0x020),
            Some((VIRTIO_F_VERSION_1 >> 32) as u32)
        );
        assert_eq!(
            device.write_u32(0x070, 0),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x024), Some(0));
        assert_eq!(device.read_u32(0x020), Some(0));
    }

    #[test]
    fn virtio_mmio_block_device_masks_unsupported_features_until_negotiation_is_valid() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);

        assert_eq!(
            device.write_u32(0x020, (VIRTIO_BLK_F_RO | VIRTIO_BLK_F_BLK_SIZE) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.driver_features, VIRTIO_BLK_F_BLK_SIZE);
        assert_eq!(device.unsupported_driver_features, VIRTIO_BLK_F_RO);

        assert_eq!(
            device.write_u32(0x070, 1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x070), Some(1 | 2));

        assert_eq!(
            device.write_u32(0x020, VIRTIO_BLK_F_BLK_SIZE as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x024, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x020, (VIRTIO_F_VERSION_1 >> 32) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.unsupported_driver_features, 0);

        assert_eq!(
            device.write_u32(0x070, VIRTIO_CONFIG_S_FEATURES_OK),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.read_u32(0x070),
            Some(1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK)
        );
    }

    #[test]
    fn virtio_mmio_block_device_requires_features_ok_before_driver_ok() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);

        assert_eq!(
            device.write_u32(0x070, 1 | 2 | VIRTIO_CONFIG_S_DRIVER_OK),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(device.read_u32(0x070), Some(1 | 2));

        assert_eq!(
            device.write_u32(0x020, VIRTIO_BLK_F_BLK_SIZE as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x024, 1),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(0x020, (VIRTIO_F_VERSION_1 >> 32) as u32),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.write_u32(
                0x070,
                VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK
            ),
            PaneVirtioMmioWriteResult::Accepted
        );
        assert_eq!(
            device.read_u32(0x070),
            Some(1 | 2 | VIRTIO_CONFIG_S_FEATURES_OK | VIRTIO_CONFIG_S_DRIVER_OK)
        );
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
    fn virtio_mmio_access_service_reads_config_with_guest_widths() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);

        let capacity = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Read,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + VIRTIO_MMIO_CONFIG_OFFSET,
                data: vec![0; 8],
            },
            |_request, _payload| unreachable!("config reads must not execute queues"),
        );

        assert!(capacity.accepted);
        assert_eq!(capacity.status, "config-read");
        assert_eq!(capacity.offset, Some(VIRTIO_MMIO_CONFIG_OFFSET));
        assert_eq!(capacity.read_data, 2048_u64.to_le_bytes());

        let block_size_low = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Read,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + VIRTIO_MMIO_CONFIG_OFFSET + 0x14,
                data: vec![0; 2],
            },
            |_request, _payload| unreachable!("config reads must not execute queues"),
        );

        assert!(block_size_low.accepted);
        assert_eq!(block_size_low.status, "config-read");
        assert_eq!(block_size_low.read_data, 512_u16.to_le_bytes());

        let block_size_high_byte = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Read,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + VIRTIO_MMIO_CONFIG_OFFSET + 0x15,
                data: vec![0; 1],
            },
            |_request, _payload| unreachable!("config reads must not execute queues"),
        );

        assert!(block_size_high_byte.accepted);
        assert_eq!(block_size_high_byte.status, "config-read");
        assert_eq!(block_size_high_byte.read_data, vec![0x02]);
    }

    #[test]
    fn virtio_mmio_access_service_rejects_narrow_control_register_reads() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Read,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA,
                data: vec![0; 1],
            },
            |_request, _payload| unreachable!("invalid widths must not execute queues"),
        );

        assert!(!outcome.accepted);
        assert_eq!(outcome.status, "unsupported-width");
        assert_eq!(outcome.offset, Some(0));
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
    fn virtio_mmio_access_service_reports_interrupt_ack() {
        let mut device = PaneVirtioMmioBlockDevice::new(1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        device.interrupt_status = 1;

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + VIRTIO_MMIO_INTERRUPT_ACK_OFFSET,
                data: 1_u32.to_le_bytes().to_vec(),
            },
            |_request, _payload| unreachable!("interrupt ack must not execute queues"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "interrupt-ack");
        assert_eq!(outcome.offset, Some(VIRTIO_MMIO_INTERRUPT_ACK_OFFSET));
        assert_eq!(
            outcome.write_result,
            Some(PaneVirtioMmioWriteResult::Accepted)
        );
        assert_eq!(device.interrupt_status, 0);
    }

    #[test]
    fn virtio_mmio_access_service_executes_queue_notify() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, false);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);
        activate_driver(&mut device);

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
    fn virtio_mmio_access_service_drains_batched_queue_notify() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x7000);
        configure_queue(&mut device);
        activate_driver(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 8);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);

        memory.write_u32(0x5000, 0);
        memory.write_u32(0x5004, 0);
        memory.write_u64(0x5008, 9);
        write_descriptor(&mut memory, 3, 0x5000, 16, 1, 4);
        write_descriptor(&mut memory, 4, 0x5100, 512, 3, 5);
        write_descriptor(&mut memory, 5, 0x5300, 1, 2, 0);
        publish_heads(&mut memory, &[0, 3]);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x050,
                data: 0_u32.to_le_bytes().to_vec(),
            },
            |request, _payload| Ok(vec![request.sector as u8; 512]),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-executed");
        assert_eq!(outcome.queue_execution_count, 2);
        assert_eq!(
            outcome
                .queue_execution
                .as_ref()
                .map(|execution| execution.used_head_index),
            Some(3)
        );
        assert_eq!(memory.read_bytes(0x4100, 1), vec![8]);
        assert_eq!(memory.read_bytes(0x5100, 1), vec![9]);
        assert_eq!(memory.read_bytes(0x3002, 2), 2_u16.to_le_bytes());
        assert_eq!(device.queue.next_avail_index, 2);
        assert_eq!(device.queue.used_index, 2);
    }

    #[test]
    fn virtio_mmio_queue_notify_without_new_descriptors_is_accepted() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);
        activate_driver(&mut device);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x050,
                data: 0_u32.to_le_bytes().to_vec(),
            },
            |_request, _payload| unreachable!("empty notify must not invoke storage service"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-empty");
        assert_eq!(outcome.queue_execution_count, 0);
        assert_eq!(device.interrupt_status, 0);
    }

    #[test]
    fn virtio_mmio_queue_notify_before_driver_ok_does_not_execute_storage() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 11);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
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
            |_request, _payload| unreachable!("pre-DRIVER_OK notify must not invoke backend"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-not-ready");
        assert_eq!(outcome.queue_execution_count, 0);
        assert_eq!(device.queue.next_avail_index, 0);
        assert_eq!(device.queue.used_index, 0);
        assert_eq!(device.interrupt_status, 0);
    }

    #[test]
    fn virtio_mmio_queue_notify_ignores_unknown_queue_index() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 11);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x050,
                data: 1_u32.to_le_bytes().to_vec(),
            },
            |_request, _payload| unreachable!("unknown queue notify must not invoke backend"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-ignored");
        assert_eq!(outcome.queue_execution_count, 0);
        assert_eq!(device.queue.last_notify, Some(1));
        assert_eq!(device.queue.next_avail_index, 0);
        assert_eq!(device.queue.used_index, 0);
        assert_eq!(device.interrupt_status, 0);
    }

    #[test]
    fn virtio_mmio_queue_notify_with_guest_error_still_completes_mmio() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);
        activate_driver(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 11);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
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
            |_request, _payload| Err("backend read failed".to_string()),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-completed-with-guest-error");
        assert_eq!(outcome.queue_execution_count, 1);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_IOERR]);
        assert_eq!(memory.read_bytes(0x3002, 2), 1_u16.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
    }

    #[test]
    fn virtio_mmio_queue_notify_consumes_bad_descriptor_chain() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        let mut memory = TestGuestMemory::new(0x5000);
        configure_queue(&mut device);
        activate_driver(&mut device);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 11);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 0);
        publish_head(&mut memory, 0);

        let outcome = service_virtio_mmio_access(
            &mut device,
            &mut memory,
            PaneVirtioMmioAccess {
                kind: PaneVirtioMmioAccessKind::Write,
                gpa: PANE_VIRTIO_MMIO_BASE_GPA + 0x050,
                data: 0_u32.to_le_bytes().to_vec(),
            },
            |_request, _payload| unreachable!("bad descriptor chain must not reach backend"),
        );

        assert!(outcome.accepted);
        assert_eq!(outcome.status, "queue-notify-completed-with-guest-error");
        assert_eq!(outcome.queue_execution_count, 1);
        assert_eq!(device.queue.next_avail_index, 1);
        assert_eq!(device.queue.used_index, 1);
        assert_eq!(memory.read_bytes(0x3002, 2), 1_u16.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3004, 4), 0_u32.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3008, 4), 0_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
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
    fn virtio_blk_used_length_reports_guest_transferred_bytes() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 4);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 128, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let execution = device
            .execute_available_block_request(&mut memory, |_request, _payload| Ok(vec![0x7a; 512]));

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_OK);
        assert_eq!(execution.bytes_transferred, 128);
        assert_eq!(execution.used_len, 129);
        assert_eq!(memory.read_bytes(0x4100, 4), vec![0x7a; 4]);
        assert_eq!(memory.read_bytes(0x3008, 4), 129_u32.to_le_bytes());
    }

    #[test]
    fn virtio_blk_rejects_cyclic_descriptor_chain_without_stalling_queue() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 4);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 1, 0);
        publish_head(&mut memory, 0);

        let execution = device
            .execute_available_block_request(&mut memory, |_request, _payload| {
                unreachable!("cyclic descriptor chain must not reach backend")
            });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_IOERR);
        assert!(execution.detail.contains("cycle"));
        assert_eq!(execution.used_head_index, 0);
        assert_eq!(execution.used_len, 0);
        assert_eq!(device.queue.next_avail_index, 1);
        assert_eq!(device.queue.used_index, 1);
        assert_eq!(memory.read_bytes(0x3002, 2), 1_u16.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3004, 4), 0_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
    }

    #[test]
    fn virtio_blk_advances_available_and_used_rings_across_requests() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x7000);

        memory.write_u32(0x4000, 0);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 4);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0x4100, 512, 3, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);

        memory.write_u32(0x5000, 0);
        memory.write_u32(0x5004, 0);
        memory.write_u64(0x5008, 5);
        write_descriptor(&mut memory, 3, 0x5000, 16, 1, 4);
        write_descriptor(&mut memory, 4, 0x5100, 512, 3, 5);
        write_descriptor(&mut memory, 5, 0x5300, 1, 2, 0);
        publish_heads(&mut memory, &[0, 3]);

        let first = device.execute_available_block_request(&mut memory, |request, _payload| {
            assert_eq!(request.sector, 4);
            Ok(vec![0x44; 512])
        });
        let second = device.execute_available_block_request(&mut memory, |request, _payload| {
            assert_eq!(request.sector, 5);
            Ok(vec![0x55; 512])
        });
        let empty = device.execute_available_block_request(&mut memory, |_request, _payload| {
            unreachable!("no unpublished request should execute")
        });

        assert_eq!(first.status, VIRTIO_BLK_STATUS_OK);
        assert_eq!(second.status, VIRTIO_BLK_STATUS_OK);
        assert_eq!(first.used_head_index, 0);
        assert_eq!(second.used_head_index, 3);
        assert_eq!(device.queue.next_avail_index, 2);
        assert_eq!(device.queue.used_index, 2);
        assert_eq!(memory.read_bytes(0x4100, 1), vec![0x44]);
        assert_eq!(memory.read_bytes(0x5100, 1), vec![0x55]);
        assert_eq!(memory.read_bytes(0x3002, 2), 2_u16.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3004, 4), 0_u32.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3008, 4), 513_u32.to_le_bytes());
        assert_eq!(memory.read_bytes(0x300c, 4), 3_u32.to_le_bytes());
        assert_eq!(memory.read_bytes(0x3010, 4), 513_u32.to_le_bytes());
        assert_eq!(empty.status, VIRTIO_BLK_STATUS_IOERR);
        assert_eq!(
            empty.detail,
            "available ring does not contain a new request"
        );
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
    fn virtio_blk_denies_writes_on_readonly_device_without_backend_call() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
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

        let execution =
            device.execute_available_block_request(&mut memory, |_request, _payload| {
                unreachable!("read-only virtio-blk writes must not reach the backend")
            });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_IOERR);
        assert_eq!(execution.bytes_transferred, 0);
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_IOERR]);
        assert_eq!(memory.read_bytes(0x3008, 4), 1_u32.to_le_bytes());
        assert_eq!(device.interrupt_status, 1);
    }

    #[test]
    fn virtio_blk_readonly_write_does_not_read_guest_payload() {
        let mut device = PaneVirtioMmioBlockDevice::new(8 * 1024 * 1024, true);
        configure_queue(&mut device);
        let mut memory = TestGuestMemory::new(0x5000);

        memory.write_u32(0x4000, 1);
        memory.write_u32(0x4004, 0);
        memory.write_u64(0x4008, 7);
        write_descriptor(&mut memory, 0, 0x4000, 16, 1, 1);
        write_descriptor(&mut memory, 1, 0xffff_0000, 512, 1, 2);
        write_descriptor(&mut memory, 2, 0x4300, 1, 2, 0);
        publish_head(&mut memory, 0);

        let execution =
            device.execute_available_block_request(&mut memory, |_request, _payload| {
                unreachable!("read-only virtio-blk writes must not reach the backend")
            });

        assert_eq!(execution.status, VIRTIO_BLK_STATUS_IOERR);
        assert_eq!(
            execution.detail,
            "virtio-blk write denied on read-only device"
        );
        assert_eq!(memory.read_bytes(0x4300, 1), vec![VIRTIO_BLK_STATUS_IOERR]);
        assert_eq!(memory.read_bytes(0x3002, 2), 1_u16.to_le_bytes());
        assert_eq!(device.queue.next_avail_index, 1);
        assert_eq!(device.queue.used_index, 1);
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
