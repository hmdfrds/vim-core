//! Packed u64 transition entries for the one-pass DFA.
//!
//! Each transition encodes the next state, a match-wins flag, and
//! a bitmask of capture slots to record on traversal.

/// A packed transition in the one-pass DFA transition table.
///
/// Layout (MSB to LSB):
/// - bits 63..43: next state ID (21 bits, max 2,097,152 states)
/// - bit 42: match_wins flag
/// - bits 41..10: slot bitmask (32 bits, max 16 capture groups)
/// - bits 9..0: reserved
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transition(u64);

impl Transition {
    const STATE_ID_BITS: u32 = 21;
    const STATE_ID_SHIFT: u32 = 64 - Self::STATE_ID_BITS; // 43
    const STATE_ID_MASK: u64 = ((1u64 << Self::STATE_ID_BITS) - 1) << Self::STATE_ID_SHIFT;
    pub(crate) const STATE_ID_LIMIT: u32 = 1 << Self::STATE_ID_BITS; // 2,097,152

    const MATCH_WINS_SHIFT: u32 = Self::STATE_ID_SHIFT - 1; // 42
    const MATCH_WINS_BIT: u64 = 1u64 << Self::MATCH_WINS_SHIFT;

    const SLOT_BITS: u32 = 32;
    const SLOT_SHIFT: u32 = 10;
    const SLOT_MASK: u64 = ((1u64 << Self::SLOT_BITS) - 1) << Self::SLOT_SHIFT;

    /// The dead/empty transition -- state ID 0, no flags, no slots.
    pub(crate) const DEAD: Self = Self(0);

    /// Create a new transition.
    pub(crate) const fn new(state_id: u32, match_wins: bool, slot_mask: u32) -> Self {
        let sid = (state_id as u64) << Self::STATE_ID_SHIFT;
        let mw = if match_wins { Self::MATCH_WINS_BIT } else { 0 };
        let slots = (slot_mask as u64) << Self::SLOT_SHIFT;
        Self(sid | mw | slots)
    }

    /// Extract the next state ID.
    #[inline]
    pub(crate) const fn state_id(self) -> u32 {
        (self.0 >> Self::STATE_ID_SHIFT) as u32
    }

    /// Whether this transition is to the dead state (state 0).
    #[inline]
    pub(crate) const fn is_dead(self) -> bool {
        self.state_id() == 0
    }

    /// The match-wins flag: if true, a previously-found match should be
    /// reported instead of continuing to extend.
    #[inline]
    pub(crate) const fn match_wins(self) -> bool {
        (self.0 & Self::MATCH_WINS_BIT) != 0
    }

    /// The slot bitmask. Bit `i` set means record current position into
    /// `slots[i]` when this transition is taken.
    #[inline]
    pub(crate) const fn slot_mask(self) -> u32 {
        ((self.0 & Self::SLOT_MASK) >> Self::SLOT_SHIFT) as u32
    }

    /// Update the state ID, preserving all other fields.
    pub(crate) const fn with_state_id(self, state_id: u32) -> Self {
        Self((self.0 & !Self::STATE_ID_MASK) | ((state_id as u64) << Self::STATE_ID_SHIFT))
    }

    /// Merge slot bits from another transition (OR the masks together).
    /// Used during construction when multiple epsilon paths contribute
    /// slots to the same consuming transition.
    pub(crate) const fn merge_slots(self, other_slots: u32) -> Self {
        Self(self.0 | ((other_slots as u64) << Self::SLOT_SHIFT))
    }
}

impl core::fmt::Debug for Transition {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_dead() {
            return write!(f, "DEAD");
        }
        write!(f, "s{}", self.state_id())?;
        if self.match_wins() {
            write!(f, " MW")?;
        }
        let slots = self.slot_mask();
        if slots != 0 {
            write!(f, " slots=0x{slots:08x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_transition_is_zero() {
        assert_eq!(Transition::DEAD.0, 0);
        assert!(Transition::DEAD.is_dead());
        assert_eq!(Transition::DEAD.state_id(), 0);
        assert!(!Transition::DEAD.match_wins());
        assert_eq!(Transition::DEAD.slot_mask(), 0);
    }

    #[test]
    fn round_trip_state_id() {
        for &sid in &[0, 1, 100, 1023, Transition::STATE_ID_LIMIT - 1] {
            let t = Transition::new(sid, false, 0);
            assert_eq!(t.state_id(), sid, "state_id mismatch for {sid}");
        }
    }

    #[test]
    fn round_trip_match_wins() {
        let t_yes = Transition::new(42, true, 0);
        let t_no = Transition::new(42, false, 0);
        assert!(t_yes.match_wins());
        assert!(!t_no.match_wins());
        assert_eq!(t_yes.state_id(), 42);
        assert_eq!(t_no.state_id(), 42);
    }

    #[test]
    fn round_trip_slot_mask() {
        let t = Transition::new(7, false, 0b1010_0101);
        assert_eq!(t.slot_mask(), 0b1010_0101);
        assert_eq!(t.state_id(), 7);
        assert!(!t.match_wins());
    }

    #[test]
    fn full_round_trip() {
        let t = Transition::new(1234, true, 0xDEAD_BEEF);
        assert_eq!(t.state_id(), 1234);
        assert!(t.match_wins());
        assert_eq!(t.slot_mask(), 0xDEAD_BEEF);
    }

    #[test]
    fn with_state_id_preserves_other_fields() {
        let t = Transition::new(1, true, 0xFF00);
        let t2 = t.with_state_id(999);
        assert_eq!(t2.state_id(), 999);
        assert!(t2.match_wins());
        assert_eq!(t2.slot_mask(), 0xFF00);
    }

    #[test]
    fn merge_slots_ors_bitmasks() {
        let t = Transition::new(5, false, 0b0011);
        let t2 = t.merge_slots(0b1100);
        assert_eq!(t2.slot_mask(), 0b1111);
        assert_eq!(t2.state_id(), 5);
    }

    #[test]
    fn debug_format_dead() {
        let s = format!("{:?}", Transition::DEAD);
        assert_eq!(s, "DEAD");
    }

    #[test]
    fn debug_format_normal() {
        let t = Transition::new(42, false, 0);
        let s = format!("{t:?}");
        assert_eq!(s, "s42");
    }

    #[test]
    fn debug_format_with_match_wins_and_slots() {
        let t = Transition::new(7, true, 0x0F);
        let s = format!("{t:?}");
        assert_eq!(s, "s7 MW slots=0x0000000f");
    }
}
