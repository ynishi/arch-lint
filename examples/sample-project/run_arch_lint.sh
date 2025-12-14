#!/bin/bash
# Verification script for arch-lint suppression patterns
#
# Expected Results:
#
# REPORTED (2 violations):
#   - lib.rs:14 unhandled_unwrap() - .unwrap() without suppression
#   - lib.rs:20 unhandled_expect() - .expect() without suppression
#
# NOT REPORTED (suppressed):
#   - lib.rs:30 comment_suppressed_unwrap() - comment-based suppression
#   - lib.rs:40-46 attribute_suppressed_unwrap() - #[arch_lint::allow]
#   - lib.rs:55-63 Config impl block - impl-level suppression
#   - lib.rs:71-77 suppressed_without_reason() - attribute suppression
#   - lib.rs:83-93 legacy module - module-level suppression
#   - startup.rs - function-level #[arch_lint::allow]
#   - tests - allow_in_tests = true

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "=== Building arch-lint CLI ==="
cargo build -p arch-lint-cli --manifest-path "$PROJECT_ROOT/Cargo.toml"

echo ""
echo "=== Building sample-project (to verify it compiles) ==="
cargo check --manifest-path "$SCRIPT_DIR/Cargo.toml"

echo ""
echo "=== Running arch-lint check ==="
echo ""

cd "$SCRIPT_DIR"
ARCH_LINT="$PROJECT_ROOT/target/debug/arch-lint"

# Run arch-lint (expect exit code 1 due to violations)
$ARCH_LINT check ./src || EXIT_CODE=$?

echo ""
echo "=== Verification Complete ==="
echo "Exit code: ${EXIT_CODE:-0}"
echo ""
echo "If you see exactly 2 errors above (lib.rs:14 and lib.rs:20),"
echo "then the suppression system is working correctly!"
