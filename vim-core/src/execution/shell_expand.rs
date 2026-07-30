//! Shell token expansion for ex commands.
//!
//! Expands `%` (current filename) and `#` (alternate filename) tokens in
//! shell command strings used by `:!cmd`, `:{range}!cmd`, and `:r !cmd`.

/// Expand shell tokens (`%` and `#`) in a command string.
///
/// Vim expands `%` to the current filename and `#` to the alternate filename
/// in `:!cmd`, `:{range}!cmd`, `:r !cmd`, and `:w !cmd` commands.
///
/// Rules:
/// - Unescaped `%` → `current_file` (if `Some`; left as `%` if `None`)
/// - Unescaped `#` → `alt_file` (if `Some`; left as `#` if `None`)
/// - `\%` → literal `%` (backslash consumed)
/// - `\#` → literal `#` (backslash consumed)
/// - `\\` → literal `\` (one backslash consumed)
/// - Other `\X` sequences are left as-is (both `\` and `X` preserved)
pub(crate) fn expand_shell_tokens(
    command: &str,
    current_file: Option<&str>,
    alt_file: Option<&str>,
) -> String {
    let mut result = String::with_capacity(command.len());
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                match chars.peek() {
                    Some('%') => {
                        chars.next();
                        result.push('%');
                    }
                    Some('#') => {
                        chars.next();
                        result.push('#');
                    }
                    Some('\\') => {
                        chars.next();
                        result.push('\\');
                    }
                    _ => {
                        // Not a recognized escape — preserve backslash and next char as-is
                        result.push('\\');
                    }
                }
            }
            '%' => {
                if let Some(file) = current_file {
                    result.push_str(file);
                } else {
                    result.push('%');
                }
            }
            '#' => {
                if let Some(file) = alt_file {
                    result.push_str(file);
                } else {
                    result.push('#');
                }
            }
            other => result.push(other),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_percent_with_current_file() {
        let result = expand_shell_tokens("gcc %", Some("main.c"), None);
        assert_eq!(result, "gcc main.c");
    }

    #[test]
    fn expand_hash_with_alt_file() {
        let result = expand_shell_tokens("diff % #", Some("a.txt"), Some("b.txt"));
        assert_eq!(result, "diff a.txt b.txt");
    }

    #[test]
    fn expand_percent_no_current_file_leaves_as_is() {
        let result = expand_shell_tokens("echo %", None, None);
        assert_eq!(result, "echo %");
    }

    #[test]
    fn expand_hash_no_alt_file_leaves_as_is() {
        let result = expand_shell_tokens("echo #", Some("a.txt"), None);
        assert_eq!(result, "echo #");
    }

    #[test]
    fn expand_escaped_percent() {
        let result = expand_shell_tokens(r"echo \%", Some("file.txt"), None);
        assert_eq!(result, "echo %");
    }

    #[test]
    fn expand_escaped_hash() {
        let result = expand_shell_tokens(r"echo \#", None, Some("alt.txt"));
        assert_eq!(result, "echo #");
    }

    #[test]
    fn expand_escaped_backslash() {
        let result = expand_shell_tokens(r"echo \\", Some("file.txt"), None);
        assert_eq!(result, r"echo \");
    }

    #[test]
    fn expand_escaped_backslash_before_percent() {
        // `\\%` → literal `\` followed by expansion of `%`
        let result = expand_shell_tokens(r"echo \\%", Some("file.txt"), None);
        assert_eq!(result, r"echo \file.txt");
    }

    #[test]
    fn expand_no_tokens() {
        let result = expand_shell_tokens("ls -la", Some("file.txt"), Some("alt.txt"));
        assert_eq!(result, "ls -la");
    }

    #[test]
    fn expand_consecutive_percent() {
        let result = expand_shell_tokens("%%", Some("f.c"), None);
        assert_eq!(result, "f.cf.c");
    }

    #[test]
    fn expand_backslash_at_end() {
        let result = expand_shell_tokens(r"echo \", None, None);
        assert_eq!(result, r"echo \");
    }

    #[test]
    fn expand_unrecognized_escape_preserved() {
        let result = expand_shell_tokens(r"echo \n", None, None);
        assert_eq!(result, r"echo \n");
    }

    #[test]
    fn expand_mixed_content() {
        let result = expand_shell_tokens(
            r"gcc -o output % && diff # \%",
            Some("main.c"),
            Some("old.c"),
        );
        assert_eq!(result, "gcc -o output main.c && diff old.c %");
    }

    #[test]
    fn expand_empty_command() {
        let result = expand_shell_tokens("", Some("file.txt"), Some("alt.txt"));
        assert_eq!(result, "");
    }

    #[test]
    fn expand_file_with_spaces() {
        let result = expand_shell_tokens("cat %", Some("my file.txt"), None);
        assert_eq!(result, "cat my file.txt");
    }

    #[test]
    fn expand_percent_at_end_of_string() {
        let result = expand_shell_tokens("cat %", Some("end.rs"), None);
        assert_eq!(result, "cat end.rs");
    }

    #[test]
    fn expand_hash_at_end_when_alt_none() {
        let result = expand_shell_tokens("echo #", Some("cur.rs"), None);
        assert_eq!(result, "echo #");
    }

    #[test]
    fn expand_percent_and_hash_mixed() {
        let result = expand_shell_tokens("diff % # > /tmp/out", Some("a.rs"), Some("b.rs"));
        assert_eq!(result, "diff a.rs b.rs > /tmp/out");
    }

    #[test]
    fn expand_only_percent_no_other_content() {
        let result = expand_shell_tokens("%", Some("solo.txt"), None);
        assert_eq!(result, "solo.txt");
    }

    #[test]
    fn expand_triple_backslash_before_percent() {
        // `\\\%` → `\\` is one escape (→ `\`), then `\%` is escaped percent (→ `%`)
        let result = expand_shell_tokens(r"\\\%", Some("file.txt"), None);
        assert_eq!(result, r"\%");
    }

    // --- Real-world use cases ---

    #[test]
    fn real_world_gcc_compile() {
        // `:!gcc % -o %:r` — note: modifiers like `:r` are NOT yet supported,
        // so `%:r` expands `%` then leaves `:r` as literal text.
        let result = expand_shell_tokens("gcc % -o %:r", Some("main.c"), None);
        // Without modifier support, `%` expands to `main.c` in both positions
        // and `:r` remains literal.
        assert_eq!(result, "gcc main.c -o main.c:r");
    }

    #[test]
    fn real_world_diff_current_and_alternate() {
        // `:!diff % #`
        let result = expand_shell_tokens("diff % #", Some("new.txt"), Some("old.txt"));
        assert_eq!(result, "diff new.txt old.txt");
    }

    #[test]
    fn real_world_escaped_env_var_percent() {
        // `:!echo \%HOME\%` — user wants literal `%HOME%` (Windows env var style)
        let result = expand_shell_tokens(r"echo \%HOME\%", Some("file.rs"), None);
        assert_eq!(result, "echo %HOME%");
    }

    #[test]
    fn real_world_grep_in_current_file() {
        // `:!grep -n TODO %`
        let result = expand_shell_tokens("grep -n TODO %", Some("src/lib.rs"), None);
        assert_eq!(result, "grep -n TODO src/lib.rs");
    }

    #[test]
    fn real_world_pipe_to_sort() {
        // `:!sort % | uniq` — sort current file and deduplicate
        let result = expand_shell_tokens("sort % | uniq", Some("data.csv"), None);
        assert_eq!(result, "sort data.csv | uniq");
    }

    #[test]
    fn real_world_no_file_set() {
        // `:!echo %` with no current file — `%` stays literal
        let result = expand_shell_tokens("echo %", None, None);
        assert_eq!(result, "echo %");
    }

    #[test]
    fn real_world_multiple_percent_in_pipeline() {
        // `:!wc -l % && echo % compiled`
        let result = expand_shell_tokens("wc -l % && echo % compiled", Some("main.rs"), None);
        assert_eq!(result, "wc -l main.rs && echo main.rs compiled");
    }

    // --- Stress tests ---

    #[test]
    fn stress_very_long_command_string() {
        // 10,000-char command with a few % tokens embedded
        let mut cmd: Vec<u8> = vec![b'a'; 10_000];
        cmd[500] = b'%';
        cmd[5000] = b'%';
        cmd[9500] = b'%';
        let cmd = String::from_utf8(cmd).unwrap();

        let result = expand_shell_tokens(&cmd, Some("F"), None);
        // Each `%` (1 char) replaced with "F" (1 char), length unchanged
        assert_eq!(result.len(), 10_000);
        assert!(!result.contains('%'));
        assert_eq!(result.matches('F').count(), 3);
    }

    #[test]
    fn stress_many_percent_occurrences() {
        // 5,000 consecutive `%` tokens
        let cmd = "%".repeat(5000);
        let result = expand_shell_tokens(&cmd, Some("x"), None);
        assert_eq!(result, "x".repeat(5000));
    }

    #[test]
    fn stress_many_hash_occurrences() {
        // 5,000 consecutive `#` tokens
        let cmd = "#".repeat(5000);
        let result = expand_shell_tokens(&cmd, None, Some("y"));
        assert_eq!(result, "y".repeat(5000));
    }

    #[test]
    fn stress_alternating_escaped_and_unescaped_percent() {
        // Pattern: `\% % \% % ...` — 2000 repetitions
        let mut cmd = String::with_capacity(8000);
        for _ in 0..2000 {
            cmd.push_str(r"\% % ");
        }
        let result = expand_shell_tokens(&cmd, Some("FILE"), None);
        // Each `\%` → literal `%`, each unescaped `%` → "FILE"
        let mut expected = String::with_capacity(20000);
        for _ in 0..2000 {
            expected.push_str("% FILE ");
        }
        assert_eq!(result, expected);
    }

    #[test]
    fn stress_alternating_escaped_and_unescaped_hash() {
        // Pattern: `\# # \# # ...` — 2000 repetitions
        let mut cmd = String::with_capacity(8000);
        for _ in 0..2000 {
            cmd.push_str(r"\# # ");
        }
        let result = expand_shell_tokens(&cmd, None, Some("ALT"));
        let mut expected = String::with_capacity(20000);
        for _ in 0..2000 {
            expected.push_str("# ALT ");
        }
        assert_eq!(result, expected);
    }

    #[test]
    fn stress_all_backslashes() {
        // 5000 backslashes — pairs of `\\` each collapse to single `\`
        let cmd = "\\".repeat(5000);
        let result = expand_shell_tokens(&cmd, Some("f"), Some("a"));
        // 5000 chars, consumed in pairs → 2500 literal backslashes
        assert_eq!(result, "\\".repeat(2500));
    }

    #[test]
    fn stress_interleaved_backslash_percent_hash() {
        // Pattern: `\\ \% \# % #` repeated 1000 times
        let mut cmd = String::with_capacity(12000);
        for _ in 0..1000 {
            cmd.push_str(r"\\ \% \# % # ");
        }
        let result = expand_shell_tokens(&cmd, Some("CUR"), Some("ALT"));
        let mut expected = String::with_capacity(20000);
        for _ in 0..1000 {
            // `\\` → `\`, ` `, `\%` → `%`, ` `, `\#` → `#`, ` `, `%` → CUR, ` `, `#` → ALT, ` `
            expected.push_str(r"\");
            expected.push_str(" % # CUR ALT ");
        }
        assert_eq!(result, expected);
    }

    #[test]
    fn stress_long_filenames() {
        // % expanded to a very long filename (10,000 chars)
        let long_name = "x".repeat(10_000);
        let cmd = "compile % done";
        let result = expand_shell_tokens(cmd, Some(&long_name), None);
        assert_eq!(result.len(), "compile ".len() + 10_000 + " done".len());
        assert!(result.starts_with("compile "));
        assert!(result.ends_with(" done"));
    }

    #[test]
    fn stress_is_single_pass_linear() {
        // Verify O(n) by running on a large input — if quadratic, this would timeout.
        // 100,000 logical units with alternating patterns.
        let mut cmd = String::with_capacity(120_000);
        for i in 0..20_000 {
            match i % 5 {
                0 => cmd.push('%'),
                1 => cmd.push('#'),
                2 => cmd.push_str(r"\%"),
                3 => cmd.push_str(r"\#"),
                _ => cmd.push('a'),
            }
        }
        let start = std::time::Instant::now();
        let result = expand_shell_tokens(&cmd, Some("cur.txt"), Some("alt.txt"));
        let elapsed = start.elapsed();
        // Should complete in well under 100ms on any machine
        assert!(elapsed.as_millis() < 100, "Took too long: {:?}", elapsed);
        // Sanity: result should be non-empty and contain expanded tokens
        assert!(!result.is_empty());
        assert!(result.contains("cur.txt"));
        assert!(result.contains("alt.txt"));
    }
}
