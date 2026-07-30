//! LRU compilation cache for compiled VimRegex patterns.
//!
//! Maintains an 8-entry thread-local cache keyed on (pattern, magic_mode).
//! Entries are `Rc<VimRegex>` to allow shared ownership. Each entry also
//! holds a persisted `DfaCache` so lazy-DFA states survive across searches.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use compact_str::CompactString;

use crate::cache::Cache;
use crate::engines::lazy_dfa::DfaCache;
use crate::MagicMode;

use super::VimRegex;

// ═══════════════════════════════════════════════════════════════════════════════
// COMPILE CACHE — 8-ENTRY LRU
// ═══════════════════════════════════════════════════════════════════════════════

const COMPILE_CACHE_CAPACITY: usize = 8;

struct CacheEntry {
    pattern: CompactString,
    magic: MagicMode,
    regex: Rc<VimRegex>,
    /// Persisted DFA cache for this compiled regex. Taken by `create_cache_seeded`
    /// and returned on `CacheWithGuard` drop. Avoids per-search DFA reconstruction.
    dfa_cache: RefCell<Option<DfaCache>>,
}

pub(crate) struct CompileCache {
    entries: Vec<CacheEntry>,
}

impl CompileCache {
    fn new() -> Self {
        Self {
            entries: Vec::with_capacity(COMPILE_CACHE_CAPACITY),
        }
    }

    pub(crate) fn get(&mut self, pattern: &str, magic: MagicMode) -> Option<Rc<VimRegex>> {
        let pos = self
            .entries
            .iter()
            .position(|e| e.pattern.as_str() == pattern && e.magic == magic)?;
        if pos > 0 {
            let entry = self.entries.remove(pos);
            self.entries.insert(0, entry);
        }
        Some(Rc::clone(&self.entries[0].regex))
    }

    pub(crate) fn insert(
        &mut self,
        pattern: &str,
        magic: MagicMode,
        regex: VimRegex,
    ) -> Rc<VimRegex> {
        let rc = Rc::new(regex);
        if self.entries.len() >= COMPILE_CACHE_CAPACITY {
            self.entries.pop();
        }
        self.entries.insert(
            0,
            CacheEntry {
                pattern: CompactString::new(pattern),
                magic,
                regex: Rc::clone(&rc),
                dfa_cache: RefCell::new(None),
            },
        );
        rc
    }
}

thread_local! {
    pub(crate) static COMPILE_CACHE: RefCell<CompileCache> = RefCell::new(CompileCache::new());
}

// ═══════════════════════════════════════════════════════════════════════════════
// DFA CACHE PERSISTENCE
// ═══════════════════════════════════════════════════════════════════════════════

/// Take the persisted `DfaCache` from the compile cache entry for `regex`.
pub(crate) fn take_dfa_cache(regex: &Rc<VimRegex>) -> Option<DfaCache> {
    COMPILE_CACHE.with(|cache| {
        let cache = cache.borrow();
        cache
            .entries
            .iter()
            .find(|e| Rc::ptr_eq(&e.regex, regex))
            .and_then(|e| e.dfa_cache.borrow_mut().take())
    })
}

/// Return a `DfaCache` to the compile cache entry for `regex`.
pub(crate) fn return_dfa_cache(regex: &Rc<VimRegex>, dfa: DfaCache) {
    if dfa.is_thrashing() {
        return;
    }
    COMPILE_CACHE.with(|cache| {
        let Ok(cache) = cache.try_borrow() else {
            return;
        };
        if let Some(entry) = cache.entries.iter().find(|e| Rc::ptr_eq(&e.regex, regex)) {
            *entry.dfa_cache.borrow_mut() = Some(dfa);
        }
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// CACHE WITH GUARD — RAII DFA CACHE RETURN
// ═══════════════════════════════════════════════════════════════════════════════

/// A `Cache` bundled with an RAII guard that returns the DFA cache to the
/// compile cache on drop.
///
/// # Thread Safety
///
/// `CacheWithGuard` is intentionally `!Send` because it holds an
/// `Rc<VimRegex>` referencing the thread-local compile cache. It must
/// be used on the same thread that created it. If you need to send a
/// cache across threads, use [`Cache`] directly (which is `Send`).
pub struct CacheWithGuard {
    pub(crate) cache: Cache,
    pub(super) regex: Rc<VimRegex>,
}

impl Deref for CacheWithGuard {
    type Target = Cache;
    fn deref(&self) -> &Cache {
        &self.cache
    }
}

impl DerefMut for CacheWithGuard {
    fn deref_mut(&mut self) -> &mut Cache {
        &mut self.cache
    }
}

impl Drop for CacheWithGuard {
    fn drop(&mut self) {
        if let Some(dfa) = self.cache.dfa.take() {
            return_dfa_cache(&self.regex, dfa);
        }
    }
}
