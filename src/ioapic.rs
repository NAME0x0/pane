//! Minimal I/O APIC emulation for the Pane-owned native runtime.
//!
//! Pane's WHP partition emulates the local APIC, but not the I/O APIC, so a
//! level-triggered device IRQ (such as the virtio-MMIO block device) has no way
//! to be delivered as a level interrupt the guest acknowledges through the local
//! APIC. Without that, virtio completion interrupts coalesce after the first
//! delivery and storage I/O stalls.
//!
//! This module is a narrow, Pane-owned reimplementation shaped by the semantics
//! of crosvm's `devices/src/irqchip/ioapic.rs` (BSD-3-Clause). It is not a
//! verbatim copy: instead of crosvm's eventfd `out_events`/`resample_events`, the
//! IOAPIC is pure and returns the vector that the WHP exit loop should inject
//! through `WHvRequestInterrupt`, and re-injects on local-APIC EOI while the line
//! remains asserted (the `remote_irr` resample mechanism).
//!
//! The device model is complete and unit-tested here; it is wired into the WHP
//! kernel-layout exit loop, the guest memory map, and the MP table in the
//! follow-on phases of the native interrupt-routing work.
#![allow(dead_code)]

use serde::Serialize;

/// Standard I/O APIC MMIO window base and length.
pub(crate) const IOAPIC_BASE_GPA: u64 = 0xfec0_0000;
pub(crate) const IOAPIC_MMIO_LENGTH_BYTES: u64 = 0x100;

/// True if `gpa` falls inside the I/O APIC MMIO window. Pane leaves this window
/// unmapped so guest accesses trap out to the device model below.
pub(crate) fn ioapic_contains_gpa(gpa: u64) -> bool {
    (IOAPIC_BASE_GPA..IOAPIC_BASE_GPA + IOAPIC_MMIO_LENGTH_BYTES).contains(&gpa)
}

/// Number of redirection-table pins. 24 is the conventional I/O APIC pin count.
pub(crate) const IOAPIC_NUM_PINS: usize = 24;

const IOAPIC_VERSION_ID: u32 = 0x0000_0020;

// MMIO register offsets within the window.
const IOREGSEL_OFFSET: u64 = 0x00;
const IOWIN_OFFSET: u64 = 0x10;

// Indirect register indexes selected through IOREGSEL.
const IOAPIC_REG_ID: u8 = 0x00;
const IOAPIC_REG_VERSION: u8 = 0x01;
const IOAPIC_REG_ARBITRATION_ID: u8 = 0x02;
/// First redirection-table register index. Each pin uses two 32-bit registers.
const IOWIN_REG_OFFSET: u8 = 0x10;

// Redirection-table entry bit positions (Intel I/O APIC datasheet).
const RTE_VECTOR_SHIFT: u64 = 0;
const RTE_VECTOR_MASK: u64 = 0xff;
const RTE_DELIVERY_STATUS_BIT: u64 = 1 << 12;
const RTE_REMOTE_IRR_BIT: u64 = 1 << 14;
const RTE_TRIGGER_MODE_LEVEL_BIT: u64 = 1 << 15;
const RTE_MASK_BIT: u64 = 1 << 16;
const RTE_DESTINATION_SHIFT: u64 = 56;
const RTE_DESTINATION_MASK: u64 = 0xff;
const RTE_DEST_MODE_LOGICAL_BIT: u64 = 1 << 11;

/// Software-writable bits of a redirection-table entry. Delivery status and
/// remote IRR are read-only to the guest; the device owns them.
const RTE_READ_ONLY_MASK: u64 = RTE_DELIVERY_STATUS_BIT | RTE_REMOTE_IRR_BIT;

/// What the WHP exit loop should inject when the IOAPIC delivers an interrupt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct IoapicDelivery {
    pub(crate) vector: u8,
    pub(crate) destination: u8,
    pub(crate) destination_logical: bool,
    pub(crate) level_triggered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PaneIoapic {
    ioregsel: u8,
    ioapicid: u32,
    redirect_table: Vec<u64>,
    /// Whether each pin's input line is currently asserted by its device.
    interrupt_level: Vec<bool>,
}

fn decode_redirection_register(reg: u8) -> Option<(usize, bool)> {
    if reg < IOWIN_REG_OFFSET {
        return None;
    }
    let relative = (reg - IOWIN_REG_OFFSET) as usize;
    let index = relative / 2;
    let is_high = relative % 2 == 1;
    if index >= IOAPIC_NUM_PINS {
        return None;
    }
    Some((index, is_high))
}

impl Default for PaneIoapic {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneIoapic {
    pub(crate) fn new() -> Self {
        // Power-on state: every pin masked, edge-triggered, vector 0.
        Self {
            ioregsel: 0,
            ioapicid: 0,
            redirect_table: vec![RTE_MASK_BIT; IOAPIC_NUM_PINS],
            interrupt_level: vec![false; IOAPIC_NUM_PINS],
        }
    }

    fn rte_vector(entry: u64) -> u8 {
        ((entry >> RTE_VECTOR_SHIFT) & RTE_VECTOR_MASK) as u8
    }

    fn rte_destination(entry: u64) -> u8 {
        ((entry >> RTE_DESTINATION_SHIFT) & RTE_DESTINATION_MASK) as u8
    }

    fn rte_is_level(entry: u64) -> bool {
        entry & RTE_TRIGGER_MODE_LEVEL_BIT != 0
    }

    fn rte_is_masked(entry: u64) -> bool {
        entry & RTE_MASK_BIT != 0
    }

    fn rte_remote_irr(entry: u64) -> bool {
        entry & RTE_REMOTE_IRR_BIT != 0
    }

    fn rte_dest_logical(entry: u64) -> bool {
        entry & RTE_DEST_MODE_LOGICAL_BIT != 0
    }

    fn delivery_for(entry: u64) -> IoapicDelivery {
        IoapicDelivery {
            vector: Self::rte_vector(entry),
            destination: Self::rte_destination(entry),
            destination_logical: Self::rte_dest_logical(entry),
            level_triggered: Self::rte_is_level(entry),
        }
    }

    /// Returns the redirection-table entry for a pin (diagnostics/tests).
    pub(crate) fn redirection_entry(&self, pin: usize) -> Option<u64> {
        self.redirect_table.get(pin).copied()
    }

    /// Decoded diagnostic view of a pin: (vector, masked, level, remote_irr,
    /// input_line_asserted). Used to inspect why a device line stopped delivering.
    pub(crate) fn pin_debug(&self, pin: usize) -> Option<(u8, bool, bool, bool, bool)> {
        let entry = *self.redirect_table.get(pin)?;
        Some((
            Self::rte_vector(entry),
            Self::rte_is_masked(entry),
            Self::rte_is_level(entry),
            Self::rte_remote_irr(entry),
            *self.interrupt_level.get(pin)?,
        ))
    }

    /// Delivery descriptor for a pin if it is programmed and unmasked, ignoring
    /// remote IRR. Used to re-deliver (resample) a level interrupt whose device
    /// still needs service, so a lost or coalesced delivery cannot stall I/O.
    pub(crate) fn pin_delivery_if_unmasked(&self, pin: usize) -> Option<IoapicDelivery> {
        let entry = *self.redirect_table.get(pin)?;
        if Self::rte_is_masked(entry) {
            return None;
        }
        Some(Self::delivery_for(entry))
    }

    /// True if any pin is programmed level-triggered and unmasked. Indicates the
    /// guest has taken ownership of IOAPIC routing for a level device.
    pub(crate) fn has_active_level_pin(&self) -> bool {
        self.redirect_table
            .iter()
            .any(|&entry| Self::rte_is_level(entry) && !Self::rte_is_masked(entry))
    }

    /// Handle a guest MMIO read of the IOAPIC window. Only 4-byte accesses to the
    /// IOREGSEL/IOWIN registers are modeled.
    pub(crate) fn mmio_read(&self, offset: u64) -> u32 {
        match offset {
            IOREGSEL_OFFSET => u32::from(self.ioregsel),
            IOWIN_OFFSET => self.read_indirect(),
            _ => 0,
        }
    }

    fn read_indirect(&self) -> u32 {
        match self.ioregsel {
            IOAPIC_REG_ID | IOAPIC_REG_ARBITRATION_ID => self.ioapicid,
            IOAPIC_REG_VERSION => ((IOAPIC_NUM_PINS as u32 - 1) << 16) | IOAPIC_VERSION_ID,
            reg => match decode_redirection_register(reg) {
                Some((index, is_high)) => {
                    let entry = self.redirect_table[index];
                    if is_high {
                        (entry >> 32) as u32
                    } else {
                        entry as u32
                    }
                }
                None => 0,
            },
        }
    }

    /// Handle a guest MMIO write of the IOAPIC window.
    pub(crate) fn mmio_write(&mut self, offset: u64, value: u32) {
        match offset {
            IOREGSEL_OFFSET => self.ioregsel = (value & 0xff) as u8,
            IOWIN_OFFSET => self.write_indirect(value),
            _ => {}
        }
    }

    fn write_indirect(&mut self, value: u32) {
        match self.ioregsel {
            IOAPIC_REG_VERSION | IOAPIC_REG_ARBITRATION_ID => { /* read-only */ }
            IOAPIC_REG_ID => self.ioapicid = value & 0x0f00_0000,
            reg => {
                let Some((index, is_high)) = decode_redirection_register(reg) else {
                    return;
                };
                let before = self.redirect_table[index];
                let entry = if is_high {
                    (before & 0x0000_0000_ffff_ffff) | (u64::from(value) << 32)
                } else {
                    let mut next = (before & 0xffff_ffff_0000_0000) | u64::from(value);
                    // Preserve read-only delivery-status and remote-IRR bits.
                    next = (next & !RTE_READ_ONLY_MASK) | (before & RTE_READ_ONLY_MASK);
                    // Switching to edge-triggered clears remote IRR.
                    if !Self::rte_is_level(next) {
                        next &= !RTE_REMOTE_IRR_BIT;
                    }
                    next
                };
                self.redirect_table[index] = entry;
            }
        }
    }

    /// Assert (`level = true`) or deassert (`level = false`) a device IRQ line.
    /// Returns the interrupt to inject, or `None` if it is masked, coalesced,
    /// or a deassertion.
    pub(crate) fn service_irq(&mut self, pin: usize, level: bool) -> Option<IoapicDelivery> {
        if pin >= IOAPIC_NUM_PINS {
            return None;
        }

        if !level {
            self.interrupt_level[pin] = false;
            return None;
        }

        let entry = self.redirect_table[pin];

        // Edge-triggered line already high: ignore the redundant assertion.
        if !Self::rte_is_level(entry) && self.interrupt_level[pin] {
            return None;
        }

        self.interrupt_level[pin] = true;

        if Self::rte_is_masked(entry) {
            return None;
        }

        // Level-triggered with remote IRR set: the previous interrupt has not been
        // acknowledged yet, so coalesce instead of injecting again.
        if Self::rte_is_level(entry) && Self::rte_remote_irr(entry) {
            return None;
        }

        if Self::rte_is_level(entry) {
            self.redirect_table[pin] |= RTE_REMOTE_IRR_BIT;
        }

        Some(Self::delivery_for(entry))
    }

    /// Called when the guest signals EOI for `vector` through the local APIC.
    /// Clears remote IRR for matching level pins and, if the device still holds
    /// the line asserted, returns the interrupt to re-inject (the resample that
    /// makes a level-triggered IRQ keep firing until the device is serviced).
    pub(crate) fn end_of_interrupt(&mut self, vector: u8) -> Option<IoapicDelivery> {
        let mut reinjection = None;
        for pin in 0..IOAPIC_NUM_PINS {
            let entry = self.redirect_table[pin];
            if Self::rte_vector(entry) == vector && Self::rte_is_level(entry) {
                self.redirect_table[pin] &= !RTE_REMOTE_IRR_BIT;
                if self.interrupt_level[pin] && !Self::rte_is_masked(entry) {
                    // Line still asserted: re-inject and re-arm remote IRR.
                    self.redirect_table[pin] |= RTE_REMOTE_IRR_BIT;
                    if reinjection.is_none() {
                        reinjection = Some(Self::delivery_for(self.redirect_table[pin]));
                    }
                }
            }
        }
        reinjection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Program pin `pin` with `vector`, level-triggered, unmasked, physical dest 0.
    fn program_level_pin(ioapic: &mut PaneIoapic, pin: usize, vector: u8) {
        let reg_low = IOWIN_REG_OFFSET + (pin as u8) * 2;
        // Low dword: vector + level trigger, mask clear.
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(reg_low));
        let low = u32::from(vector) | (RTE_TRIGGER_MODE_LEVEL_BIT as u32);
        ioapic.mmio_write(IOWIN_OFFSET, low);
    }

    #[test]
    fn version_register_reports_pin_count() {
        let ioapic = PaneIoapic::new();
        let mut probe = ioapic;
        probe.mmio_write(IOREGSEL_OFFSET, u32::from(IOAPIC_REG_VERSION));
        let version = probe.mmio_read(IOWIN_OFFSET);
        assert_eq!(version & 0xff, IOAPIC_VERSION_ID & 0xff);
        assert_eq!((version >> 16) & 0xff, (IOAPIC_NUM_PINS as u32) - 1);
    }

    #[test]
    fn power_on_pins_are_masked() {
        let ioapic = PaneIoapic::new();
        assert!(!ioapic.has_active_level_pin());
        for pin in 0..IOAPIC_NUM_PINS {
            assert!(PaneIoapic::rte_is_masked(
                ioapic.redirection_entry(pin).unwrap()
            ));
        }
    }

    #[test]
    fn programming_a_level_pin_round_trips_through_mmio() {
        let mut ioapic = PaneIoapic::new();
        program_level_pin(&mut ioapic, 5, 0x33);
        let entry = ioapic.redirection_entry(5).unwrap();
        assert_eq!(PaneIoapic::rte_vector(entry), 0x33);
        assert!(PaneIoapic::rte_is_level(entry));
        assert!(!PaneIoapic::rte_is_masked(entry));
        assert!(ioapic.has_active_level_pin());
        // Read back the low dword through MMIO.
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(IOWIN_REG_OFFSET + 10));
        assert_eq!(ioapic.mmio_read(IOWIN_OFFSET) & 0xff, 0x33);
    }

    #[test]
    fn masked_pin_does_not_inject() {
        let mut ioapic = PaneIoapic::new();
        // Pins are masked at power-on.
        assert_eq!(ioapic.service_irq(5, true), None);
    }

    #[test]
    fn level_irq_injects_once_then_coalesces_until_eoi() {
        let mut ioapic = PaneIoapic::new();
        program_level_pin(&mut ioapic, 5, 0x33);

        // First assertion injects vector 0x33 and arms remote IRR.
        let delivery = ioapic
            .service_irq(5, true)
            .expect("first assertion injects");
        assert_eq!(delivery.vector, 0x33);
        assert!(delivery.level_triggered);
        assert!(PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));

        // Further assertions while the line is held coalesce (no double inject).
        assert_eq!(ioapic.service_irq(5, true), None);

        // EOI with the line still asserted re-injects (level resample).
        let reinjected = ioapic.end_of_interrupt(0x33).expect("resample re-injects");
        assert_eq!(reinjected.vector, 0x33);
        assert!(PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));

        // Device deasserts, then EOI clears remote IRR with no re-injection.
        assert_eq!(ioapic.service_irq(5, false), None);
        assert_eq!(ioapic.end_of_interrupt(0x33), None);
        assert!(!PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));
    }

    #[test]
    fn eoi_for_unrelated_vector_does_not_touch_pin() {
        let mut ioapic = PaneIoapic::new();
        program_level_pin(&mut ioapic, 5, 0x33);
        ioapic.service_irq(5, true).unwrap();
        // EOI for a different vector leaves remote IRR set.
        assert_eq!(ioapic.end_of_interrupt(0x40), None);
        assert!(PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));
    }

    #[test]
    fn switching_to_edge_clears_remote_irr() {
        let mut ioapic = PaneIoapic::new();
        program_level_pin(&mut ioapic, 5, 0x33);
        ioapic.service_irq(5, true).unwrap();
        assert!(PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));
        // Rewrite the low dword as edge-triggered (clear the level bit).
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(IOWIN_REG_OFFSET + 10));
        ioapic.mmio_write(IOWIN_OFFSET, 0x33);
        assert!(!PaneIoapic::rte_remote_irr(
            ioapic.redirection_entry(5).unwrap()
        ));
    }

    #[test]
    fn destination_and_mode_decode_from_high_dword() {
        let mut ioapic = PaneIoapic::new();
        // Low dword: vector 0x41, level, logical destination mode.
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(IOWIN_REG_OFFSET + 10));
        ioapic.mmio_write(
            IOWIN_OFFSET,
            0x41 | (RTE_TRIGGER_MODE_LEVEL_BIT as u32) | (RTE_DEST_MODE_LOGICAL_BIT as u32),
        );
        // High dword: destination 0x02 in the top byte.
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(IOWIN_REG_OFFSET + 11));
        ioapic.mmio_write(IOWIN_OFFSET, 0x02 << 24);
        let delivery = ioapic.service_irq(5, true).unwrap();
        assert_eq!(delivery.vector, 0x41);
        assert_eq!(delivery.destination, 0x02);
        assert!(delivery.destination_logical);
    }

    #[test]
    fn edge_irq_reasserts_only_after_deassert() {
        let mut ioapic = PaneIoapic::new();
        // Program edge-triggered (no level bit), unmasked, vector 0x50.
        ioapic.mmio_write(IOREGSEL_OFFSET, u32::from(IOWIN_REG_OFFSET + 10));
        ioapic.mmio_write(IOWIN_OFFSET, 0x50);
        assert_eq!(ioapic.service_irq(5, true).map(|d| d.vector), Some(0x50));
        // Still high: no new injection for an edge line.
        assert_eq!(ioapic.service_irq(5, true), None);
        // Deassert then assert: injects again.
        assert_eq!(ioapic.service_irq(5, false), None);
        assert_eq!(ioapic.service_irq(5, true).map(|d| d.vector), Some(0x50));
    }
}
