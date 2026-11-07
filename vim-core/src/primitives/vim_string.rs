//! The owned string type of vim-core's API.
//!
//! This is the only file that names `compact_str`. It stays a private
//! dependency: no public item here exposes its types or methods, so a new
//! `compact_str` major changes this file and nothing else.

use std::borrow::{Borrow, Cow};
use std::ffi::OsStr;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Deref, DerefMut, RangeBounds};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use compact_str::{CompactString, ToCompactString};

/// An owned UTF-8 string, the text type of vim-core's API.
///
/// It keeps text of up to 24 bytes (12 on 32-bit targets) inline, without a
/// heap allocation, and is the size of a `String`. It derefs to `str`, so
/// every `str` method works on it with `str`'s behaviour, and it converts
/// from and into the std string types.
///
/// Its storage is `compact_str`'s `CompactString`, a private dependency: no
/// public item exposes it or its methods, and there is no conversion from or
/// into it. A host with its own `CompactString` converts through `&str`.
///
/// ```
/// use vim_core::VimString;
///
/// let mut s = VimString::from("abc");
/// s.push_str("def");
/// assert_eq!(s, "abcdef");
/// assert_eq!(s.to_uppercase(), "ABCDEF");
/// let owned: String = s.into();
/// assert_eq!(owned, "abcdef");
/// ```
#[derive(Clone, Default)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct VimString(CompactString);

/// Builds a [`VimString`] with `format!` syntax.
///
/// ```
/// let s = vim_core::format_vim!("{}-{:>3}", "a", 7);
/// assert_eq!(s, "a-  7");
/// ```
#[macro_export]
macro_rules! format_vim {
    ($($arg:tt)*) => {
        $crate::VimString::from_fmt(::core::format_args!($($arg)*))
    };
}

impl VimString {
    /// Creates a `VimString` holding a copy of `text`.
    #[inline]
    #[must_use]
    pub fn new<T: AsRef<str>>(text: T) -> Self {
        Self(CompactString::new(text))
    }

    /// Creates a `VimString` from a static string without allocating.
    ///
    /// Usable in `const` items.
    #[inline]
    #[must_use]
    pub const fn from_static(text: &'static str) -> Self {
        Self(CompactString::const_new(text))
    }

    /// Creates an empty `VimString` with room for at least `capacity` bytes.
    ///
    /// # Panics
    ///
    /// Panics if the capacity exceeds `isize::MAX` bytes or the allocation
    /// fails.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self(CompactString::with_capacity(capacity))
    }

    /// Creates a `VimString` from format arguments. [`format_vim!`] calls
    /// this.
    ///
    /// # Panics
    ///
    /// Panics if a formatting trait implementation returns an error, as
    /// `format!` does.
    #[inline]
    #[must_use]
    pub fn from_fmt(args: fmt::Arguments<'_>) -> Self {
        Self(args.to_compact_string())
    }

    /// The text as a `&str`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The text as a `&mut str`.
    #[inline]
    #[must_use]
    pub fn as_mut_str(&mut self) -> &mut str {
        self.0.as_mut_str()
    }

    /// The length in bytes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string has no bytes.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of bytes it can hold without reallocating.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }

    /// Reserves room for at least `additional` more bytes.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize` or the allocation fails.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    /// Appends a character.
    #[inline]
    pub fn push(&mut self, ch: char) {
        self.0.push(ch);
    }

    /// Appends a string slice.
    #[inline]
    pub fn push_str(&mut self, text: &str) {
        self.0.push_str(text);
    }

    /// Removes and returns the last character, or `None` if it is empty.
    #[inline]
    pub fn pop(&mut self) -> Option<char> {
        self.0.pop()
    }

    /// Inserts a character at byte offset `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is past the end or not on a char boundary.
    #[inline]
    pub fn insert(&mut self, idx: usize, ch: char) {
        self.0.insert(idx, ch);
    }

    /// Inserts a string slice at byte offset `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is past the end or not on a char boundary.
    #[inline]
    pub fn insert_str(&mut self, idx: usize, text: &str) {
        self.0.insert_str(idx, text);
    }

    /// Removes and returns the character at byte offset `idx`.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is at or past the end or not on a char boundary.
    #[inline]
    pub fn remove(&mut self, idx: usize) -> char {
        self.0.remove(idx)
    }

    /// Shortens the string to `new_len` bytes. Does nothing if `new_len` is
    /// not less than the current length.
    ///
    /// # Panics
    ///
    /// Panics if `new_len` is not on a char boundary.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        self.0.truncate(new_len);
    }

    /// Removes all text, keeping the capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Splits the string at byte offset `at` and returns the tail.
    ///
    /// # Panics
    ///
    /// Panics if `at` is past the end or not on a char boundary.
    #[inline]
    #[must_use = "use `truncate` to drop the tail"]
    pub fn split_off(&mut self, at: usize) -> Self {
        Self(self.0.split_off(at))
    }

    /// Keeps only the characters for which `keep` returns `true`.
    #[inline]
    pub fn retain(&mut self, keep: impl FnMut(char) -> bool) {
        self.0.retain(keep);
    }

    /// Replaces the bytes in `range` with `replace_with`.
    ///
    /// # Panics
    ///
    /// Panics if either end of `range` is out of bounds or not on a char
    /// boundary.
    #[inline]
    pub fn replace_range(&mut self, range: impl RangeBounds<usize>, replace_with: &str) {
        self.0.replace_range(range, replace_with);
    }

    /// Converts into a `String`, reusing the heap buffer when there is one.
    #[inline]
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

impl Deref for VimString {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl DerefMut for VimString {
    #[inline]
    fn deref_mut(&mut self) -> &mut str {
        self.0.as_mut_str()
    }
}

impl AsRef<str> for VimString {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<[u8]> for VimString {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl AsRef<Path> for VimString {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self.0.as_str())
    }
}

impl AsRef<OsStr> for VimString {
    #[inline]
    fn as_ref(&self) -> &OsStr {
        OsStr::new(self.0.as_str())
    }
}

impl Borrow<str> for VimString {
    #[inline]
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

// ---------------------------------------------------------------------------
// Change
// ---------------------------------------------------------------------------

impl fmt::Write for VimString {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }

    #[inline]
    fn write_char(&mut self, c: char) -> fmt::Result {
        self.0.write_char(c)
    }

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        self.0.write_fmt(args)
    }
}

impl Add<&str> for VimString {
    type Output = Self;

    #[inline]
    fn add(mut self, rhs: &str) -> Self {
        self.0.push_str(rhs);
        self
    }
}

impl AddAssign<&str> for VimString {
    #[inline]
    fn add_assign(&mut self, rhs: &str) {
        self.0.push_str(rhs);
    }
}

/// `Extend` and `FromIterator` for the item types `CompactString` takes.
macro_rules! extend_and_collect {
    ($($item:ty $(, $lt:lifetime)?);* $(;)?) => {$(
        impl<$($lt)?> Extend<$item> for VimString {
            #[inline]
            fn extend<I: IntoIterator<Item = $item>>(&mut self, iter: I) {
                self.0.extend(iter);
            }
        }

        impl<$($lt)?> FromIterator<$item> for VimString {
            #[inline]
            fn from_iter<I: IntoIterator<Item = $item>>(iter: I) -> Self {
                Self(iter.into_iter().collect())
            }
        }
    )*};
}

extend_and_collect!(char; &'a char, 'a; &'a str, 'a; String; Cow<'a, str>, 'a; Box<str>);

impl Extend<Self> for VimString {
    #[inline]
    fn extend<I: IntoIterator<Item = Self>>(&mut self, iter: I) {
        for s in iter {
            self.0.push_str(&s);
        }
    }
}

impl FromIterator<Self> for VimString {
    #[inline]
    fn from_iter<I: IntoIterator<Item = Self>>(iter: I) -> Self {
        let mut out = Self::default();
        out.extend(iter);
        out
    }
}

impl Extend<VimString> for String {
    #[inline]
    fn extend<I: IntoIterator<Item = VimString>>(&mut self, iter: I) {
        for s in iter {
            self.push_str(&s);
        }
    }
}

impl FromIterator<VimString> for String {
    #[inline]
    fn from_iter<I: IntoIterator<Item = VimString>>(iter: I) -> Self {
        let mut out = Self::new();
        out.extend(iter);
        out
    }
}

// ---------------------------------------------------------------------------
// Convert in
// ---------------------------------------------------------------------------

impl From<&str> for VimString {
    #[inline]
    fn from(text: &str) -> Self {
        Self(CompactString::from(text))
    }
}

impl From<&mut str> for VimString {
    #[inline]
    fn from(text: &mut str) -> Self {
        Self(CompactString::from(&*text))
    }
}

impl From<String> for VimString {
    #[inline]
    fn from(text: String) -> Self {
        Self(CompactString::from(text))
    }
}

impl From<&String> for VimString {
    #[inline]
    fn from(text: &String) -> Self {
        Self(CompactString::from(text.as_str()))
    }
}

impl From<&Self> for VimString {
    #[inline]
    fn from(text: &Self) -> Self {
        text.clone()
    }
}

impl From<Box<str>> for VimString {
    #[inline]
    fn from(text: Box<str>) -> Self {
        Self(CompactString::from(text))
    }
}

impl From<Cow<'_, str>> for VimString {
    #[inline]
    fn from(text: Cow<'_, str>) -> Self {
        Self(CompactString::from(text))
    }
}

impl From<char> for VimString {
    #[inline]
    fn from(ch: char) -> Self {
        Self(CompactString::new(ch.encode_utf8(&mut [0; 4])))
    }
}

impl FromStr for VimString {
    type Err = std::convert::Infallible;

    #[inline]
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(text))
    }
}

// ---------------------------------------------------------------------------
// Convert out
// ---------------------------------------------------------------------------

impl From<VimString> for String {
    #[inline]
    fn from(text: VimString) -> Self {
        text.0.into_string()
    }
}

impl From<VimString> for Box<str> {
    #[inline]
    fn from(text: VimString) -> Self {
        text.0.into_string().into_boxed_str()
    }
}

impl From<VimString> for Arc<str> {
    #[inline]
    fn from(text: VimString) -> Self {
        Self::from(text.0.as_str())
    }
}

impl From<VimString> for Cow<'_, str> {
    #[inline]
    fn from(text: VimString) -> Self {
        Cow::Owned(text.0.into_string())
    }
}

impl<'a> From<&'a VimString> for Cow<'a, str> {
    #[inline]
    fn from(text: &'a VimString) -> Self {
        Cow::Borrowed(text.0.as_str())
    }
}

impl From<VimString> for PathBuf {
    #[inline]
    fn from(text: VimString) -> Self {
        Self::from(text.0.into_string())
    }
}

// ---------------------------------------------------------------------------
// Compare and hash, all as `str`
// ---------------------------------------------------------------------------

impl PartialEq for VimString {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for VimString {}

impl PartialOrd for VimString {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VimString {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_str().cmp(other.0.as_str())
    }
}

// `Borrow<str>` needs this to hash exactly as `str` does.
impl Hash for VimString {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

/// `PartialEq` both ways between `VimString` and each listed type.
macro_rules! eq_both_ways {
    ($($other:ty),* $(,)?) => {$(
        impl PartialEq<$other> for VimString {
            #[inline]
            fn eq(&self, other: &$other) -> bool {
                self.0.as_str() == &other[..]
            }
        }

        impl PartialEq<VimString> for $other {
            #[inline]
            fn eq(&self, other: &VimString) -> bool {
                &self[..] == other.0.as_str()
            }
        }
    )*};
}

eq_both_ways!(str, &str, String, &String, Cow<'_, str>);

// ---------------------------------------------------------------------------
// Format
// ---------------------------------------------------------------------------

impl fmt::Display for VimString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0.as_str(), f)
    }
}

impl fmt::Debug for VimString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0.as_str(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::VimString;
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::Arc;

    const SHORT: VimString = VimString::from_static("NORMAL");
    const LONG: VimString = VimString::from_static("a static text longer than 24 bytes");

    #[test]
    fn conversions_round_trip() {
        assert_eq!(VimString::from("abc"), "abc");
        assert_eq!(VimString::from(String::from("abc")), "abc");
        assert_eq!(VimString::from(&String::from("abc")), "abc");
        assert_eq!(VimString::from(Box::<str>::from("abc")), "abc");
        assert_eq!(VimString::from(Cow::Borrowed("abc")), "abc");
        assert_eq!(VimString::from(Cow::<str>::Owned("abc".into())), "abc");
        assert_eq!(VimString::from('é'), "é");
        assert_eq!(VimString::from(&VimString::from("abc")), "abc");
        assert_eq!("abc".parse::<VimString>(), Ok(VimString::from("abc")));

        let s = VimString::from("round trip");
        assert_eq!(String::from(s.clone()), "round trip");
        assert_eq!(s.clone().into_string(), "round trip");
        assert_eq!(&*Box::<str>::from(s.clone()), "round trip");
        assert_eq!(&*Arc::<str>::from(s.clone()), "round trip");
        assert_eq!(PathBuf::from(s.clone()), PathBuf::from("round trip"));
        assert!(matches!(Cow::from(&s), Cow::Borrowed("round trip")));
        assert_eq!(Cow::<str>::from(s), "round trip");
    }

    #[test]
    fn hash_matches_str() {
        let mut map: ahash::AHashMap<VimString, u8> = ahash::AHashMap::new();
        map.insert(VimString::from("key"), 1);
        map.insert(VimString::from("a key longer than 24 bytes"), 2);
        assert_eq!(map.get("key"), Some(&1));
        assert_eq!(map.get("a key longer than 24 bytes"), Some(&2));

        let build = ahash::RandomState::with_seeds(1, 2, 3, 4);
        assert_eq!(
            build.hash_one(VimString::from("abc")),
            build.hash_one("abc")
        );
    }

    #[test]
    fn debug_and_display_match_str() {
        let text = "tab\there \"quote\" \\ é \u{0}";
        let s = VimString::from(text);
        assert_eq!(format!("{s:?}"), format!("{text:?}"));
        assert_eq!(format!("{s}"), text);
        assert_eq!(format!("[{s:>8.3}]"), format!("[{:>8.3}]", text));
    }

    #[test]
    fn eq_both_ways_and_str_order() {
        let s = VimString::from("abc");
        let string = String::from("abc");
        let cow: Cow<'_, str> = Cow::Borrowed("abc");
        assert!(s == *"abc" && *"abc" == s);
        assert!(s == "abc" && "abc" == s);
        assert!(s == string && string == s);
        assert!(s == &string && &string == s);
        assert!(s == cow && cow == s);
        assert!(s != "abd" && "abd" != s);

        let mut words: Vec<VimString> = ["b", "a", "B", "ä", "aa"]
            .into_iter()
            .map(VimString::from)
            .collect();
        words.sort();
        assert_eq!(words, ["B", "a", "aa", "b", "ä"]);
    }

    #[test]
    fn from_static_in_const() {
        assert_eq!(SHORT, "NORMAL");
        assert_eq!(LONG, "a static text longer than 24 bytes");
        let mut grown = LONG;
        grown.push('!');
        assert_eq!(grown, "a static text longer than 24 bytes!");
    }

    #[test]
    fn format_vim_and_from_fmt() {
        assert_eq!(crate::format_vim!("{}-{}", 1, "two"), "1-two");
        assert_eq!(crate::format_vim!("[{:>5}]", "ab"), "[   ab]");
        assert_eq!(crate::format_vim!("{:.2}", 1.23456), "1.23");
        assert_eq!(crate::format_vim!("{:08.3}", -1.5), "-001.500");
        assert_eq!(crate::format_vim!("static"), "static");
        let long = "x".repeat(40);
        assert_eq!(crate::format_vim!("{long}"), long.as_str());
        assert_eq!(VimString::from_fmt(format_args!("{:<3}|", 'a')), "a  |");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_is_a_plain_string() {
        let s = VimString::from("a\"b");
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""a\"b""#);
        let back: VimString = serde_json::from_str(r#""a\"b""#).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn mutators_across_the_inline_limit() {
        let mut s = VimString::new("0123456789");
        s.push_str("0123456789");
        s.push_str("abcd");
        assert_eq!(s.len(), 24);
        s.push('é');
        assert_eq!(s, "01234567890123456789abcdé");
        assert_eq!(s.pop(), Some('é'));
        s.truncate(5);
        assert_eq!(s, "01234");
        s.truncate(50);
        assert_eq!(s, "01234");

        let mut s = VimString::from("a1b2c3d4e5f6g7h8i9j0k1l2m3");
        s.retain(|c| c.is_ascii_alphabetic());
        assert_eq!(s, "abcdefghijklm");

        let mut s = VimString::from("short");
        s.replace_range(1..4, "a text that makes it longer than 24 bytes");
        assert_eq!(s, "sa text that makes it longer than 24 bytest");
        s.replace_range(1.., "");
        assert_eq!(s, "s");

        let mut s = VimString::from("head and a tail longer than 24 bytes");
        let tail = s.split_off(4);
        assert_eq!(s, "head");
        assert_eq!(tail, " and a tail longer than 24 bytes");

        let mut s = VimString::with_capacity(40);
        assert!(s.capacity() >= 40 && s.is_empty());
        s.insert_str(0, "bd");
        s.insert(1, 'c');
        s.insert(0, 'a');
        assert_eq!(s.remove(3), 'd');
        s += "!";
        let mut s = s + "?";
        s.extend(['x', 'y']);
        s.extend(["z"]);
        s.extend([VimString::from("w")]);
        assert_eq!(s, "abc!?xyzw");
        s.as_mut_str().make_ascii_uppercase();
        assert_eq!(s, "ABC!?XYZW");
        s.clear();
        assert!(s.is_empty());

        let joined: VimString = ["a", "b"].into_iter().collect();
        let chars: VimString = ['c', 'd'].iter().collect();
        let strings: String = [joined.clone(), chars.clone()].into_iter().collect();
        assert_eq!(strings, "abcd");
        let parts: VimString = [joined, chars].into_iter().collect();
        assert_eq!(parts, "abcd");
    }

    #[test]
    #[should_panic(expected = "char boundary")]
    fn truncate_off_a_char_boundary_panics() {
        VimString::from("é").truncate(1);
    }
}
