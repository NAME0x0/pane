//! Minimal local APIC (xAPIC) emulation for the Pane-owned native runtime.
//!
//! Pane runs the WHP partition with `LocalApicEmulationMode=None`, so the guest's
//! local-APIC MMIO window (0xFEE00000) traps out to this software model instead of
//! WHP's in-platform APIC. Owning the LAPIC lets Pane control interrupt priority
//! (TPR/PPR/ISR), end-of-interrupt, and -- critically -- the LAPIC timer that drives
//! the guest scheduler tick / jiffies. With WHP's XApic LAPIC, injected timer and
//! virtio completion interrupts were not delivered to the guest during the blocking
//! ext4 mount (jiffies stalled, kernel threads starved); a Pane-owned LAPIC computes
//! the deliverable vector itself and injects it through the pending-interruption
//! register, the way QEMU's `kernel-irqchip=off` mode and crosvm's userspace irqchip
//! deliver interrupts on WHPX.
//!
//! The model is shaped by the semantics of crosvm's `devices/src/irqchip/apic.rs`
//! (BSD-3-Clause) and the Intel SDM Vol.3 local-APIC register map. It is pure and
//! unit-tested here; the WHP exit loop drives MMIO, timer ticks, delivery, and EOI.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use serde::Serialize;

/// Standard local-APIC MMIO window base and length. Left unmapped in the guest so
/// accesses trap to this model.
pub(crate) const LAPIC_BASE_GPA: u64 = 0xfee0_0000;
pub(crate) const LAPIC_MMIO_LENGTH_BYTES: u64 = 0x1000;

/// True if `gpa` falls inside the local-APIC MMIO window.
pub(crate) fn lapic_contains_gpa(gpa: u64) -> bool {
    (LAPIC_BASE_GPA..LAPIC_BASE_GPA + LAPIC_MMIO_LENGTH_BYTES).contains(&gpa)
}

// Register offsets within the MMIO window (Intel SDM Vol.3 Table "Local APIC
// Register Address Map"). Only the registers Linux actually touches are modeled.
const REG_ID: u64 = 0x020;
const REG_VERSION: u64 = 0x030;
const REG_TPR: u64 = 0x080;
const REG_PPR: u64 = 0x0a0;
const REG_EOI: u64 = 0x0b0;
const REG_LDR: u64 = 0x0d0;
const REG_DFR: u64 = 0x0e0;
const REG_SPURIOUS: u64 = 0x0f0;
const REG_ISR_BASE: u64 = 0x100;
const REG_TMR_BASE: u64 = 0x180;
const REG_IRR_BASE: u64 = 0x200;
const REG_ESR: u64 = 0x280;
const REG_ICR_LOW: u64 = 0x300;
const REG_ICR_HIGH: u64 = 0x310;
const REG_LVT_TIMER: u64 = 0x320;
const REG_LVT_THERMAL: u64 = 0x330;
const REG_LVT_PERF: u64 = 0x340;
const REG_LVT_LINT0: u64 = 0x350;
const REG_LVT_LINT1: u64 = 0x360;
const REG_LVT_ERROR: u64 = 0x370;
const REG_TIMER_INITIAL_COUNT: u64 = 0x380;
const REG_TIMER_CURRENT_COUNT: u64 = 0x390;
const REG_TIMER_DIVIDE: u64 = 0x3e0;

/// xAPIC version register value: version 0x14, max LVT entry 5 (6 LVTs, bits [23:16]).
const LAPIC_VERSION: u32 = 0x0005_0014;
/// Spurious-vector register: bit 8 is the APIC software-enable bit.
const SVR_ENABLE_BIT: u32 = 1 << 8;
/// LVT mask bit (bit 16) and timer-mode bit (bit 17 = periodic).
const LVT_MASK_BIT: u32 = 1 << 16;
const LVT_TIMER_PERIODIC_BIT: u32 = 1 << 17;
const LVT_VECTOR_MASK: u32 = 0xff;

/// A 256-vector bitmap stored as eight 32-bit words (xAPIC ISR/IRR/TMR layout: word
/// `i` covers vectors `32*i ..= 32*i+31`, accessed at MMIO `base + i*0x10`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct VectorBitmap {
    words: [u32; 8],
}

impl VectorBitmap {
    fn new() -> Self {
        Self { words: [0; 8] }
    }

    fn set(&mut self, vector: u8) {
        self.words[(vector >> 5) as usize] |= 1 << (vector & 0x1f);
    }

    fn clear(&mut self, vector: u8) {
        self.words[(vector >> 5) as usize] &= !(1 << (vector & 0x1f));
    }

    fn is_set(&self, vector: u8) -> bool {
        self.words[(vector >> 5) as usize] & (1 << (vector & 0x1f)) != 0
    }

    /// Highest set vector, or None if empty.
    fn highest(&self) -> Option<u8> {
        for word_index in (0..8).rev() {
            let word = self.words[word_index];
            if word != 0 {
                let bit = 31 - word.leading_zeros();
                return Some((word_index as u8) << 5 | bit as u8);
            }
        }
        None
    }

    fn word(&self, index: usize) -> u32 {
        self.words[index]
    }
}

/// What the exit loop should do after a guest LAPIC MMIO write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LapicWriteEffect {
    /// No side effect beyond updating LAPIC state.
    None,
    /// Guest wrote EOI; the exit loop should call `complete_eoi`.
    EndOfInterrupt,
    /// Guest wrote the ICR low dword; `vector` should be self-delivered (used for the
    /// single-vCPU self-IPI path; INIT/SIPI to absent APs are ignored).
    SelfIpi { vector: u8 },
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PaneLapic {
    id: u32,
    tpr: u32,
    ldr: u32,
    dfr: u32,
    spurious: u32,
    esr: u32,
    icr_high: u32,
    lvt_timer: u32,
    lvt_thermal: u32,
    lvt_perf: u32,
    lvt_lint0: u32,
    lvt_lint1: u32,
    lvt_error: u32,
    timer_initial_count: u32,
    timer_divide: u32,
    irr: VectorBitmap,
    isr: VectorBitmap,
    tmr: VectorBitmap,
    /// Wall-clock instant of the last timer accounting update.
    #[serde(skip)]
    timer_anchor: Option<Instant>,
}

impl Default for PaneLapic {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneLapic {
    pub(crate) fn new() -> Self {
        Self {
            id: 0,
            tpr: 0,
            ldr: 0,
            dfr: 0xffff_ffff,
            // Power-on: APIC software-disabled, spurious vector 0xff.
            spurious: 0xff,
            esr: 0,
            icr_high: 0,
            // LVTs reset masked.
            lvt_timer: LVT_MASK_BIT,
            lvt_thermal: LVT_MASK_BIT,
            lvt_perf: LVT_MASK_BIT,
            lvt_lint0: LVT_MASK_BIT,
            lvt_lint1: LVT_MASK_BIT,
            lvt_error: LVT_MASK_BIT,
            timer_initial_count: 0,
            timer_divide: 0,
            irr: VectorBitmap::new(),
            isr: VectorBitmap::new(),
            tmr: VectorBitmap::new(),
            timer_anchor: None,
        }
    }

    /// True once the guest has set the APIC software-enable bit in the spurious
    /// register. While disabled the LAPIC delivers nothing.
    fn enabled(&self) -> bool {
        self.spurious & SVR_ENABLE_BIT != 0
    }

    /// Processor priority class: max of the TPR class and the highest in-service
    /// vector's class. A pending interrupt is delivered only if its class exceeds this.
    fn processor_priority_class(&self) -> u32 {
        let tpr_class = (self.tpr >> 4) & 0xf;
        let isr_class = self.isr.highest().map(|v| u32::from(v) >> 4).unwrap_or(0);
        tpr_class.max(isr_class)
    }

    /// Raise an interrupt request for `vector` (called by the I/O APIC, virtio
    /// completion path, and the LAPIC timer). Edge semantics: just set the IRR bit.
    /// `level` records the trigger mode so EOI can notify the I/O APIC for level lines.
    pub(crate) fn raise_interrupt(&mut self, vector: u8, level: bool) {
        self.irr.set(vector);
        if level {
            self.tmr.set(vector);
        } else {
            self.tmr.clear(vector);
        }
    }

    /// If the highest pending IRR vector outranks the processor priority and the APIC
    /// is enabled, move it IRR -> ISR and return it for injection into the vCPU. The
    /// caller injects it through the pending-interruption register.
    pub(crate) fn take_deliverable_vector(&mut self) -> Option<u8> {
        if !self.enabled() {
            return None;
        }
        let vector = self.irr.highest()?;
        if (u32::from(vector) >> 4) <= self.processor_priority_class() {
            return None;
        }
        self.irr.clear(vector);
        self.isr.set(vector);
        Some(vector)
    }

    /// True if a vector is ready to deliver right now (does not mutate state). Lets the
    /// exit loop decide whether to bother checking guest readiness.
    pub(crate) fn has_deliverable_vector(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        match self.irr.highest() {
            Some(vector) => (u32::from(vector) >> 4) > self.processor_priority_class(),
            None => false,
        }
    }

    /// Handle a guest EOI: clear the highest in-service vector. Returns the vector if it
    /// was level-triggered (the caller must notify the I/O APIC to resample the line).
    pub(crate) fn complete_eoi(&mut self) -> Option<u8> {
        let vector = self.isr.highest()?;
        self.isr.clear(vector);
        if self.tmr.is_set(vector) {
            self.tmr.clear(vector);
            Some(vector)
        } else {
            None
        }
    }

    /// Divisor encoded in the timer divide-config register (bits 0,1,3).
    fn timer_divisor(&self) -> u32 {
        let encoded = ((self.timer_divide & 0b1000) >> 1) | (self.timer_divide & 0b11);
        match encoded {
            0b000 => 2,
            0b001 => 4,
            0b010 => 8,
            0b011 => 16,
            0b100 => 32,
            0b101 => 64,
            0b110 => 128,
            0b111 => 1,
            _ => 1,
        }
    }

    fn timer_periodic(&self) -> bool {
        self.lvt_timer & LVT_TIMER_PERIODIC_BIT != 0
    }

    fn timer_masked(&self) -> bool {
        self.lvt_timer & LVT_MASK_BIT != 0
    }

    fn timer_vector(&self) -> u8 {
        (self.lvt_timer & LVT_VECTOR_MASK) as u8
    }

    fn timer_armed(&self) -> bool {
        self.enabled() && !self.timer_masked() && self.timer_initial_count != 0
    }

    /// Pane drives the LAPIC timer at a fixed wall-clock cadence rather than emulating a
    /// precise APIC bus frequency: the guest only needs jiffies to advance at a steady
    /// rate for the scheduler and timeouts to make progress. `period` is the desired
    /// tick interval. Advances the timer and raises the timer vector when it expires;
    /// returns the vector raised (for logging). One-shot timers fire once then disarm.
    pub(crate) fn service_timer(&mut self, now: Instant, period: Duration) -> Option<u8> {
        if !self.timer_armed() {
            self.timer_anchor = None;
            return None;
        }
        let anchor = match self.timer_anchor {
            Some(anchor) => anchor,
            None => {
                self.timer_anchor = Some(now);
                return None;
            }
        };
        if now.duration_since(anchor) < period {
            return None;
        }
        self.timer_anchor = Some(now);
        let vector = self.timer_vector();
        self.raise_interrupt(vector, false);
        if !self.timer_periodic() {
            // One-shot: disarm until the guest reprograms the initial count.
            self.timer_initial_count = 0;
            self.timer_anchor = None;
        }
        Some(vector)
    }

    pub(crate) fn mmio_read(&self, offset: u64) -> u32 {
        match offset {
            REG_ID => self.id,
            REG_VERSION => LAPIC_VERSION,
            REG_TPR => self.tpr,
            REG_PPR => self.processor_priority_class() << 4,
            REG_LDR => self.ldr,
            REG_DFR => self.dfr,
            REG_SPURIOUS => self.spurious,
            REG_ESR => self.esr,
            REG_ICR_LOW => 0,
            REG_ICR_HIGH => self.icr_high,
            REG_LVT_TIMER => self.lvt_timer,
            REG_LVT_THERMAL => self.lvt_thermal,
            REG_LVT_PERF => self.lvt_perf,
            REG_LVT_LINT0 => self.lvt_lint0,
            REG_LVT_LINT1 => self.lvt_lint1,
            REG_LVT_ERROR => self.lvt_error,
            REG_TIMER_INITIAL_COUNT => self.timer_initial_count,
            // We do not model an exact count-down; report the initial count while armed.
            REG_TIMER_CURRENT_COUNT => {
                if self.timer_armed() {
                    self.timer_initial_count
                } else {
                    0
                }
            }
            REG_TIMER_DIVIDE => self.timer_divide,
            offset if (REG_ISR_BASE..REG_ISR_BASE + 0x80).contains(&offset) => {
                Self::bitmap_word(&self.isr, offset - REG_ISR_BASE)
            }
            offset if (REG_TMR_BASE..REG_TMR_BASE + 0x80).contains(&offset) => {
                Self::bitmap_word(&self.tmr, offset - REG_TMR_BASE)
            }
            offset if (REG_IRR_BASE..REG_IRR_BASE + 0x80).contains(&offset) => {
                Self::bitmap_word(&self.irr, offset - REG_IRR_BASE)
            }
            _ => 0,
        }
    }

    fn bitmap_word(bitmap: &VectorBitmap, relative: u64) -> u32 {
        // Each 32-bit word occupies a 16-byte stride.
        let index = (relative / 0x10) as usize;
        if index < 8 {
            bitmap.word(index)
        } else {
            0
        }
    }

    pub(crate) fn mmio_write(&mut self, offset: u64, value: u32) -> LapicWriteEffect {
        match offset {
            REG_ID => self.id = value & 0xff00_0000,
            REG_TPR => self.tpr = value & 0xff,
            REG_EOI => return LapicWriteEffect::EndOfInterrupt,
            REG_LDR => self.ldr = value & 0xff00_0000,
            REG_DFR => self.dfr = value | 0x0fff_ffff,
            REG_SPURIOUS => self.spurious = value,
            REG_ESR => self.esr = 0,
            REG_ICR_HIGH => self.icr_high = value & 0xff00_0000,
            REG_ICR_LOW => return self.write_icr_low(value),
            REG_LVT_TIMER => self.lvt_timer = value,
            REG_LVT_THERMAL => self.lvt_thermal = value,
            REG_LVT_PERF => self.lvt_perf = value,
            REG_LVT_LINT0 => self.lvt_lint0 = value,
            REG_LVT_LINT1 => self.lvt_lint1 = value,
            REG_LVT_ERROR => self.lvt_error = value,
            REG_TIMER_INITIAL_COUNT => {
                self.timer_initial_count = value;
                self.timer_anchor = None;
            }
            REG_TIMER_DIVIDE => self.timer_divide = value & 0b1011,
            _ => {}
        }
        LapicWriteEffect::None
    }

    fn write_icr_low(&mut self, value: u32) -> LapicWriteEffect {
        // ICR delivery mode in bits [10:8]: 0 = Fixed. Destination shorthand in [19:18]:
        // 0b01 = self. On a single-vCPU partition a fixed self-IPI is the only IPI that
        // must be delivered; INIT (0b101) / Startup (0b110) target absent APs and are
        // ignored.
        let delivery_mode = (value >> 8) & 0b111;
        let shorthand = (value >> 18) & 0b11;
        let vector = (value & 0xff) as u8;
        if delivery_mode == 0 && (shorthand == 0b01 || shorthand == 0b10 || shorthand == 0b11) {
            return LapicWriteEffect::SelfIpi { vector };
        }
        if delivery_mode == 0 && shorthand == 0b00 {
            // Physical/logical destination; single vCPU so it is us.
            return LapicWriteEffect::SelfIpi { vector };
        }
        LapicWriteEffect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_lapic() -> PaneLapic {
        let mut lapic = PaneLapic::new();
        lapic.mmio_write(REG_SPURIOUS, 0x1ff); // enable + spurious vector 0xff
        lapic
    }

    #[test]
    fn window_contains_base_and_registers() {
        assert!(lapic_contains_gpa(LAPIC_BASE_GPA));
        assert!(lapic_contains_gpa(LAPIC_BASE_GPA + REG_EOI));
        assert!(!lapic_contains_gpa(
            LAPIC_BASE_GPA + LAPIC_MMIO_LENGTH_BYTES
        ));
    }

    #[test]
    fn version_register_reports_six_lvts() {
        let lapic = PaneLapic::new();
        assert_eq!(lapic.mmio_read(REG_VERSION) & 0xff, 0x14);
        assert_eq!((lapic.mmio_read(REG_VERSION) >> 16) & 0xff, 5);
    }

    #[test]
    fn disabled_apic_delivers_nothing() {
        let mut lapic = PaneLapic::new(); // software-disabled at power-on
        lapic.raise_interrupt(0x40, false);
        assert!(!lapic.has_deliverable_vector());
        assert_eq!(lapic.take_deliverable_vector(), None);
    }

    #[test]
    fn raises_and_delivers_highest_vector() {
        let mut lapic = enabled_lapic();
        lapic.raise_interrupt(0x30, false);
        lapic.raise_interrupt(0x50, false);
        assert!(lapic.has_deliverable_vector());
        // Highest priority first.
        assert_eq!(lapic.take_deliverable_vector(), Some(0x50));
        // 0x50 now in service (class 5); 0x30 (class 3) is blocked by PPR until EOI.
        assert_eq!(lapic.take_deliverable_vector(), None);
        assert_eq!(lapic.complete_eoi(), None); // edge vector, no I/O APIC notify
                                                // After EOI, 0x30 can deliver.
        assert_eq!(lapic.take_deliverable_vector(), Some(0x30));
    }

    #[test]
    fn tpr_blocks_lower_priority_vectors() {
        let mut lapic = enabled_lapic();
        lapic.mmio_write(REG_TPR, 0x40); // class 4
        lapic.raise_interrupt(0x35, false); // class 3 -> blocked
        assert_eq!(lapic.take_deliverable_vector(), None);
        lapic.raise_interrupt(0x55, false); // class 5 -> deliverable
        assert_eq!(lapic.take_deliverable_vector(), Some(0x55));
    }

    #[test]
    fn level_eoi_reports_vector_for_ioapic() {
        let mut lapic = enabled_lapic();
        lapic.raise_interrupt(0x23, true); // level-triggered virtio line
        assert_eq!(lapic.take_deliverable_vector(), Some(0x23));
        assert_eq!(lapic.complete_eoi(), Some(0x23));
    }

    #[test]
    fn isr_and_irr_readback_through_mmio() {
        let mut lapic = enabled_lapic();
        lapic.raise_interrupt(0x42, false);
        // IRR word 2 covers vectors 0x40..0x5f; bit 2 = 0x42.
        assert_eq!(lapic.mmio_read(REG_IRR_BASE + 2 * 0x10) & (1 << 2), 1 << 2);
        lapic.take_deliverable_vector();
        assert_eq!(lapic.mmio_read(REG_ISR_BASE + 2 * 0x10) & (1 << 2), 1 << 2);
    }

    #[test]
    fn periodic_timer_fires_at_cadence() {
        let mut lapic = enabled_lapic();
        lapic.mmio_write(REG_TIMER_DIVIDE, 0b1011); // divide by 1
        lapic.mmio_write(REG_LVT_TIMER, 0x30 | LVT_TIMER_PERIODIC_BIT); // vector 0x30, periodic
        lapic.mmio_write(REG_TIMER_INITIAL_COUNT, 1_000_000);
        let start = Instant::now();
        // First service sets the anchor.
        assert_eq!(lapic.service_timer(start, Duration::from_millis(1)), None);
        // Before the period elapses, nothing fires.
        assert_eq!(
            lapic.service_timer(start + Duration::from_micros(500), Duration::from_millis(1)),
            None
        );
        // After the period, the timer vector is raised.
        assert_eq!(
            lapic.service_timer(start + Duration::from_millis(2), Duration::from_millis(1)),
            Some(0x30)
        );
        assert!(lapic.irr.is_set(0x30));
    }

    #[test]
    fn one_shot_timer_disarms_after_firing() {
        let mut lapic = enabled_lapic();
        lapic.mmio_write(REG_TIMER_DIVIDE, 0b1011);
        lapic.mmio_write(REG_LVT_TIMER, 0x31); // one-shot
        lapic.mmio_write(REG_TIMER_INITIAL_COUNT, 1);
        let start = Instant::now();
        lapic.service_timer(start, Duration::from_millis(1));
        assert_eq!(
            lapic.service_timer(start + Duration::from_millis(2), Duration::from_millis(1)),
            Some(0x31)
        );
        // Disarmed: initial count cleared, no further fires.
        assert_eq!(lapic.timer_initial_count, 0);
        assert_eq!(
            lapic.service_timer(start + Duration::from_millis(4), Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn masked_timer_does_not_fire() {
        let mut lapic = enabled_lapic();
        lapic.mmio_write(REG_LVT_TIMER, 0x30 | LVT_MASK_BIT | LVT_TIMER_PERIODIC_BIT);
        lapic.mmio_write(REG_TIMER_INITIAL_COUNT, 1);
        let start = Instant::now();
        lapic.service_timer(start, Duration::from_millis(1));
        assert_eq!(
            lapic.service_timer(start + Duration::from_secs(1), Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn eoi_write_signals_effect() {
        let mut lapic = enabled_lapic();
        assert_eq!(
            lapic.mmio_write(REG_EOI, 0),
            LapicWriteEffect::EndOfInterrupt
        );
    }

    #[test]
    fn self_ipi_fixed_returns_vector() {
        let mut lapic = enabled_lapic();
        // Fixed delivery (mode 0), self shorthand (0b01), vector 0xfd.
        let effect = lapic.mmio_write(REG_ICR_LOW, 0xfd | (0b01 << 18));
        assert_eq!(effect, LapicWriteEffect::SelfIpi { vector: 0xfd });
    }
}
