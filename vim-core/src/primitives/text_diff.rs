//! Minimal text diff utility based on common prefix/suffix matching.

/// Compute the minimal diff between two text strings.
///
/// Returns `(common_prefix_len, old_middle, new_middle, common_suffix_len)`.
///
/// The old and new middle slices represent the portion that actually changed.
/// If the strings are identical, both middle slices are empty.
#[must_use]
pub fn diff_texts<'a>(old: &'a str, new: &'a str) -> (usize, &'a str, &'a str, usize) {
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let old_rest = &old[prefix..];
    let new_rest = &new[prefix..];
    let suffix = old_rest
        .bytes()
        .rev()
        .zip(new_rest.bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = &old_rest[..old_rest.len() - suffix];
    let new_middle = &new_rest[..new_rest.len() - suffix];
    (prefix, old_middle, new_middle, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("hello", "hello");
        assert_eq!(prefix, 5);
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, "");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn both_empty() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("", "");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, "");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn old_empty() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("", "abc");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, "abc");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn new_empty() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("abc", "");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "abc");
        assert_eq!(new_mid, "");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn prefix_only_diff() {
        // Same suffix, different prefix
        let (prefix, old_mid, new_mid, suffix) = diff_texts("XXXhello", "YYhello");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "XXX");
        assert_eq!(new_mid, "YY");
        assert_eq!(suffix, 5); // "hello"
    }

    #[test]
    fn suffix_only_diff() {
        // Same prefix, different suffix
        let (prefix, old_mid, new_mid, suffix) = diff_texts("helloXXX", "helloYY");
        assert_eq!(prefix, 5); // "hello"
        assert_eq!(old_mid, "XXX");
        assert_eq!(new_mid, "YY");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn middle_diff() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("helloXXXworld", "helloYYworld");
        assert_eq!(prefix, 5); // "hello"
        assert_eq!(old_mid, "XXX");
        assert_eq!(new_mid, "YY");
        assert_eq!(suffix, 5); // "world"
    }

    #[test]
    fn completely_different() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("abc", "xyz");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "abc");
        assert_eq!(new_mid, "xyz");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn insertion_in_middle() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("helloworld", "hello world");
        assert_eq!(prefix, 5); // "hello"
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, " ");
        assert_eq!(suffix, 5); // "world"
    }

    #[test]
    fn deletion_in_middle() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("hello world", "helloworld");
        assert_eq!(prefix, 5); // "hello"
        assert_eq!(old_mid, " ");
        assert_eq!(new_mid, "");
        assert_eq!(suffix, 5); // "world"
    }

    #[test]
    fn single_char_change() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("cat", "car");
        assert_eq!(prefix, 2); // "ca"
        assert_eq!(old_mid, "t");
        assert_eq!(new_mid, "r");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn append_to_end() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("hello", "hello world");
        assert_eq!(prefix, 5); // "hello"
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, " world");
        assert_eq!(suffix, 0);
    }

    #[test]
    fn prepend_to_start() {
        let (prefix, old_mid, new_mid, suffix) = diff_texts("world", "hello world");
        assert_eq!(prefix, 0);
        assert_eq!(old_mid, "");
        assert_eq!(new_mid, "hello ");
        assert_eq!(suffix, 5); // "world"
    }
}
