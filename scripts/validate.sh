#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

"$SCRIPT_DIR/verify-warp-compatibility.sh" --strict || exit $?
source "$SCRIPT_DIR/cargo-env.sh"

STATUS=0
nexshell_run_cargo_clean test --bin nexshell --no-fail-fast \
    --manifest-path "$PROJECT_DIR/Cargo.toml" || STATUS=$?
nexshell_run_cargo_clean test --lib --no-fail-fast \
    --manifest-path "$PROJECT_DIR/Cargo.toml" || STATUS=$?
nexshell_run_cargo_clean check --all-targets \
    --manifest-path "$PROJECT_DIR/Cargo.toml" || STATUS=$?
nexshell_run_cargo_clean check --target x86_64-apple-darwin --all-targets \
    --manifest-path "$PROJECT_DIR/Cargo.toml" || STATUS=$?
nexshell_run_cargo_clean check --target x86_64-pc-windows-gnu --all-targets \
    --manifest-path "$PROJECT_DIR/Cargo.toml" || STATUS=$?
bash "$SCRIPT_DIR/tests/macos-bundle-metadata.sh" || STATUS=$?
bash "$SCRIPT_DIR/tests/warp-compatibility.sh" || STATUS=$?

exit "$STATUS"
