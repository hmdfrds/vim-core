//! Undo lifecycle intent for multi-cursor effect wrapping.
//!
//! `UndoIntent` captures whether a command's undo group should be closed
//! (delete, yank) or left open (change, insert entry). Derived once at
//! the executor boundary, carried as metadata through multi-cursor
//! dispatch, materialised into effects at a single wrapping point.

use crate::effects::Effect;
use crate::primitives::UndoCursorStrategy;

/// Declares how the executor wants undo markers wrapped.
///
/// Derived once at the executor boundary (from the command's effects),
/// carried as metadata, consumed by the single wrapping point in
/// multi-cursor dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoIntent {
    /// No undo group (pure motions, mode switches, scrolling).
    None,

    /// Complete atom: BEGIN + effects + END.
    /// Used by: delete, yank, paste, join, case change, etc.
    Closed {
        /// Cursor placement strategy after undoing this group.
        strategy: UndoCursorStrategy,
    },

    /// Group opens now, stays open until exit_finalize() on Esc.
    /// Used by: change (ciw/cc/C/s/S), insert entry (i/a/o/O/I/A).
    OpenEnded {
        /// Cursor placement strategy after undoing this group.
        strategy: UndoCursorStrategy,
    },
}

impl UndoIntent {
    /// Derive undo intent by scanning effects for `BeginUndoGroup`/`EndUndoGroup`.
    ///
    /// This replaces the ad-hoc scanning in `replicate_effects_precise`
    /// and the missing equivalent in `per_cursor.rs`.
    #[must_use]
    pub fn from_effects(effects: &[Effect]) -> Self {
        let mut strategy: Option<UndoCursorStrategy> = Option::None;
        let mut has_begin = false;
        let mut has_end = false;

        for effect in effects {
            match effect {
                Effect::BeginUndoGroup { cursor_strategy } => {
                    has_begin = true;
                    if strategy.is_none() {
                        strategy = Some(*cursor_strategy);
                    }
                }
                Effect::EndUndoGroup { .. } => {
                    has_end = true;
                }
                _ => {}
            }
        }

        if !has_begin {
            return Self::None;
        }

        let strategy = strategy.unwrap_or(UndoCursorStrategy::FirstEdit);

        if has_end {
            Self::Closed { strategy }
        } else {
            Self::OpenEnded { strategy }
        }
    }

    /// Materialise undo markers into the effects list based on intent.
    ///
    /// Called exactly once, after all multi-cursor manipulation is done.
    /// Both per-cursor and algebraic rebase paths converge here.
    pub fn materialize_undo_markers(effects: &mut Vec<Effect>, intent: &UndoIntent) {
        match intent {
            UndoIntent::None => {}
            UndoIntent::Closed { strategy } => {
                effects.insert(
                    0,
                    Effect::BeginUndoGroup {
                        cursor_strategy: *strategy,
                    },
                );
                effects.push(Effect::EndUndoGroup { node_id: None });
            }
            UndoIntent::OpenEnded { strategy } => {
                effects.insert(
                    0,
                    Effect::BeginUndoGroup {
                        cursor_strategy: *strategy,
                    },
                );
            }
        }
    }

    /// Whether this intent has any undo group at all.
    #[must_use]
    pub const fn has_undo_group(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Whether the undo group is intentionally left open.
    #[must_use]
    pub const fn is_open_ended(&self) -> bool {
        matches!(self, Self::OpenEnded { .. })
    }

    /// The undo cursor strategy, if any.
    #[must_use]
    pub const fn strategy(&self) -> Option<UndoCursorStrategy> {
        match self {
            Self::None => None,
            Self::Closed { strategy } | Self::OpenEnded { strategy } => Some(*strategy),
        }
    }
}

/// Strip `BeginUndoGroup` and `EndUndoGroup` effects from a list in place.
///
/// Used by multi-cursor paths before replication, since undo markers are
/// materialised separately via `materialize_undo_markers`.
pub fn strip_undo_markers(effects: &mut Vec<Effect>) {
    effects.retain(|e| {
        !matches!(
            e,
            Effect::BeginUndoGroup { .. } | Effect::EndUndoGroup { .. }
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{Offset, UndoCursorStrategy};

    #[test]
    fn from_effects_none() {
        let effects = vec![Effect::SetCursor {
            offset: Offset::new(0),
        }];
        assert_eq!(UndoIntent::from_effects(&effects), UndoIntent::None);
    }

    #[test]
    fn from_effects_closed() {
        let effects = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::SetCursor {
                offset: Offset::new(0),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        assert_eq!(
            UndoIntent::from_effects(&effects),
            UndoIntent::Closed {
                strategy: UndoCursorStrategy::FirstEdit
            }
        );
    }

    #[test]
    fn from_effects_open_ended() {
        let effects = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::EntryPosition,
            },
            Effect::SetCursor {
                offset: Offset::new(0),
            },
        ];
        assert_eq!(
            UndoIntent::from_effects(&effects),
            UndoIntent::OpenEnded {
                strategy: UndoCursorStrategy::EntryPosition
            }
        );
    }

    #[test]
    fn materialize_closed() {
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(5),
        }];
        UndoIntent::materialize_undo_markers(
            &mut effects,
            &UndoIntent::Closed {
                strategy: UndoCursorStrategy::FirstEdit,
            },
        );
        assert_eq!(effects.len(), 3);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[2], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn materialize_open_ended_no_end() {
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(5),
        }];
        UndoIntent::materialize_undo_markers(
            &mut effects,
            &UndoIntent::OpenEnded {
                strategy: UndoCursorStrategy::FirstEdit,
            },
        );
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(!matches!(effects[1], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn materialize_none_is_noop() {
        let mut effects = vec![Effect::SetCursor {
            offset: Offset::new(5),
        }];
        UndoIntent::materialize_undo_markers(&mut effects, &UndoIntent::None);
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn strip_undo_markers_removes_both() {
        let mut effects = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::SetCursor {
                offset: Offset::new(5),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        strip_undo_markers(&mut effects);
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], Effect::SetCursor { .. }));
    }

    #[test]
    fn strip_preserves_non_undo_effects() {
        let mut effects = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: "x".into(),
            },
            Effect::SetCursor {
                offset: Offset::new(1),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        strip_undo_markers(&mut effects);
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::Insert { .. }));
        assert!(matches!(effects[1], Effect::SetCursor { .. }));
    }

    #[test]
    fn from_effects_empty_slice() {
        assert_eq!(UndoIntent::from_effects(&[]), UndoIntent::None);
    }

    #[test]
    fn materialize_closed_on_empty_vec() {
        let mut effects: Vec<Effect> = vec![];
        UndoIntent::materialize_undo_markers(
            &mut effects,
            &UndoIntent::Closed {
                strategy: UndoCursorStrategy::FirstEdit,
            },
        );
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(effects[1], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn strip_empty_is_noop() {
        let mut effects: Vec<Effect> = vec![];
        strip_undo_markers(&mut effects);
        assert!(effects.is_empty());
    }

    #[test]
    fn roundtrip_closed() {
        let original = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::FirstEdit,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: "x".into(),
            },
            Effect::EndUndoGroup { node_id: None },
        ];
        let intent = UndoIntent::from_effects(&original);
        assert_eq!(
            intent,
            UndoIntent::Closed {
                strategy: UndoCursorStrategy::FirstEdit
            }
        );

        let mut stripped = original.clone();
        strip_undo_markers(&mut stripped);
        assert_eq!(stripped.len(), 1);

        UndoIntent::materialize_undo_markers(&mut stripped, &intent);
        assert_eq!(stripped.len(), 3);
        assert!(matches!(stripped[0], Effect::BeginUndoGroup { .. }));
        assert!(matches!(stripped[2], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn roundtrip_open_ended() {
        let original = vec![
            Effect::BeginUndoGroup {
                cursor_strategy: UndoCursorStrategy::EntryPosition,
            },
            Effect::Insert {
                offset: Offset::new(0),
                text: "x".into(),
            },
        ];
        let intent = UndoIntent::from_effects(&original);
        assert_eq!(
            intent,
            UndoIntent::OpenEnded {
                strategy: UndoCursorStrategy::EntryPosition
            }
        );

        let mut stripped = original.clone();
        strip_undo_markers(&mut stripped);
        assert_eq!(stripped.len(), 1);

        UndoIntent::materialize_undo_markers(&mut stripped, &intent);
        assert_eq!(stripped.len(), 2);
        assert!(matches!(stripped[0], Effect::BeginUndoGroup { .. }));
        assert!(!matches!(stripped[1], Effect::EndUndoGroup { .. }));
    }

    #[test]
    fn accessors() {
        let none = UndoIntent::None;
        assert!(!none.has_undo_group());
        assert!(!none.is_open_ended());
        assert!(none.strategy().is_none());

        let closed = UndoIntent::Closed {
            strategy: UndoCursorStrategy::FirstEdit,
        };
        assert!(closed.has_undo_group());
        assert!(!closed.is_open_ended());
        assert_eq!(closed.strategy(), Some(UndoCursorStrategy::FirstEdit));

        let open = UndoIntent::OpenEnded {
            strategy: UndoCursorStrategy::EntryPosition,
        };
        assert!(open.has_undo_group());
        assert!(open.is_open_ended());
        assert_eq!(open.strategy(), Some(UndoCursorStrategy::EntryPosition));
    }
}
