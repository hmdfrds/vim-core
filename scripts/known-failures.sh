#!/usr/bin/env bash
#
# The test suite of this repository does not pass, and is not meant to.
# See the "A note on failing tests" section of README.md.
#
# Deleting those tests would erase the only executable record of the defects
# they describe; marking them #[ignore] would turn a bug report into a green
# check mark. So instead they are *pinned*: the exact set of failing tests is
# recorded in tests-known-failing.txt, and this script asserts that the set
# has not changed.
#
# That keeps the useful half of a green build. A new failure is a regression
# and fails CI. A test that starts passing also fails CI -- loudly, because
# somebody fixed a bug and should be crossing it off the list.
#
#   ./scripts/known-failures.sh          check against the baseline (CI mode)
#   ./scripts/known-failures.sh --bless  rewrite the baseline from reality
#
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
BASELINE=tests-known-failing.txt

# Run each test binary separately rather than parsing one merged stream.
#
# The obvious implementation -- `cargo test --workspace 2>&1` and attribute
# each "---- name stdout ----" to the most recent "Running .../deps/foo-hash"
# line -- is wrong, and wrong in a way that only shows up in CI. Cargo writes
# "Running" to stderr and the failure blocks to stdout; merging them relies on
# the two streams interleaving in order, which holds on a terminal and does
# not hold on a non-TTY runner. There the target name comes out empty, every
# failure looks new, and every recorded failure looks fixed.
#
# Asking cargo for the binaries and running them one at a time makes the
# attribution structural instead of positional. --no-fail-fast is still
# load-bearing: without it cargo stops at the first failing target.
# A test binary that fails to build must fail this check: skipping it would
# hide every test in it.
build_log=$(mktemp)
trap 'rm -f "$build_log"' EXIT
if ! build_json=$(cargo test --workspace --no-run --message-format=json 2>"$build_log"); then
  cat "$build_log" >&2
  echo "the test build failed" >&2
  exit 2
fi
binaries=$(printf '%s\n' "$build_json" | python3 -c '
import json, sys
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m.get("profile", {}).get("test"):
        print(m["executable"])
' | sort -u)

if [ -z "$binaries" ]; then
  echo "could not enumerate test binaries" >&2
  exit 2
fi

# A binary that exits non-zero without reporting a failed test crashed (a
# stack overflow or an abort), and its failures were never printed.
crashed=()
results=$(mktemp)
trap 'rm -f "$build_log" "$results"' EXIT
for bin in $binaries; do
  # Strip the trailing -<hash> cargo appends to the binary name.
  target=$(basename "$bin" | sed -E 's/-[0-9a-f]{8,}$//')
  output=$("$bin" 2>/dev/null)
  status=$?
  names=$(printf '%s\n' "$output" | sed -n 's/^---- \(.*\) stdout ----$/\1/p')
  if [ "$status" -ne 0 ] && [ -z "$names" ]; then
    crashed+=("$target")
  fi
  [ -n "$names" ] && printf '%s\n' "$names" | while read -r name; do echo "$target::$name"; done >> "$results"
done
actual=$(sort -u "$results")

if [ "${#crashed[@]}" -gt 0 ]; then
  printf 'CRASHED   %s\n' "${crashed[@]}" >&2
  echo "A test binary stopped without reporting its results." >&2
  exit 1
fi

if [ "${1:-}" = "--bless" ]; then
  {
    echo "# Tests known to fail, one per line, as <target>::<test>."
    echo "# Regenerate with: ./scripts/known-failures.sh --bless"
    echo "#"
    echo "# These fail identically on the upstream v0.7.1 tag this repository is"
    echo "# cut from. They are pinned rather than silenced -- see README.md."
    printf '%s\n' "$actual"
  } > "$BASELINE"
  echo "blessed $(printf '%s\n' "$actual" | grep -c . ) known failures into $BASELINE"
  exit 0
fi

expected=$(grep -vE '^\s*(#|$)' "$BASELINE" | sort -u)

if [ "$actual" = "$expected" ]; then
  echo "OK: $(printf '%s\n' "$actual" | grep -c .) known failures, exactly as recorded."
  exit 0
fi

echo "The set of failing tests changed."
echo
comm -13 <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") | sed 's/^/  NEW FAILURE   /'
comm -23 <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") | sed 's/^/  NOW PASSING   /'
echo
echo "A NEW FAILURE is a regression -- fix it."
echo "A NOW PASSING test means a known defect got fixed. Confirm that is what"
echo "happened, then run ./scripts/known-failures.sh --bless and commit."
exit 1
