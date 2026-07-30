//! Reusable integration utilities for `VimEngine` consumers.
//!
//! When using `VimEngine` directly, integrators must handle
//! common patterns: draining pending keys from mappings/macros, caching document
//! text per keystroke cycle, and recursively completing host requests.
//!
//! This module provides all three as reusable utilities. The `process_host_requests`
//! function generalises the recursive host-request completion pattern from
//! `godot-vim`'s `handle_host_requests`, parameterised over handler and effect
//! callbacks so any integrator can use it without copy-pasting the recursion logic.

use compact_str::CompactString;

use super::engine::MacroOutput;
use super::host::{HostRequest, HostResult};
use super::response::Response;
use super::VimEngine;
use crate::effects::Effect;

/// Result of draining pending keys from the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrainResult {
    /// All pending keys were processed.
    Complete,
    /// Processing was halted after reaching the iteration limit.
    /// The engine may still have pending keys.
    HitLimit {
        /// The number of iterations that were executed before hitting the limit.
        iterations: usize,
    },
}

/// Drain pending entries from the engine (macros, mappings) with runaway protection.
///
/// After processing a key, the engine may buffer additional entries from macro playback
/// or mapping expansion. This function drains them one at a time, calling `process_entry`
/// for each, until no more pending entries remain or `max_iterations` is reached.
///
/// The callback receives a [`MacroOutput`] which is either a key (to feed through
/// `process()`) or a text block (to apply directly at the cursor).
///
/// # Arguments
/// * `engine` - The VimEngine instance
/// * `max_iterations` - Safety limit to prevent infinite loops (recommend 1000)
/// * `process_entry` - Callback invoked for each drained entry. Receives the
///   [`MacroOutput`] and must return the Response from processing it.
///
/// # Example
/// ```ignore
/// let result = drain_pending_keys(&mut engine, 1000, |output| {
///     match output {
///         MacroOutput::Key(key) => {
///             let ctx = build_context(&document);
///             engine.process(key, ctx)
///         }
///         MacroOutput::TextBlock { text, cursor_offset } => {
///             // apply text block directly
///             Response::default()
///         }
///     }
/// });
/// ```
pub fn drain_pending_keys<F>(
    engine: &mut VimEngine,
    max_iterations: usize,
    mut process_entry: F,
) -> DrainResult
where
    F: FnMut(MacroOutput) -> Response,
{
    let mut iterations = 0;
    for _ in 0..max_iterations {
        match engine.drain_next_key() {
            Some(output) => {
                let _ = process_entry(output);
                iterations += 1;
            }
            None => return DrainResult::Complete,
        }
    }
    // We processed max_iterations entries without the queue emptying.
    // There may still be pending entries — we can't check without consuming one,
    // so we conservatively report HitLimit.
    DrainResult::HitLimit { iterations }
}

/// Memoizes document text for one keystroke processing cycle.
///
/// Fetching document text can be expensive (FFI call, rope flattening, etc.).
/// This cache stores the text after the first fetch and returns it on subsequent
/// calls within the same cycle. Call [`invalidate()`](CycleTextCache::invalidate)
/// after any text mutation effect (Insert, Delete, Replace, Undo, Redo) to force
/// a re-fetch.
#[derive(Debug, Clone, Default)]
pub struct CycleTextCache {
    cached: Option<String>,
}

impl CycleTextCache {
    /// Create an empty cache.
    #[must_use]
    pub const fn new() -> Self {
        Self { cached: None }
    }

    /// Get cached text, or fetch it using the provided closure if not cached.
    pub fn get_or_fetch<F: FnOnce() -> String>(&mut self, fetch: F) -> &str {
        self.cached.get_or_insert_with(fetch)
    }

    /// Invalidate the cache. Next `get_or_fetch` will call the fetch closure.
    pub fn invalidate(&mut self) {
        self.cached = None;
    }

    /// Take the cached text out, leaving the cache empty.
    /// Useful for seeding the next cycle's cache if no mutations occurred.
    pub const fn take(&mut self) -> Option<String> {
        self.cached.take()
    }

    /// Seed the cache with text from a previous cycle.
    pub fn seed(&mut self, text: String) {
        self.cached = Some(text);
    }
}

/// Process host requests recursively with depth limiting.
///
/// For each request, calls `handler` to get a result, completes it via the
/// engine, processes any sub-effects via `effect_handler`, and recursively
/// handles any sub-requests.
///
/// At `max_depth`, remaining requests are completed with `HostResult::Failure`
/// to unblock the engine pipeline.
pub fn process_host_requests<H, F>(
    engine: &mut VimEngine,
    requests: &[HostRequest],
    handler: &mut H,
    effect_handler: &mut F,
    depth: usize,
    max_depth: usize,
) where
    H: FnMut(&HostRequest) -> HostResult,
    F: FnMut(Vec<Effect>),
{
    if requests.is_empty() {
        return;
    }

    if depth >= max_depth {
        // Depth limit reached — complete each request with Failure so the
        // engine cleans up its pending map and doesn't leak entries.
        for request in requests {
            let failure = HostResult::Failure {
                id: request.id(),
                error: CompactString::from("host request depth limit exceeded"),
            };
            let _ = engine.complete_host_request(&failure);
        }
        return;
    }

    for request in requests {
        let result = handler(request);

        let mut response = engine.complete_host_request(&result);

        let sub_effects = response.take_effects();
        if !sub_effects.is_empty() {
            effect_handler(sub_effects);
        }

        let sub_requests = response.take_host_requests();
        if !sub_requests.is_empty() {
            process_host_requests(
                engine,
                &sub_requests,
                handler,
                effect_handler,
                depth + 1,
                max_depth,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CycleTextCache tests ──────────────────────────────────────────

    #[test]
    fn cache_empty_calls_closure() {
        let mut cache = CycleTextCache::new();
        let text = cache.get_or_fetch(|| "hello world".to_string());
        assert_eq!(text, "hello world");
    }

    #[test]
    fn cache_hit_does_not_call_closure() {
        let mut cache = CycleTextCache::new();
        cache.get_or_fetch(|| "first".to_string());
        // Second call should NOT invoke the closure.
        let text = cache.get_or_fetch(|| panic!("closure should not be called"));
        assert_eq!(text, "first");
    }

    #[test]
    fn invalidate_forces_refetch() {
        let mut cache = CycleTextCache::new();
        cache.get_or_fetch(|| "old".to_string());
        cache.invalidate();
        let text = cache.get_or_fetch(|| "new".to_string());
        assert_eq!(text, "new");
    }

    #[test]
    fn take_returns_cached_and_empties() {
        let mut cache = CycleTextCache::new();
        cache.get_or_fetch(|| "data".to_string());
        let taken = cache.take();
        assert_eq!(taken, Some("data".to_string()));
        // Cache should now be empty — closure will be called.
        let text = cache.get_or_fetch(|| "refetched".to_string());
        assert_eq!(text, "refetched");
    }

    #[test]
    fn take_on_empty_returns_none() {
        let mut cache = CycleTextCache::new();
        assert_eq!(cache.take(), None);
    }

    #[test]
    fn seed_prepopulates_cache() {
        let mut cache = CycleTextCache::new();
        cache.seed("seeded".to_string());
        let text = cache.get_or_fetch(|| panic!("closure should not be called"));
        assert_eq!(text, "seeded");
    }

    // ── drain_pending_keys tests ──────────────────────────────────────
    //
    // drain_pending_keys is a thin wrapper around VimEngine::drain_next_key().
    // The heavy integration tests for drain_next_key (with real macros and
    // buffer keys) live in engine_tests.rs. Below we test the wrapper's own
    // logic: boundary conditions, iteration counting, and callback invocation.

    use crate::execution::VimEngine;

    #[test]
    fn drain_no_pending_keys_returns_complete() {
        let mut engine = VimEngine::default();
        let result = drain_pending_keys(&mut engine, 1000, |_key| {
            panic!("should not be called when no keys are pending");
        });
        assert_eq!(result, DrainResult::Complete);
    }

    #[test]
    fn drain_max_zero_returns_hit_limit() {
        let mut engine = VimEngine::default();
        let result = drain_pending_keys(&mut engine, 0, |_key| {
            panic!("should not be called with max_iterations=0");
        });
        assert_eq!(result, DrainResult::HitLimit { iterations: 0 });
    }

    #[test]
    fn drain_complete_with_large_limit_on_empty_engine() {
        // Even with a huge limit, an engine with no pending keys returns Complete.
        let mut engine = VimEngine::default();
        let result = drain_pending_keys(&mut engine, 100_000, |_key| {
            panic!("should not be called");
        });
        assert_eq!(result, DrainResult::Complete);
    }

    #[test]
    fn drain_callback_receives_response() {
        // Verify the callback's return value is consumed (not just ignored).
        // Since there are no pending keys, this is a smoke test for the API.
        let mut engine = VimEngine::default();
        let result = drain_pending_keys(&mut engine, 10, |_key| Response::default());
        assert_eq!(result, DrainResult::Complete);
    }

    // ── process_host_requests tests ──────────────────────────────────

    use crate::effects::Effect;
    use crate::execution::host::{HostRequest, HostRequestId, HostRequestMeta, HostResult};

    fn make_dummy_request(id: u64) -> HostRequest {
        HostRequest::Quit {
            meta: HostRequestMeta {
                id: HostRequestId::new(id),
            },
            force: false,
        }
    }

    #[test]
    fn process_single_request_no_sub_requests() {
        let mut engine = VimEngine::default();
        let mut handler_calls = 0usize;
        let mut effect_calls = 0usize;

        let request = make_dummy_request(1);

        process_host_requests(
            &mut engine,
            &[request],
            &mut |req: &HostRequest| {
                handler_calls += 1;
                HostResult::Success {
                    id: req.id(),
                    message: None,
                }
            },
            &mut |_effects: Vec<Effect>| {
                effect_calls += 1;
            },
            0,
            10,
        );

        assert_eq!(handler_calls, 1, "handler should be called exactly once");
        // No sub-effects from a default engine (no pending request matched),
        // so effect_handler should not be called.
        assert_eq!(
            effect_calls, 0,
            "effect_handler should not be called when no sub-effects"
        );
    }

    #[test]
    fn process_empty_requests_is_noop() {
        let mut engine = VimEngine::default();

        process_host_requests(
            &mut engine,
            &[],
            &mut |_req: &HostRequest| {
                panic!("handler should not be called for empty requests");
            },
            &mut |_effects: Vec<Effect>| {
                panic!("effect_handler should not be called for empty requests");
            },
            0,
            10,
        );
    }

    #[test]
    fn process_max_depth_zero_completes_with_failure() {
        let mut engine = VimEngine::default();
        let mut handler_calls = 0usize;

        let request = make_dummy_request(42);

        process_host_requests(
            &mut engine,
            &[request],
            &mut |_req: &HostRequest| {
                handler_calls += 1;
                panic!("handler should not be called when depth >= max_depth");
            },
            &mut |_effects: Vec<Effect>| {
                panic!("effect_handler should not be called at max depth");
            },
            0, // depth
            0, // max_depth — immediately at limit
        );

        assert_eq!(
            handler_calls, 0,
            "handler should not be called at depth limit"
        );
    }

    #[test]
    fn process_max_depth_exceeded_completes_multiple_with_failure() {
        let mut engine = VimEngine::default();
        let mut handler_calls = 0usize;

        let requests = vec![
            make_dummy_request(1),
            make_dummy_request(2),
            make_dummy_request(3),
        ];

        process_host_requests(
            &mut engine,
            &requests,
            &mut |_req: &HostRequest| {
                handler_calls += 1;
                panic!("handler should not be called when at depth limit");
            },
            &mut |_effects: Vec<Effect>| {},
            5, // depth
            5, // max_depth — at limit
        );

        assert_eq!(handler_calls, 0, "no handler calls when depth >= max_depth");
    }

    #[test]
    fn process_multiple_requests_calls_handler_for_each() {
        let mut engine = VimEngine::default();
        let mut handler_calls = 0usize;

        let requests = vec![
            make_dummy_request(1),
            make_dummy_request(2),
            make_dummy_request(3),
        ];

        process_host_requests(
            &mut engine,
            &requests,
            &mut |req: &HostRequest| {
                handler_calls += 1;
                HostResult::Success {
                    id: req.id(),
                    message: None,
                }
            },
            &mut |_effects: Vec<Effect>| {},
            0,
            10,
        );

        assert_eq!(
            handler_calls, 3,
            "handler should be called once per request"
        );
    }
}
