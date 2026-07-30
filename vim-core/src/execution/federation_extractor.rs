//! Event extraction from engine effects.
//!
//! Scans an [`Effects`](crate::effects::Effects) collection produced by the engine
//! and emits [`StateEvent`]s for each effect that represents a federable state change.
//!
//! This is intentionally a pure function — it does not mutate state or effects.
//!
//! Lives in `execution/` rather than `state/federation/` because it reads from
//! `effects::Effect` (which state may not depend on).

use crate::effects::Effect;
use crate::primitives::{MotionType, RegisterContent, SearchDirection};
use crate::state::federation::backend::GlobalMark;
use crate::state::federation::events::{EventSource, StateEvent};
use crate::state::VimState;
use compact_str::CompactString;
use smallvec::SmallVec;

/// Extract federation-relevant [`StateEvent`]s from a completed effect list.
///
/// Called after the engine processes a keystroke, before the response is
/// returned to the caller. The returned events should be forwarded to any
/// connected federation backend or peer instances.
///
/// # Covered effect types
///
/// | Effect                | StateEvent produced             |
/// |-----------------------|---------------------------------|
/// | `SetRegister`         | `RegisterChanged`               |
/// | `ClearNamedRegister`  | `RegisterChanged` (empty)       |
/// | `SetMark` (A-Z)       | `GlobalMarkChanged`             |
/// | `SetMark` (a-z)       | `LocalMarkChanged`              |
/// | `SetSearchPattern`    | `SearchPatternChanged`          |
/// | `SetMode`             | `ModeChanged`                   |
/// | `StartRecording`      | `MacroRecordingChanged(Some(r))`|
/// | `StopRecording`       | `MacroRecordingChanged(None)`   |
/// | `PushJumpList`        | `JumpAdded`                     |
/// | `EndUndoGroup`        | `UndoBranched` (when node created)|
///
/// # Notes
///
/// - All extracted events have [`EventSource::Local`] — they represent
///   changes produced by this engine instance.
/// - Effects that do not correspond to federable state (text edits, cursor
///   moves, UI hints, etc.) are ignored.
pub fn extract_events(effects: &[Effect], state: &VimState) -> SmallVec<[StateEvent; 2]> {
    let mut events: SmallVec<[StateEvent; 2]> = SmallVec::new();

    for effect in effects {
        match effect {
            // ── Register changes ─────────────────────────────────────────
            Effect::SetRegister {
                name,
                text,
                motion_type,
            } => {
                let content = match motion_type {
                    MotionType::LineWise => RegisterContent::line_wise(text.clone()),
                    // drift: CharWise and BlockWise both map to char_wise register content in the federation event model
                    _ => RegisterContent::char_wise(text.clone()),
                };
                events.push(StateEvent::RegisterChanged {
                    name: *name,
                    content,
                    source: EventSource::Local,
                });
            }

            // ── Register clearing ─────────────────────────────────────────
            // Clearing a named register is federated as a RegisterChanged with
            // empty charwise content so that peer instances can also clear it.
            Effect::ClearNamedRegister { register } => {
                events.push(StateEvent::RegisterChanged {
                    name: *register,
                    content: RegisterContent::char_wise(CompactString::new("")),
                    source: EventSource::Local,
                });
            }

            // ── Mark changes ─────────────────────────────────────────────
            Effect::SetMark { name, offset, .. } => {
                if name.is_global() {
                    // Global marks (A-Z) — emit GlobalMarkChanged.
                    // We don't have file-path context in the extractor, so
                    // file_path is left empty. Callers that have file context
                    // should post-process or enrich the event.
                    let mark = GlobalMark {
                        file_path: CompactString::new(""),
                        offset: offset.get(),
                        line: 0,
                        column: 0,
                        timestamp: 0,
                    };
                    events.push(StateEvent::GlobalMarkChanged {
                        name: *name,
                        mark,
                        source: EventSource::Local,
                    });
                } else if name.is_local() {
                    events.push(StateEvent::LocalMarkChanged {
                        name: *name,
                        offset: *offset,
                        source: EventSource::Local,
                    });
                }
                // Special/numbered marks are not federated.
            }

            // ── Search pattern changes ────────────────────────────────────
            Effect::SetSearchPattern { pattern, direction } => {
                let search_direction: SearchDirection = (*direction).into();
                events.push(StateEvent::SearchPatternChanged {
                    pattern: pattern.clone(),
                    direction: search_direction,
                    source: EventSource::Local,
                });
            }

            // ── Mode changes ──────────────────────────────────────────────
            Effect::SetMode { mode, .. } => {
                let from = state.mode();
                events.push(StateEvent::ModeChanged { from, to: *mode });
            }

            // ── Macro recording state changes ─────────────────────────────
            // StartRecording carries the target register; StopRecording carries
            // no data. Both are emitted unconditionally — the federation layer
            // can decide whether to propagate them based on its policy.
            Effect::StartRecording { register } => {
                events.push(StateEvent::MacroRecordingChanged {
                    register: Some(*register),
                });
            }
            Effect::StopRecording => {
                events.push(StateEvent::MacroRecordingChanged { register: None });
            }

            // ── Jump list additions ───────────────────────────────────────
            // PushJumpList carries the byte offset being pushed. No file
            // context is available at this layer; consumers that need it
            // should enrich the event with the current buffer path.
            Effect::PushJumpList { offset } => {
                events.push(StateEvent::JumpAdded {
                    offset: *offset,
                    file_hint: None,
                });
            }

            // ── Undo tree branch points ───────────────────────────────────
            // EndUndoGroup carries the engine-assigned NodeId for the group
            // that was just committed. A `Some(id)` means a new undo node was
            // created (a branch point in the tree); `None` means the group
            // was empty and no node was committed. Only emit UndoBranched
            // when a real node was created.
            Effect::EndUndoGroup { node_id: Some(id) } => {
                events.push(StateEvent::UndoBranched {
                    branch_point: id.index() as u64,
                });
            }

            // All other effects are not federable.
            _ => {}
        }
    }

    events
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{Direction, MarkName, Mode, NodeId, Offset, RegisterName};
    use crate::state::VimState;

    fn empty_state() -> VimState {
        VimState::new()
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn single_event(effects: &[Effect]) -> StateEvent {
        let state = empty_state();
        let events = extract_events(effects, &state);
        assert_eq!(
            events.len(),
            1,
            "expected exactly one event, got {}",
            events.len()
        );
        events.into_iter().next().unwrap()
    }

    // ── ClearNamedRegister ────────────────────────────────────────────────────

    #[test]
    fn clear_named_register_emits_register_changed_with_empty_content() {
        let reg = RegisterName::new_unchecked('a');
        let effects = vec![Effect::ClearNamedRegister { register: reg }];

        let event = single_event(&effects);

        match event {
            StateEvent::RegisterChanged {
                name,
                content,
                source,
            } => {
                assert_eq!(name, reg, "register name must match the cleared register");
                assert!(
                    content.is_empty(),
                    "content must be empty for a cleared register"
                );
                assert_eq!(source, EventSource::Local);
            }
            other => panic!("expected RegisterChanged, got {other:?}"),
        }
    }

    #[test]
    fn clear_named_register_content_is_charwise() {
        let reg = RegisterName::new_unchecked('b');
        let effects = vec![Effect::ClearNamedRegister { register: reg }];

        let state = empty_state();
        let events = extract_events(&effects, &state);
        assert_eq!(events.len(), 1);

        match &events[0] {
            StateEvent::RegisterChanged { content, .. } => {
                assert_eq!(
                    content.motion_type(),
                    MotionType::CharWise,
                    "cleared register content must be charwise"
                );
            }
            other => panic!("expected RegisterChanged, got {other:?}"),
        }
    }

    // ── PushJumpList / JumpAdded ──────────────────────────────────────────────

    #[test]
    fn push_jump_list_emits_jump_added() {
        let offset = Offset::new(42);
        let effects = vec![Effect::PushJumpList { offset }];

        let event = single_event(&effects);

        match event {
            StateEvent::JumpAdded {
                offset: evt_offset,
                file_hint,
            } => {
                assert_eq!(
                    evt_offset, offset,
                    "JumpAdded offset must match PushJumpList offset"
                );
                assert!(
                    file_hint.is_none(),
                    "file_hint must be None — no file context available in the extractor"
                );
            }
            other => panic!("expected JumpAdded, got {other:?}"),
        }
    }

    #[test]
    fn push_jump_list_zero_offset() {
        let effects = vec![Effect::PushJumpList {
            offset: Offset::new(0),
        }];
        let event = single_event(&effects);
        match event {
            StateEvent::JumpAdded { offset, .. } => assert_eq!(offset, Offset::new(0)),
            other => panic!("expected JumpAdded, got {other:?}"),
        }
    }

    // ── StartRecording / MacroRecordingChanged ────────────────────────────────

    #[test]
    fn start_recording_emits_macro_recording_changed_some() {
        let reg = RegisterName::new_unchecked('q');
        let effects = vec![Effect::StartRecording { register: reg }];

        let event = single_event(&effects);

        match event {
            StateEvent::MacroRecordingChanged { register: Some(r) } => {
                assert_eq!(
                    r, reg,
                    "MacroRecordingChanged register must match StartRecording register"
                );
            }
            other => panic!("expected MacroRecordingChanged(Some(_)), got {other:?}"),
        }
    }

    #[test]
    fn stop_recording_emits_macro_recording_changed_none() {
        let effects = vec![Effect::StopRecording];

        let event = single_event(&effects);

        match event {
            StateEvent::MacroRecordingChanged { register: None } => {}
            other => panic!("expected MacroRecordingChanged(None), got {other:?}"),
        }
    }

    // ── EndUndoGroup / UndoBranched ───────────────────────────────────────────

    #[test]
    fn end_undo_group_with_node_id_emits_undo_branched() {
        let node = NodeId::new(7);
        let effects = vec![Effect::EndUndoGroup {
            node_id: Some(node),
        }];

        let event = single_event(&effects);

        match event {
            StateEvent::UndoBranched { branch_point } => {
                assert_eq!(
                    branch_point,
                    node.index() as u64,
                    "UndoBranched branch_point must equal the NodeId index"
                );
            }
            other => panic!("expected UndoBranched, got {other:?}"),
        }
    }

    #[test]
    fn end_undo_group_with_no_node_id_emits_nothing() {
        let effects = vec![Effect::EndUndoGroup { node_id: None }];
        let state = empty_state();
        let events = extract_events(&effects, &state);
        assert!(
            events.is_empty(),
            "empty undo group (node_id = None) must not emit any event; got: {events:?}"
        );
    }

    // ── Regression: existing wiring is unaffected ──────────────────────────────

    #[test]
    fn set_register_still_emits_register_changed() {
        let reg = RegisterName::UNNAMED;
        let effects = vec![Effect::SetRegister {
            name: reg,
            text: CompactString::new("hello"),
            motion_type: MotionType::CharWise,
        }];
        let event = single_event(&effects);
        match event {
            StateEvent::RegisterChanged { name, .. } => assert_eq!(name, reg),
            other => panic!("expected RegisterChanged, got {other:?}"),
        }
    }

    #[test]
    fn set_mode_still_emits_mode_changed() {
        let effects = vec![Effect::set_mode(Mode::Insert)];
        let state = empty_state();
        let events = extract_events(&effects, &state);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::ModeChanged { to, .. } => assert_eq!(*to, Mode::Insert),
            other => panic!("expected ModeChanged, got {other:?}"),
        }
    }

    #[test]
    fn set_search_pattern_still_emits_search_pattern_changed() {
        let effects = vec![Effect::SetSearchPattern {
            pattern: CompactString::new("foo"),
            direction: Direction::Forward,
        }];
        let state = empty_state();
        let events = extract_events(&effects, &state);
        assert_eq!(events.len(), 1);
        match &events[0] {
            StateEvent::SearchPatternChanged { pattern, .. } => {
                assert_eq!(pattern.as_str(), "foo")
            }
            other => panic!("expected SearchPatternChanged, got {other:?}"),
        }
    }

    #[test]
    fn global_mark_still_emits_global_mark_changed() {
        let name = MarkName::new_unchecked('A');
        let effects = vec![Effect::SetMark {
            name,
            offset: Offset::new(10),
            topline_offset: None,
        }];
        let event = single_event(&effects);
        match event {
            StateEvent::GlobalMarkChanged { name: n, .. } => assert_eq!(n, name),
            other => panic!("expected GlobalMarkChanged, got {other:?}"),
        }
    }

    #[test]
    fn local_mark_still_emits_local_mark_changed() {
        let name = MarkName::new_unchecked('a');
        let effects = vec![Effect::SetMark {
            name,
            offset: Offset::new(5),
            topline_offset: None,
        }];
        let event = single_event(&effects);
        match event {
            StateEvent::LocalMarkChanged {
                name: n, offset, ..
            } => {
                assert_eq!(n, name);
                assert_eq!(offset, Offset::new(5));
            }
            other => panic!("expected LocalMarkChanged, got {other:?}"),
        }
    }

    // ── Multiple effects in one batch ─────────────────────────────────────────

    #[test]
    fn multiple_events_extracted_in_order() {
        let reg_q = RegisterName::new_unchecked('q');
        let effects = vec![
            Effect::StartRecording { register: reg_q },
            Effect::PushJumpList {
                offset: Offset::new(100),
            },
            Effect::StopRecording,
        ];
        let state = empty_state();
        let events = extract_events(&effects, &state);
        assert_eq!(
            events.len(),
            3,
            "all three federable effects must produce events"
        );

        assert!(
            matches!(
                events[0],
                StateEvent::MacroRecordingChanged { register: Some(_) }
            ),
            "first event must be MacroRecordingChanged(Some)"
        );
        assert!(
            matches!(events[1], StateEvent::JumpAdded { .. }),
            "second event must be JumpAdded"
        );
        assert!(
            matches!(
                events[2],
                StateEvent::MacroRecordingChanged { register: None }
            ),
            "third event must be MacroRecordingChanged(None)"
        );
    }
}
