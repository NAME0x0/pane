//! Minimal Intel MultiProcessor (MP) table for the Pane-owned native runtime.
//!
//! With `acpi=off` on the guest kernel command line, Linux discovers the local
//! APIC and I/O APIC by scanning low memory for an MP floating-pointer
//! structure (`_MP_`) and parsing the configuration table it points at. Without
//! this table the guest never enables I/O APIC interrupt routing and falls back
//! to the legacy 8259 PIC, which is why a level-triggered device IRQ (virtio)
//! cannot be acknowledged through the local APIC.
//!
//! This builder emits just enough of the MP specification (one CPU, the ISA bus,
//! one I/O APIC, identity ISA->I/O-APIC interrupt routing with the virtio pin
//! marked level-triggered, and the two local-APIC LINT sources) for Linux to
//! route the virtio IRQ through the I/O APIC. The structures and checksum rules
//! follow the MP spec; the layout mirrors what crosvm's `mptable.rs` emits.

/// Guest-physical address of the MP floating pointer. Within Pane's mapped
/// `bios-rom` region (0xE0000..0x100000) and inside the 0xF0000..0xFFFFF window
/// Linux scans for the `_MP_` signature.
pub(crate) const MP_FLOATING_POINTER_GPA: u64 = 0x000f_0000;
/// Configuration table immediately follows the 16-byte floating pointer.
pub(crate) const MP_CONFIG_TABLE_GPA: u64 = MP_FLOATING_POINTER_GPA + 16;

const LOCAL_APIC_ADDRESS: u32 = 0xfee0_0000;
const IO_APIC_ADDRESS: u32 = 0xfec0_0000;
const APIC_VERSION: u8 = 0x14;
const IO_APIC_ID: u8 = 1;

// Configuration-table entry type tags.
const MP_PROCESSOR: u8 = 0;
const MP_BUS: u8 = 1;
const MP_IOAPIC: u8 = 2;
const MP_INTSRC: u8 = 3;
const MP_LINTSRC: u8 = 4;

// CPU flags.
const CPU_ENABLED: u8 = 1;
const CPU_BOOTPROCESSOR: u8 = 2;
const CPU_STEPPING: u32 = 0x0600;
const CPU_FEATURE_FPU: u32 = 0x0001;
const CPU_FEATURE_APIC: u32 = 0x0200;

// Interrupt source types.
const IRQ_TYPE_INT: u8 = 0;
const IRQ_TYPE_NMI: u8 = 1;
const IRQ_TYPE_EXTINT: u8 = 3;

// MP interrupt flag encodings: polarity in bits [1:0], trigger in bits [3:2].
// All ISA IRQs conform to the bus default, which is edge-triggered, active high.
// The virtio-MMIO line is routed edge to match crosvm's virtio-MMIO wiring and, more
// importantly, to keep the guest's I/O APIC redirection-entry trigger mode consistent
// with how Pane delivers the interrupt (WHvRequestInterrupt, edge): a guest RTE
// programmed level while Pane injects edge desynchronizes the WHP local-APIC EOI/ISR
// state, which stalled storage delivery after a few completions.
const IRQ_FLAG_DEFAULT: u16 = 0; // conforms to bus (ISA => edge, active high)

const NUM_ISA_IRQS: u8 = 16;

/// The virtio-MMIO device's ISA IRQ. Must match `crate::virtio::PANE_VIRTIO_MMIO_IRQ`.
const VIRTIO_IRQ: u8 = crate::virtio::PANE_VIRTIO_MMIO_IRQ as u8;

/// MP interrupt flag for an ISA IRQ pin: bus-default edge for every line (the timer on
/// pin 0 and the virtio-MMIO line alike), so the guest's redirection-entry trigger mode
/// matches Pane's edge `WHvRequestInterrupt` delivery.
fn isa_irq_flag(irq: u8) -> u16 {
    let _ = irq;
    IRQ_FLAG_DEFAULT
}

fn sum_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0_u8, |acc, &b| acc.wrapping_add(b))
}

/// One assembled MP table ready to be embedded into guest memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneMptable {
    pub(crate) floating_pointer_gpa: u64,
    pub(crate) floating_pointer: Vec<u8>,
    pub(crate) config_table_gpa: u64,
    pub(crate) config_table: Vec<u8>,
}

fn push_processor(out: &mut Vec<u8>, apic_id: u8, boot: bool) {
    let flags = CPU_ENABLED | if boot { CPU_BOOTPROCESSOR } else { 0 };
    out.push(MP_PROCESSOR);
    out.push(apic_id);
    out.push(APIC_VERSION);
    out.push(flags);
    out.extend_from_slice(&CPU_STEPPING.to_le_bytes());
    out.extend_from_slice(&(CPU_FEATURE_FPU | CPU_FEATURE_APIC).to_le_bytes());
    out.extend_from_slice(&[0_u8; 8]); // reserved
}

fn push_bus(out: &mut Vec<u8>, bus_id: u8, bus_type: &[u8; 6]) {
    out.push(MP_BUS);
    out.push(bus_id);
    out.extend_from_slice(bus_type);
}

fn push_ioapic(out: &mut Vec<u8>) {
    out.push(MP_IOAPIC);
    out.push(IO_APIC_ID);
    out.push(APIC_VERSION);
    out.push(1); // enabled
    out.extend_from_slice(&IO_APIC_ADDRESS.to_le_bytes());
}

fn push_intsrc(out: &mut Vec<u8>, irq_flag: u16, src_bus_irq: u8, dst_irq: u8) {
    out.push(MP_INTSRC);
    out.push(IRQ_TYPE_INT);
    out.extend_from_slice(&irq_flag.to_le_bytes());
    out.push(0); // source bus id (ISA bus 0)
    out.push(src_bus_irq);
    out.push(IO_APIC_ID); // destination I/O APIC
    out.push(dst_irq); // destination I/O APIC INTIN pin
}

fn push_lintsrc(out: &mut Vec<u8>, irq_type: u8, dest_lint: u8) {
    out.push(MP_LINTSRC);
    out.push(irq_type);
    out.extend_from_slice(&IRQ_FLAG_DEFAULT.to_le_bytes());
    out.push(0); // source bus id
    out.push(0); // source bus irq
    out.push(0); // destination local APIC (0 = all / BSP)
    out.push(dest_lint);
}

/// Build the MP floating pointer and configuration table.
pub(crate) fn build_pane_mptable() -> PaneMptable {
    // Entries first, so we can length-prefix and checksum the config table.
    let mut entries = Vec::new();
    push_processor(&mut entries, 0, true);
    push_bus(&mut entries, 0, b"ISA   ");
    push_ioapic(&mut entries);
    // Identity ISA IRQ -> I/O APIC pin routing. The timer (pin 0) and the other ISA
    // lines are edge-triggered (bus default); the virtio-MMIO IRQ is level-triggered,
    // active-high so its completion interrupts resample through the local-APIC EOI.
    for irq in 0..NUM_ISA_IRQS {
        push_intsrc(&mut entries, isa_irq_flag(irq), irq, irq);
    }
    push_lintsrc(&mut entries, IRQ_TYPE_EXTINT, 0); // LINT0 = ExtINT
    push_lintsrc(&mut entries, IRQ_TYPE_NMI, 1); // LINT1 = NMI

    // Count entries by tag (each entry begins with its type byte and has a fixed size).
    let entry_count = 1 + 1 + 1 + (NUM_ISA_IRQS as u16) + 2;

    // 44-byte configuration table header.
    let base_table_length = (44 + entries.len()) as u16;
    let mut config = Vec::with_capacity(base_table_length as usize);
    config.extend_from_slice(b"PCMP"); // signature
    config.extend_from_slice(&base_table_length.to_le_bytes());
    config.push(4); // spec revision 1.4
    config.push(0); // checksum placeholder
    config.extend_from_slice(b"PANE    "); // oem id (8)
    config.extend_from_slice(b"PANE000000  "); // product id (12)
    config.extend_from_slice(&0_u32.to_le_bytes()); // oem table pointer
    config.extend_from_slice(&0_u16.to_le_bytes()); // oem table size
    config.extend_from_slice(&entry_count.to_le_bytes()); // entry count
    config.extend_from_slice(&LOCAL_APIC_ADDRESS.to_le_bytes());
    config.extend_from_slice(&0_u32.to_le_bytes()); // reserved
    debug_assert_eq!(config.len(), 44);
    config.extend_from_slice(&entries);

    // Configuration-table checksum makes the whole base table sum to zero.
    let checksum = 0_u8.wrapping_sub(sum_checksum(&config));
    config[7] = checksum;

    // 16-byte floating pointer.
    let mut fp = Vec::with_capacity(16);
    fp.extend_from_slice(b"_MP_");
    fp.extend_from_slice(&(MP_CONFIG_TABLE_GPA as u32).to_le_bytes()); // physptr
    fp.push(1); // length in 16-byte paragraphs
    fp.push(4); // spec revision 1.4
    fp.push(0); // checksum placeholder
    fp.extend_from_slice(&[0_u8; 5]); // feature bytes (0 = configuration table present)
    debug_assert_eq!(fp.len(), 16);
    let fp_checksum = 0_u8.wrapping_sub(sum_checksum(&fp));
    fp[10] = fp_checksum;

    PaneMptable {
        floating_pointer_gpa: MP_FLOATING_POINTER_GPA,
        floating_pointer: fp,
        config_table_gpa: MP_CONFIG_TABLE_GPA,
        config_table: config,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_pointer_is_valid() {
        let table = build_pane_mptable();
        assert_eq!(table.floating_pointer.len(), 16);
        assert_eq!(&table.floating_pointer[0..4], b"_MP_");
        // physptr points at the config table.
        let physptr = u32::from_le_bytes(table.floating_pointer[4..8].try_into().unwrap());
        assert_eq!(u64::from(physptr), table.config_table_gpa);
        // Floating-pointer checksum sums to zero.
        assert_eq!(sum_checksum(&table.floating_pointer), 0);
    }

    #[test]
    fn config_table_header_and_checksum_are_valid() {
        let table = build_pane_mptable();
        assert_eq!(&table.config_table[0..4], b"PCMP");
        let length = u16::from_le_bytes(table.config_table[4..6].try_into().unwrap());
        assert_eq!(length as usize, table.config_table.len());
        // Local APIC address at offset 36.
        let lapic = u32::from_le_bytes(table.config_table[36..40].try_into().unwrap());
        assert_eq!(lapic, LOCAL_APIC_ADDRESS);
        // Base-table checksum sums to zero.
        assert_eq!(sum_checksum(&table.config_table), 0);
    }

    #[test]
    fn contains_ioapic_entry_at_expected_address() {
        let table = build_pane_mptable();
        // Walk entries after the 44-byte header.
        let mut offset = 44;
        let mut found_ioapic = false;
        while offset < table.config_table.len() {
            let entry_type = table.config_table[offset];
            let size = match entry_type {
                MP_PROCESSOR => 20,
                MP_BUS | MP_IOAPIC | MP_INTSRC | MP_LINTSRC => 8,
                other => panic!("unexpected MP entry type {other}"),
            };
            if entry_type == MP_IOAPIC {
                let addr = u32::from_le_bytes(
                    table.config_table[offset + 4..offset + 8]
                        .try_into()
                        .unwrap(),
                );
                assert_eq!(addr, IO_APIC_ADDRESS);
                assert_eq!(
                    table.config_table[offset + 3],
                    1,
                    "I/O APIC must be enabled"
                );
                found_ioapic = true;
            }
            offset += size;
        }
        assert!(found_ioapic, "MP table must describe the I/O APIC");
        assert_eq!(
            offset,
            table.config_table.len(),
            "entries must tile exactly"
        );
    }

    #[test]
    fn virtio_irq_is_routed_edge_triggered_to_its_own_pin() {
        let table = build_pane_mptable();
        let mut offset = 44;
        let mut virtio_flag = None;
        let mut timer_flag = None;
        while offset < table.config_table.len() {
            let entry_type = table.config_table[offset];
            let size = if entry_type == MP_PROCESSOR { 20 } else { 8 };
            if entry_type == MP_INTSRC {
                let flag = u16::from_le_bytes(
                    table.config_table[offset + 2..offset + 4]
                        .try_into()
                        .unwrap(),
                );
                let src_irq = table.config_table[offset + 5];
                let dst_irq = table.config_table[offset + 7];
                if src_irq == VIRTIO_IRQ {
                    assert_eq!(dst_irq, VIRTIO_IRQ, "virtio IRQ must route to its own pin");
                    virtio_flag = Some(flag);
                }
                if src_irq == 0 {
                    timer_flag = Some(flag);
                }
            }
            offset += size;
        }
        // virtio-MMIO and the pin-0 timer are both bus-default edge, so the guest's
        // redirection-entry trigger mode matches Pane's edge WHvRequestInterrupt delivery.
        assert_eq!(virtio_flag, Some(IRQ_FLAG_DEFAULT));
        assert_eq!(timer_flag, Some(IRQ_FLAG_DEFAULT));
    }
}
