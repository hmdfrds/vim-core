//! Three-layer keymap system.
//!
//! Provides user-customizable key mappings over core defaults,
//! with an optional buffer-local overlay for per-buffer mappings.

use super::{
    Key, KeyClass, KeyEvent, MappingEntry, MappingFlags, MappingKind, MappingOwner, MappingTrie,
    TrieLookup, CORE_KEYMAP,
};
use crate::primitives::Mode;
use ahash::AHashMap;
use arrayvec::ArrayVec;
use compact_str::CompactString;
use smart_default::SmartDefault;

// ─────────────────────────────────────────────────────────────────────────────
// ModeMap<T> — type-safe per-mode storage
// ─────────────────────────────────────────────────────────────────────────────

/// Per-mode storage indexed by `MappingMode`.
///
/// A thin newtype over `[T; MappingMode::COUNT]` that implements
/// `Index<MappingMode>` / `IndexMut<MappingMode>`. This makes all
/// mode-indexed access bounds-safe by construction — the compiler
/// proves the index is in range, so no `#[allow(clippy::indexing_slicing)]`
/// is ever needed.
#[derive(Debug, Clone)]
pub struct ModeMap<T>([T; MappingMode::COUNT]);

impl<T: Default> Default for ModeMap<T> {
    fn default() -> Self {
        Self(std::array::from_fn(|_| T::default()))
    }
}

impl<T> std::ops::Index<MappingMode> for ModeMap<T> {
    type Output = T;
    #[inline]
    fn index(&self, mm: MappingMode) -> &T {
        // SAFETY: idx() returns 0..COUNT by the exhaustive match in MappingMode::idx().
        // The array has exactly COUNT elements. This is bounds-safe by construction.
        #[allow(
            clippy::indexing_slicing,
            reason = "idx() is bounded by exhaustive match over MappingMode"
        )]
        &self.0[mm.idx()]
    }
}

impl<T> std::ops::IndexMut<MappingMode> for ModeMap<T> {
    #[inline]
    fn index_mut(&mut self, mm: MappingMode) -> &mut T {
        #[allow(
            clippy::indexing_slicing,
            reason = "idx() is bounded by exhaustive match over MappingMode"
        )]
        &mut self.0[mm.idx()]
    }
}

impl<T> ModeMap<T> {
    /// Iterate over all values.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    /// Iterate mutably over all values.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.0.iter_mut()
    }

    /// Consume into an array.
    #[must_use]
    pub fn into_array(self) -> [T; MappingMode::COUNT] {
        self.0
    }
}

impl<'a, T> IntoIterator for &'a ModeMap<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut ModeMap<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

/// Opaque per-buffer mapping storage.
///
/// The shell keeps one of these per buffer and swaps via
/// `Keymap::set_buffer_mappings()` / `Keymap::take_buffer_mappings()`.
#[derive(Debug, Clone, Default)]
pub struct BufferMappings {
    tries: ModeMap<MappingTrie>,
}

impl BufferMappings {
    /// Check if all tries are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tries.iter().all(MappingTrie::is_empty)
    }

    /// Iterate over all mappings in a specific mode.
    ///
    /// Returns `(lhs, entry)` pairs. Used for `:map <buffer>` listing.
    #[must_use]
    pub fn iter_mode(&self, mm: MappingMode) -> Vec<(Vec<KeyEvent>, &MappingEntry)> {
        self.tries[mm].entries()
    }
}

/// Error from a mapping insertion attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MapError {
    /// E227: mapping already exists from a different owner and either the
    /// existing or new mapping has the `<unique>` flag set.
    UniqueConflict,
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UniqueConflict => write!(f, "E227: mapping already exists"),
        }
    }
}

impl std::error::Error for MapError {}

/// Maximum length of a key sequence for mappings.
pub const MAX_KEY_SEQUENCE_LEN: usize = 8;

/// A sequence of key events (for mappings).
pub type KeySequence = ArrayVec<KeyEvent, MAX_KEY_SEQUENCE_LEN>;

/// Flat mode discriminant for user mapping storage.
///
/// Unlike [`Mode`], this has no payload — `Visual(VisualType)` and
/// `OperatorPending(Operator)` collapse to single variants. This is the
/// correct key for mapping lookup: `:vmap` applies to all visual subtypes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum MappingMode {
    /// Normal mode mappings (`:nmap`).
    Normal,
    /// Visual + Select mode mappings (`:vmap`).
    Visual,
    /// Operator-pending mode mappings (`:omap`).
    Operator,
    /// Insert mode mappings (`:imap`).
    Insert,
    /// Command-line mode mappings (`:cmap`).
    Command,
    /// Visual-only mode mappings (`:xmap`).
    /// Active in Visual mode but NOT in Select mode.
    VisualOnly,
    /// Select-only mode mappings (`:smap`).
    /// Active in Select mode but NOT in Visual mode.
    SelectOnly,
}

impl MappingMode {
    /// Total number of mapping modes (for array sizing).
    pub const COUNT: usize = 7;

    /// All mapping modes in index order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Normal,
        Self::Visual,
        Self::Operator,
        Self::Insert,
        Self::Command,
        Self::VisualOnly,
        Self::SelectOnly,
    ];

    /// Array index for this mode. Stable, used by `Keymap` internal storage.
    #[inline]
    #[must_use]
    pub const fn idx(self) -> usize {
        match self {
            Self::Normal => 0,
            Self::Visual => 1,
            Self::Operator => 2,
            Self::Insert => 3,
            Self::Command => 4,
            Self::VisualOnly => 5,
            Self::SelectOnly => 6,
        }
    }

    /// Convert from [`Mode`] to [`MappingMode`].
    ///
    /// Replace and VirtualReplace use Insert-mode mappings (`:imap` applies),
    /// matching real Vim behaviour. Returns `None` only for modes that have no
    /// user-mapping layer (currently none).
    #[must_use]
    pub const fn from_mode(mode: Mode) -> Option<Self> {
        match mode {
            Mode::Normal => Some(Self::Normal),
            Mode::Visual(_) => Some(Self::Visual),
            Mode::OperatorPending(_) => Some(Self::Operator),
            Mode::Insert | Mode::Replace | Mode::VirtualReplace => Some(Self::Insert),
            Mode::Select(_) => Some(Self::Visual),
            Mode::CommandLine => Some(Self::Command),
        }
    }

    /// Return all `MappingMode`s that should be consulted for a given [`Mode`].
    ///
    /// For Visual modes, this returns both `Visual` (`:vmap`) and `VisualOnly`
    /// (`:xmap`). For Select modes, both `Visual` (`:vmap`) and `SelectOnly`
    /// (`:smap`). This allows the lookup layer to check mode-specific tries.
    #[must_use]
    pub const fn all_for_mode(mode: Mode) -> &'static [Self] {
        match mode {
            Mode::Normal => &[Self::Normal],
            Mode::Visual(_) => &[Self::Visual, Self::VisualOnly],
            Mode::OperatorPending(_) => &[Self::Operator],
            Mode::Insert | Mode::Replace | Mode::VirtualReplace => &[Self::Insert],
            Mode::Select(_) => &[Self::Visual, Self::SelectOnly],
            Mode::CommandLine => &[Self::Command],
        }
    }
}

/// Three-layer keymap: Buffer-local overlay → User layer → Core layer.
///
/// Lookup order: buffer-local mappings first, then global user mappings,
/// then core defaults. Buffer-local overlay is managed by the shell
/// via `set_buffer_mappings()` / `take_buffer_mappings()`.
///
/// # Example
/// ```ignore
/// use vim_core::keymap::{Keymap, KeyEvent, KeyClass, MappingMode};
/// use vim_core::state::Mode;
///
/// let keymap = Keymap::default();
/// let class = keymap.classify(KeyEvent::char('d'), Mode::Normal);
/// assert_eq!(class, KeyClass::Operator);
/// ```
#[derive(Debug, Clone, SmartDefault)]
pub struct Keymap {
    /// User-defined remappings per mode (trie-backed for multi-key LHS).
    user: ModeMap<MappingTrie>,
    /// Buffer-local mapping overlay (per mode).
    /// `None` = no buffer-local mappings active.
    /// Shell swaps these via `set_buffer_mappings()` / `take_buffer_mappings()`.
    buf: ModeMap<Option<MappingTrie>>,
    /// Leader key — used to resolve `<Leader>` placeholders at mapping
    /// definition time. Default: backslash (`\`).
    #[default(KeyEvent::char('\\'))]
    leader: KeyEvent,
    /// Local leader key — used to resolve `<LocalLeader>` placeholders at
    /// mapping definition time. Default: backslash (`\`).
    /// Typically set to `,` for filetype-specific plugin mappings.
    #[default(KeyEvent::char('\\'))]
    local_leader: KeyEvent,
    /// Maximum recursive mapping expansion depth. Prevents infinite
    /// loops in recursive mappings. Corresponds to Vim's `maxmapdepth`
    /// option. Default: 1000.
    #[default(1000)]
    max_map_depth: u32,
    /// `<Plug>` name registry — maps `u32` ids to human-readable names.
    ///
    /// Used by `Key::Plug(id)` to preserve `Copy` on `Key` while still
    /// supporting named plugin mappings like `<Plug>(surround-word)`.
    plug_names: super::NameRegistry,
    /// `<Action>` name registry — maps `u32` ids to host action names.
    ///
    /// Used by `Key::Action(id)` for IdeaVim-style host action bridge.
    action_names: super::NameRegistry,
    /// FileType-specific mapping overlays.
    ///
    /// Each entry maps a file type (e.g., "rust", "python") to per-mode
    /// mapping tries. Active when the engine's current filetype matches.
    #[default(AHashMap::new())]
    filetype_maps: AHashMap<CompactString, ModeMap<MappingTrie>>,
    /// Currently active file type for filetype-specific mapping lookup.
    /// Set by the engine via `set_filetype()`.
    #[default(None)]
    active_filetype: Option<CompactString>,
}

impl Keymap {
    /// Create a new keymap with default (empty user layer).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // ═══════════════════════════════════════════════════════════════════
    // Private resolver: MappingMode → HashMap
    // ═══════════════════════════════════════════════════════════════════

    /// Get the user mapping trie for a given mapping mode (immutable).
    #[inline]
    fn user_table(&self, mm: MappingMode) -> &MappingTrie {
        &self.user[mm]
    }

    /// Get the user mapping trie for a given mapping mode (mutable).
    #[inline]
    fn user_table_mut(&mut self, mm: MappingMode) -> &mut MappingTrie {
        &mut self.user[mm]
    }

    /// Get the buffer-local mapping trie for a given mapping mode (immutable).
    #[inline]
    fn buf_table(&self, mm: MappingMode) -> Option<&MappingTrie> {
        self.buf[mm].as_ref()
    }

    /// Get or create the buffer-local mapping trie for a given mode.
    #[inline]
    fn buf_table_mut(&mut self, mm: MappingMode) -> &mut MappingTrie {
        self.buf[mm].get_or_insert_with(MappingTrie::default)
    }

    /// Resolve `<Leader>` placeholders in a key sequence.
    ///
    /// Replaces any `Key::Leader` with the current leader key value.
    /// Used by both `map`/`unmap` and `map_buffer`/`unmap_buffer`.
    fn resolve_leader(&self, keys: &[KeyEvent]) -> smallvec::SmallVec<[KeyEvent; 8]> {
        keys.iter()
            .map(|k| self.resolve_leader_single(*k))
            .collect()
    }

    /// Resolve `<Leader>` and `<LocalLeader>` for a single key event.
    fn resolve_leader_single(&self, event: KeyEvent) -> KeyEvent {
        if event.key == Key::Leader {
            self.leader
        } else if event.key == Key::LocalLeader {
            self.local_leader
        } else {
            event
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Classification
    // ═══════════════════════════════════════════════════════════════════

    /// Classify a key event in the given mode.
    ///
    /// Lookup order:
    /// 1. Escape-class keys (mode-independent, checked in `classify_core`)
    /// 2. Buffer-local overlay — if a buffer-local mapping exists, classify
    ///    the **first key** of the target sequence through the core keymap.
    /// 3. FileType-specific overlay — if a filetype is set and matches.
    /// 4. Global user layer — same classification-by-target semantics.
    /// 5. Core layer (Vim defaults)
    #[must_use]
    pub fn classify(&self, event: KeyEvent, mode: Mode) -> KeyClass {
        // Check buffer-local mappings first
        if let Some(entry) = self.get_buf_mapping(event, mode) {
            if let Some(first_key) = entry.sequence().first() {
                return self.classify_core(*first_key, mode);
            }
        }
        // Check filetype-specific mappings
        if let Some(entry) = self.get_filetype_mapping(event, mode) {
            if let Some(first_key) = entry.sequence().first() {
                return self.classify_core(*first_key, mode);
            }
        }
        // Check global user mappings
        if let Some(entry) = self.get_global_user_mapping(event, mode) {
            if let Some(first_key) = entry.sequence().first() {
                return self.classify_core(*first_key, mode);
            }
        }
        // Fall through to core
        self.classify_core(event, mode)
    }

    /// Classify using only the core keymap.
    pub(crate) fn classify_core(&self, event: KeyEvent, mode: Mode) -> KeyClass {
        use super::Key;

        // Escape-class keys are mode-independent — always Escape regardless of mode.
        // This covers: Escape key, Ctrl-C, Ctrl-[
        if event.key == Key::Escape || event == KeyEvent::ctrl('c') || event == KeyEvent::ctrl('[')
        {
            return KeyClass::Escape;
        }

        match mode {
            Mode::Normal => CORE_KEYMAP.classify(event, MappingMode::Normal),
            Mode::Visual(_) => CORE_KEYMAP.classify(event, MappingMode::Visual),
            Mode::OperatorPending(_) => CORE_KEYMAP.classify(event, MappingMode::Operator),
            // Select: shares Visual keymap tables per `:vmap` semantics.
            // SelectModeHandler handles everything before the core keymap is
            // consulted; this path is used by would_handle_key and mapping expansion.
            Mode::Select(_) => CORE_KEYMAP.classify(event, MappingMode::Visual),
            // Insert, Replace, CommandLine: chars handled by VimEngine directly
            Mode::Insert | Mode::Replace | Mode::VirtualReplace | Mode::CommandLine => {
                KeyClass::Unknown
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Key interest set accessors
    // ═══════════════════════════════════════════════════════════════════

    /// Iterate all (key, class) pairs from the core keymap for a given mode.
    pub fn core_entries(&self, mm: MappingMode) -> impl Iterator<Item = (&KeyEvent, &KeyClass)> {
        CORE_KEYMAP.entries(mm)
    }

    /// Collect first-key entries from all user mapping tries across all modes.
    ///
    /// Returns `(key_event, mapping_mode)` pairs for the root level of each
    /// trie. Covers global user mappings, buffer-local mappings, and all
    /// filetype-specific mappings (conservatively included regardless of the
    /// currently active filetype).
    #[must_use]
    pub fn user_mapping_first_keys(&self) -> Vec<(KeyEvent, MappingMode)> {
        let mut result = Vec::new();
        for mm in MappingMode::ALL {
            // Global user mappings
            for (lhs, _entry) in self.user[mm].entries() {
                if let Some(&first) = lhs.first() {
                    result.push((first, mm));
                }
            }
            // Buffer-local mappings
            if let Some(trie) = self.buf[mm].as_ref() {
                for (lhs, _entry) in trie.entries() {
                    if let Some(&first) = lhs.first() {
                        result.push((first, mm));
                    }
                }
            }
            // Filetype mappings (all filetypes, conservatively)
            for ft_maps in self.filetype_maps.values() {
                for (lhs, _entry) in ft_maps[mm].entries() {
                    if let Some(&first) = lhs.first() {
                        result.push((first, mm));
                    }
                }
            }
        }
        result
    }

    // ═══════════════════════════════════════════════════════════════════
    // User mapping queries
    // ═══════════════════════════════════════════════════════════════════

    /// Check if a single key has a user mapping in the given mode.
    /// Checks buffer-local first, then global.
    #[must_use]
    pub fn has_user_mapping(&self, event: KeyEvent, mode: Mode) -> bool {
        self.get_user_mapping(event, mode).is_some()
    }

    /// Get single-key user mapping if it exists.
    /// Checks buffer-local first, then filetype-specific, then global.
    /// For Visual/Select modes, also checks VisualOnly/SelectOnly tries.
    #[must_use]
    pub fn get_user_mapping(&self, event: KeyEvent, mode: Mode) -> Option<&MappingEntry> {
        self.get_buf_mapping(event, mode)
            .or_else(|| self.get_filetype_mapping(event, mode))
            .or_else(|| self.get_global_user_mapping(event, mode))
    }

    /// Check if a single key has a buffer-local mapping in the given mode.
    #[must_use]
    pub fn has_buffer_mapping(&self, event: KeyEvent, mode: Mode) -> bool {
        self.get_buf_mapping(event, mode).is_some()
    }

    /// Get single-key buffer-local mapping if it exists.
    #[must_use]
    pub fn get_buffer_mapping(&self, event: KeyEvent, mode: Mode) -> Option<&MappingEntry> {
        self.get_buf_mapping(event, mode)
    }

    /// Get single-key buffer-local mapping (internal).
    ///
    /// Resolves `<Leader>` in the query key, matching `map_buffer()` semantics.
    /// Checks all relevant modes (e.g., Visual + VisualOnly for visual mode).
    fn get_buf_mapping(&self, event: KeyEvent, mode: Mode) -> Option<&MappingEntry> {
        let resolved = self.resolve_leader_single(event);
        for &mm in MappingMode::all_for_mode(mode) {
            if let Some(entry) = self
                .buf_table(mm)
                .and_then(|trie| trie.get_single(resolved))
            {
                return Some(entry);
            }
        }
        None
    }

    /// Get single-key filetype-specific mapping (internal).
    ///
    /// Returns `None` if no filetype is active or no mapping exists.
    /// Checks all relevant modes (e.g., Visual + VisualOnly for visual mode).
    fn get_filetype_mapping(&self, event: KeyEvent, mode: Mode) -> Option<&MappingEntry> {
        let ft = self.active_filetype.as_deref()?;
        let resolved = self.resolve_leader_single(event);
        let ft_maps = self.filetype_maps.get(ft)?;
        for &mm in MappingMode::all_for_mode(mode) {
            if let Some(entry) = ft_maps[mm].get_single(resolved) {
                return Some(entry);
            }
        }
        None
    }

    /// Get single-key global user mapping (internal).
    ///
    /// Resolves `<Leader>` in the query key, matching `map()` semantics.
    /// Checks all relevant modes (e.g., Visual + VisualOnly for visual mode).
    fn get_global_user_mapping(&self, event: KeyEvent, mode: Mode) -> Option<&MappingEntry> {
        let resolved = self.resolve_leader_single(event);
        for &mm in MappingMode::all_for_mode(mode) {
            if let Some(entry) = self.user_table(mm).get_single(resolved) {
                return Some(entry);
            }
        }
        None
    }

    /// List all user mappings for a mode (buffer-local + global).
    ///
    /// Returns `(lhs, entry, is_buffer_local)` triples.
    #[must_use]
    pub fn list_mappings(&self, mm: MappingMode) -> Vec<(Vec<KeyEvent>, &MappingEntry, bool)> {
        let mut results = Vec::new();
        // Buffer-local first
        if let Some(trie) = self.buf_table(mm) {
            for (lhs, entry) in trie.entries() {
                results.push((lhs, entry, true));
            }
        }
        // Global
        for (lhs, entry) in self.user_table(mm).entries() {
            results.push((lhs, entry, false));
        }
        results
    }

    /// List direct children of a prefix path across all trie layers.
    ///
    /// Returns `(continuation_key, entry)` pairs for mappings that are exactly
    /// one key deeper than `prefix`. Deduplication follows the standard
    /// priority: buffer-local > filetype-specific > global user. If the same
    /// continuation key exists in multiple layers, only the highest-priority
    /// entry is returned.
    ///
    /// Used by the which-key infobox to show available next keys after a prefix.
    #[must_use]
    pub fn list_continuations(
        &self,
        mm: MappingMode,
        prefix: &[KeyEvent],
    ) -> Vec<(KeyEvent, &MappingEntry)> {
        use ahash::AHashSet;
        let mut seen = AHashSet::new();
        let mut results = Vec::new();
        let depth = prefix.len();

        // Collect tries in priority order: buffer-local > filetype > global.
        let buf_trie = self.buf_table(mm);
        let ft_trie = self
            .active_filetype
            .as_deref()
            .and_then(|ft| self.filetype_maps.get(ft))
            .map(|ft_maps| &ft_maps[mm]);
        let global_trie = self.user_table(mm);

        for trie in buf_trie.into_iter().chain(ft_trie).chain(Some(global_trie)) {
            for (lhs, entry) in trie.entries() {
                if lhs.len() == depth + 1 && lhs.get(..depth) == Some(prefix) {
                    if let Some(&continuation_key) = lhs.get(depth) {
                        if seen.insert(continuation_key) {
                            results.push((continuation_key, entry));
                        }
                    }
                }
            }
        }

        results
    }

    /// Multi-key prefix lookup for the MappingExpander.
    /// Merges buffer-local, filetype-specific, and global results.
    #[must_use]
    pub fn lookup(&self, mm: MappingMode, prefix: &[KeyEvent]) -> TrieLookup<'_> {
        let buf_result = self
            .buf_table(mm)
            .map_or(TrieLookup::NoMatch, |t| t.lookup(prefix));
        let ft_result = self
            .active_filetype
            .as_deref()
            .and_then(|ft| self.filetype_maps.get(ft))
            .map_or(TrieLookup::NoMatch, |ft_maps| ft_maps[mm].lookup(prefix));
        let global_result = self.user_table(mm).lookup(prefix);
        // Merge: buf > filetype > global
        let buf_ft = merge_lookups(buf_result, ft_result);
        merge_lookups(buf_ft, global_result)
    }

    // ═══════════════════════════════════════════════════════════════════
    // Generic CRUD — single source of truth
    // ═══════════════════════════════════════════════════════════════════

    /// Insert a pre-built [`MappingEntry`] into the global user layer.
    ///
    /// Unlike [`map()`](Self::map), this method accepts a fully constructed
    /// entry so the caller can set a custom [`MappingOwner`] (e.g., for
    /// host extension mappings). The `from` key sequence is resolved for
    /// `<Leader>` placeholders before insertion.
    pub fn map_entry(&mut self, mm: MappingMode, from: &[KeyEvent], entry: MappingEntry) {
        let resolved = self.resolve_leader(from);
        self.user_table_mut(mm).insert(&resolved, entry);
    }

    /// Insert a mapping with `<unique>` conflict checking.
    ///
    /// If the new entry has `flags.unique` set and there is an existing
    /// mapping at the same LHS from a **different** owner, returns
    /// `Err(MapError::UniqueConflict)`. Same-owner overwrites are allowed.
    /// Non-unique mappings always succeed.
    ///
    /// # Errors
    ///
    /// Returns [`MapError::UniqueConflict`] (Vim's E227) when a mapping
    /// already exists at the resolved LHS, its owner differs from
    /// `entry.owner()`, and either the incoming `entry` or the existing one
    /// carries the `<unique>` flag. The trie is left untouched. Re-mapping the
    /// same LHS from the same owner, or over a non-`<unique>` mapping with a
    /// non-`<unique>` entry, succeeds.
    pub fn try_map_entry(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        entry: MappingEntry,
    ) -> Result<(), MapError> {
        let resolved = self.resolve_leader(from);
        let trie = self.user_table_mut(mm);

        // Check <unique> constraint: existing entry from different owner blocks insert.
        if entry.flags.unique {
            if let Some(existing) = trie.get_exact(&resolved) {
                if existing.owner() != entry.owner() {
                    return Err(MapError::UniqueConflict);
                }
            }
        }
        // Also check if existing entry is <unique> — protect it from foreign overwrite.
        if let Some(existing) = trie.get_exact(&resolved) {
            if existing.unique() && existing.owner() != entry.owner() {
                return Err(MapError::UniqueConflict);
            }
        }

        trie.insert(&resolved, entry);
        Ok(())
    }

    /// Add a user mapping for the given mode.
    pub fn map(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.map_with_expr(mm, from, to, kind, flags, None);
    }

    /// Add a user mapping with full flag support (including `<expr>` text).
    pub fn map_with_expr(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
        expr_text: Option<CompactString>,
    ) {
        let resolved = self.resolve_leader(from);
        self.user_table_mut(mm)
            .insert(&resolved, build_entry(to, kind, flags, expr_text));
    }

    /// Remove a user mapping from the given mode.
    ///
    /// Resolves `<Leader>` in `from` before lookup, matching `map()` semantics.
    pub fn unmap(&mut self, mm: MappingMode, from: &[KeyEvent]) -> Option<MappingEntry> {
        let resolved = self.resolve_leader(from);
        self.user_table_mut(mm).remove(&resolved)
    }

    /// Clear all user mappings for the given mode.
    pub fn clear(&mut self, mm: MappingMode) {
        self.user_table_mut(mm).clear();
    }

    /// Clear all user mappings across all modes.
    pub fn clear_all_mappings(&mut self) {
        for trie in &mut self.user {
            trie.clear();
        }
    }

    /// Get the current leader key.
    #[inline]
    #[must_use]
    pub const fn leader(&self) -> KeyEvent {
        self.leader
    }

    /// Set the leader key.
    ///
    /// Only affects mappings defined **after** this call.
    /// Existing mappings retain the leader key they were defined with.
    #[inline]
    pub const fn set_leader(&mut self, key: KeyEvent) {
        self.leader = key;
    }

    /// Get the current local leader key.
    #[inline]
    #[must_use]
    pub const fn local_leader(&self) -> KeyEvent {
        self.local_leader
    }

    /// Set the local leader key (`maplocalleader`).
    ///
    /// Only affects mappings defined **after** this call.
    /// Existing mappings retain the local leader key they were defined with.
    #[inline]
    pub const fn set_local_leader(&mut self, key: KeyEvent) {
        self.local_leader = key;
    }

    /// Get the current maximum mapping expansion depth (`maxmapdepth`).
    #[inline]
    #[must_use]
    pub const fn max_map_depth(&self) -> u32 {
        self.max_map_depth
    }

    /// Set the maximum mapping expansion depth (`maxmapdepth`).
    ///
    /// Controls how many times a recursive mapping can re-expand before
    /// the engine reports an error. Default is 1000.
    #[inline]
    pub const fn set_max_map_depth(&mut self, depth: u32) {
        self.max_map_depth = depth;
    }

    // ═══════════════════════════════════════════════════════════════════
    // <Plug> / <Action> registries
    // ═══════════════════════════════════════════════════════════════════

    /// Register a `<Plug>` name and return its `KeyEvent`.
    ///
    /// Idempotent: calling with the same name returns the same key.
    ///
    /// # Example
    /// ```ignore
    /// let plug = keymap.register_plug("surround-word");
    /// // plug == KeyEvent::plug(0)
    /// keymap.map(MappingMode::Normal, &[KeyEvent::char('S')], key_sequence(&[plug]), MappingKind::Recursive);
    /// ```
    pub fn register_plug(&mut self, name: &str) -> KeyEvent {
        let id = self.plug_names.register(name);
        KeyEvent::plug(id)
    }

    /// Get the human-readable name for a `<Plug>` id.
    #[must_use]
    pub fn plug_name(&self, id: u32) -> Option<&str> {
        self.plug_names.get_name(id)
    }

    /// Get the `<Plug>` id for a name, if registered.
    #[must_use]
    pub fn plug_id(&self, name: &str) -> Option<u32> {
        self.plug_names.get_id(name)
    }

    /// Register an `<Action>` name and return its `KeyEvent`.
    ///
    /// Idempotent: calling with the same name returns the same key.
    ///
    /// # Example
    /// ```ignore
    /// let action = keymap.register_action("ReformatCode");
    /// keymap.map(MappingMode::Normal, &[KeyEvent::char('=')], key_sequence(&[action]), MappingKind::NonRecursive);
    /// ```
    pub fn register_action(&mut self, name: &str) -> KeyEvent {
        let id = self.action_names.register(name);
        KeyEvent::action(id)
    }

    /// Get the human-readable name for an `<Action>` id.
    #[must_use]
    pub fn action_name(&self, id: u32) -> Option<&str> {
        self.action_names.get_name(id)
    }

    /// Get the `<Action>` id for a name, if registered.
    #[must_use]
    pub fn action_id(&self, name: &str) -> Option<u32> {
        self.action_names.get_id(name)
    }

    /// Borrow both name registries mutably for use by the key notation
    /// parser when resolving `<Action>(name)` and `<Plug>(name)`.
    ///
    /// Returns `(&mut plug_registry, &mut action_registry)`.
    #[inline]
    pub(crate) const fn registries_mut(
        &mut self,
    ) -> (&mut super::NameRegistry, &mut super::NameRegistry) {
        (&mut self.plug_names, &mut self.action_names)
    }

    // ═══════════════════════════════════════════════════════════════════
    // Convenience wrappers (delegate to generic CRUD)
    // ═══════════════════════════════════════════════════════════════════

    /// Add a user mapping for Normal mode (non-recursive, single-key shorthand).
    pub fn map_normal(&mut self, from: KeyEvent, to: Vec<KeyEvent>) {
        self.map(
            MappingMode::Normal,
            &[from],
            to,
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
    }

    /// Add a user mapping for Visual mode (non-recursive, single-key shorthand).
    pub fn map_visual(&mut self, from: KeyEvent, to: Vec<KeyEvent>) {
        self.map(
            MappingMode::Visual,
            &[from],
            to,
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
    }

    /// Add a user mapping for Operator-pending mode (non-recursive, single-key shorthand).
    pub fn map_operator(&mut self, from: KeyEvent, to: Vec<KeyEvent>) {
        self.map(
            MappingMode::Operator,
            &[from],
            to,
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
    }

    /// Add a user mapping for Insert mode (non-recursive, single-key shorthand).
    pub fn map_insert(&mut self, from: KeyEvent, to: Vec<KeyEvent>) {
        self.map(
            MappingMode::Insert,
            &[from],
            to,
            MappingKind::NonRecursive,
            MappingFlags::default(),
        );
    }

    /// Remove a Normal mode mapping.
    pub fn unmap_normal(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap(MappingMode::Normal, &[event])
    }

    /// Remove a Visual mode mapping.
    pub fn unmap_visual(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap(MappingMode::Visual, &[event])
    }

    /// Remove an Operator-pending mode mapping.
    pub fn unmap_operator(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap(MappingMode::Operator, &[event])
    }

    /// Remove an Insert mode mapping.
    pub fn unmap_insert(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap(MappingMode::Insert, &[event])
    }

    /// Clear all Normal mode mappings.
    pub fn clear_normal_mappings(&mut self) {
        self.clear(MappingMode::Normal);
    }

    /// Clear all Visual mode mappings.
    pub fn clear_visual_mappings(&mut self) {
        self.clear(MappingMode::Visual);
    }

    /// Clear all Operator-pending mode mappings.
    pub fn clear_operator_mappings(&mut self) {
        self.clear(MappingMode::Operator);
    }

    /// Clear all Insert mode mappings.
    pub fn clear_insert_mappings(&mut self) {
        self.clear(MappingMode::Insert);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Buffer-local CRUD
    // ═══════════════════════════════════════════════════════════════════

    /// Add a buffer-local mapping. Same semantics as `map()` but stored in the buffer overlay.
    pub fn map_buffer(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.map_buffer_with_expr(mm, from, to, kind, flags, None);
    }

    /// Add a buffer-local mapping with full flag support (including `<expr>` text).
    pub fn map_buffer_with_expr(
        &mut self,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
        expr_text: Option<CompactString>,
    ) {
        let resolved = self.resolve_leader(from);
        self.buf_table_mut(mm)
            .insert(&resolved, build_entry(to, kind, flags, expr_text));
    }

    /// Remove a buffer-local mapping.
    ///
    /// Resolves `<Leader>` in `from` before lookup, matching `map_buffer()` semantics.
    /// Returns `None` if no buffer-local trie exists for this mode, or if
    /// the key sequence is not found.
    pub fn unmap_buffer(&mut self, mm: MappingMode, from: &[KeyEvent]) -> Option<MappingEntry> {
        let resolved = self.resolve_leader(from);
        // Only remove from an existing trie — don't lazily create one.
        self.buf[mm]
            .as_mut()
            .and_then(|trie| trie.remove(&resolved))
    }

    /// Clear all buffer-local mappings for the given mode.
    pub fn clear_buffer(&mut self, mm: MappingMode) {
        self.buf[mm] = None;
    }

    /// Remove a Normal mode buffer-local mapping.
    pub fn unmap_buffer_normal(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap_buffer(MappingMode::Normal, &[event])
    }

    /// Remove a Visual mode buffer-local mapping.
    pub fn unmap_buffer_visual(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap_buffer(MappingMode::Visual, &[event])
    }

    /// Remove an Operator-pending mode buffer-local mapping.
    pub fn unmap_buffer_operator(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap_buffer(MappingMode::Operator, &[event])
    }

    /// Remove an Insert mode buffer-local mapping.
    pub fn unmap_buffer_insert(&mut self, event: KeyEvent) -> Option<MappingEntry> {
        self.unmap_buffer(MappingMode::Insert, &[event])
    }

    /// Clear all buffer-local mappings across all modes.
    pub fn clear_all_buffer_mappings(&mut self) {
        for slot in &mut self.buf {
            *slot = None;
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // FileType-specific mappings
    // ═══════════════════════════════════════════════════════════════════

    /// Set the currently active file type.
    ///
    /// Affects filetype-specific mapping lookup. Pass `None` to clear.
    pub fn set_filetype(&mut self, filetype: Option<CompactString>) {
        self.active_filetype = filetype;
    }

    /// Get the currently active file type.
    #[must_use]
    pub fn active_filetype(&self) -> Option<&str> {
        self.active_filetype.as_deref()
    }

    /// Add a filetype-specific mapping. Only active when the engine's filetype matches.
    pub fn map_filetype(
        &mut self,
        filetype: &str,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
    ) {
        self.map_filetype_with_expr(filetype, mm, from, to, kind, flags, None);
    }

    /// Add a filetype-specific mapping with full flag support (including `<expr>` text).
    #[allow(
        clippy::too_many_arguments,
        reason = "filetype param extends the base map_with_expr signature"
    )]
    pub fn map_filetype_with_expr(
        &mut self,
        filetype: &str,
        mm: MappingMode,
        from: &[KeyEvent],
        to: Vec<KeyEvent>,
        kind: MappingKind,
        flags: MappingFlags,
        expr_text: Option<CompactString>,
    ) {
        let resolved = self.resolve_leader(from);
        let ft_maps = self
            .filetype_maps
            .entry(CompactString::from(filetype))
            .or_default();
        ft_maps[mm].insert(&resolved, build_entry(to, kind, flags, expr_text));
    }

    /// Remove a filetype-specific mapping.
    ///
    /// Resolves `<Leader>` in `from` before lookup, matching `map_filetype()` semantics.
    /// Returns `None` if no filetype trie exists, or if the key sequence is not found.
    pub fn unmap_filetype(
        &mut self,
        filetype: &str,
        mm: MappingMode,
        from: &[KeyEvent],
    ) -> Option<MappingEntry> {
        let resolved = self.resolve_leader(from);
        self.filetype_maps
            .get_mut(filetype)
            .and_then(|ft_maps| ft_maps[mm].remove(&resolved))
    }

    /// Clear all filetype-specific mappings for a given filetype and mode.
    pub fn clear_filetype(&mut self, filetype: &str, mm: MappingMode) {
        if let Some(ft_maps) = self.filetype_maps.get_mut(filetype) {
            ft_maps[mm].clear();
        }
    }

    /// Clear all filetype-specific mappings across all filetypes.
    pub fn clear_all_filetype_mappings(&mut self) {
        self.filetype_maps.clear();
    }

    // ═══════════════════════════════════════════════════════════════════
    // Owner-based removal
    // ═══════════════════════════════════════════════════════════════════

    /// Remove all mappings owned by `owner` across all layers and modes.
    ///
    /// Walks every layer:
    /// - All modes in the global user layer.
    /// - All modes in the buffer-local overlay (if present).
    /// - All modes in every filetype-specific overlay.
    ///
    /// Returns the total count of removed entries.
    pub fn remove_mappings_by_owner(&mut self, owner: &MappingOwner) -> usize {
        let mut total = 0;

        // Global user layer — all modes.
        for trie in &mut self.user {
            total += trie.remove_by_owner(owner);
        }

        // Buffer-local overlay — all modes where a trie exists.
        for slot in &mut self.buf {
            if let Some(trie) = slot.as_mut() {
                total += trie.remove_by_owner(owner);
            }
        }

        // FileType-specific overlays — all filetypes, all modes.
        for ft_maps in self.filetype_maps.values_mut() {
            for trie in ft_maps.iter_mut() {
                total += trie.remove_by_owner(owner);
            }
        }

        total
    }

    // ═══════════════════════════════════════════════════════════════════
    // Buffer overlay swap (shell calls on buffer switch)
    // ═══════════════════════════════════════════════════════════════════

    /// Extract all buffer-local mappings, leaving the overlay empty.
    ///
    /// The shell should call this before switching away from a buffer,
    /// then store the returned `BufferMappings` keyed by buffer id.
    pub fn take_buffer_mappings(&mut self) -> BufferMappings {
        let mut tries: ModeMap<MappingTrie> = ModeMap::default();
        for mm in MappingMode::ALL {
            tries[mm] = self.buf[mm].take().unwrap_or_default();
        }
        BufferMappings { tries }
    }

    /// Install buffer-local mappings from a `BufferMappings`.
    ///
    /// The shell should call this after switching to a buffer.
    /// Any previous buffer-local overlay is replaced.
    pub fn set_buffer_mappings(&mut self, bm: BufferMappings) {
        let arr = bm.tries.into_array();
        for (slot, trie) in self.buf.iter_mut().zip(arr) {
            *slot = if trie.is_empty() { None } else { Some(trie) };
        }
    }
}

/// Build a `MappingEntry` from flags.
fn build_entry(
    to: Vec<KeyEvent>,
    kind: MappingKind,
    flags: MappingFlags,
    expr_text: Option<CompactString>,
) -> MappingEntry {
    let sequence = if flags.expr { Vec::new() } else { to };
    let expression = if flags.expr {
        Some(expr_text.unwrap_or_default())
    } else {
        None
    };
    MappingEntry::with_flags(sequence, kind, flags, expression)
}

/// Merge buffer-local and global `TrieLookup` results.
///
/// Implements `:help map-precedence` — buffer-local mappings are **always**
/// found before global ones. Quoting Vim:
///
/// > "Buffer-local mappings are used before global mappings."
/// > — `:help :map-local`, Vim 9.0
///
/// # Merge rules (exhaustive)
///
/// | Buffer result | Global result | Merged result | Rationale |
/// |--------------|---------------|---------------|-----------|
/// | ExactOnly(b) | *any*         | ExactOnly(b)  | Buffer exact wins unconditionally |
/// | Prefix(Some(b)) | *any*      | Prefix(Some(b)) | Buffer exact + buffer prefix; global irrelevant |
/// | Prefix(None) | ExactOnly(g)  | Prefix(None)  | Buffer prefix shadows global exact (:help map-precedence) |
/// | Prefix(None) | Prefix(g_ex)  | Prefix(g_ex)  | Both layers have prefixes; global exact bubbles up |
/// | Prefix(None) | NoMatch       | Prefix(None)  | Buffer prefix alone, no exact from either layer |
/// | NoMatch      | *any*         | global result | Buffer empty, delegate entirely |
const fn merge_lookups<'a>(buf: TrieLookup<'a>, global: TrieLookup<'a>) -> TrieLookup<'a> {
    match (buf, global) {
        // Buffer exact match — wins unconditionally per :help map-precedence.
        // Even if global has a longer prefix, the buffer-local fires immediately.
        (TrieLookup::ExactOnly(b), _) => TrieLookup::ExactOnly(b),

        // Buffer has prefix + exact: buffer exact takes priority, and the
        // buffer prefix means we're still waiting (but exact is known).
        (TrieLookup::Prefix { exact: Some(b), .. }, _) => TrieLookup::Prefix { exact: Some(b) },

        // Buffer has prefix but no exact: check if global can supply one.
        (TrieLookup::Prefix { exact: None, .. }, TrieLookup::ExactOnly(_)) => {
            TrieLookup::Prefix { exact: None }
        }
        (TrieLookup::Prefix { exact: None, .. }, TrieLookup::Prefix { exact: g_exact, .. }) => {
            TrieLookup::Prefix { exact: g_exact }
        }
        (TrieLookup::Prefix { exact: None, .. }, TrieLookup::NoMatch) => {
            TrieLookup::Prefix { exact: None }
        }

        // Buffer layer has nothing for this prefix — delegate entirely to global.
        (TrieLookup::NoMatch, g) => g,
    }
}

/// Create a key sequence from key events, truncating to [`MAX_KEY_SEQUENCE_LEN`].
#[must_use]
pub fn key_sequence(keys: &[KeyEvent]) -> Vec<KeyEvent> {
    let len = keys.len().min(MAX_KEY_SEQUENCE_LEN);
    match keys.get(..len) {
        Some(slice) => slice.to_vec(),
        None => keys.to_vec(),
    }
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
