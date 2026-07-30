// Regression tests — each test documents and prevents recurrence of a specific bug.
//
// ## Naming Convention
//
//   neovim_test!(regression, YYYY_MM_DD_description, "initial text", (line, col), "keys")
//
// Each test MUST have a one-line comment above it citing what was fixed.
// Date-based naming enables `git log --since` traceability.
