#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST="$PROJECT_DIR/warp-compatibility.toml"
STRICT=false

if [[ $# -gt 1 ]] || [[ $# -eq 1 && "$1" != "--strict" ]]; then
    printf 'usage: %s [--strict]\n' "$0" >&2
    exit 2
fi
if [[ ${1:-} == "--strict" ]]; then
    STRICT=true
fi

fail() {
    printf 'warp compatibility error: %s\n' "$*" >&2
    exit 1
}

read_value() {
    local key="$1"
    awk -F '=' -v key="$key" '
        $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
            value = substr($0, index($0, "=") + 1)
            sub(/^[[:space:]]*/, "", value)
            sub(/[[:space:]]*$/, "", value)
            if (value ~ /^".*"$/) {
                value = substr(value, 2, length(value) - 2)
            }
            print value
            exit
        }
    ' "$MANIFEST"
}

[[ -f "$MANIFEST" ]] || fail "missing manifest $MANIFEST"

SCHEMA_VERSION="$(read_value schema_version)"
OFFICIAL_UPSTREAM="$(read_value official_upstream)"
INTEGRATION_MIRROR="$(read_value integration_mirror)"
RELATIVE_PATH="$(read_value relative_path)"
EXPECTED_REMOTE="$(read_value remote)"

[[ "$SCHEMA_VERSION" == "1" ]] || fail "unsupported schema_version: ${SCHEMA_VERSION:-missing}"
[[ "$OFFICIAL_UPSTREAM" =~ ^[0-9a-f]{40}$ ]] || fail "official_upstream must be a full SHA"
[[ "$INTEGRATION_MIRROR" =~ ^[0-9a-f]{40}$ ]] || fail "integration_mirror must be a full SHA"
[[ -n "$RELATIVE_PATH" ]] || fail "relative_path is missing"
[[ -n "$EXPECTED_REMOTE" ]] || fail "remote is missing"

WARP_DIR="$(cd "$PROJECT_DIR/$RELATIVE_PATH" 2>/dev/null && pwd)" \
    || fail "Warp path is unavailable: $PROJECT_DIR/$RELATIVE_PATH"
git -C "$WARP_DIR" rev-parse --git-dir >/dev/null 2>&1 \
    || fail "Warp path has no Git metadata: $WARP_DIR"

HEAD_SHA="$(git -C "$WARP_DIR" rev-parse HEAD)" \
    || fail "cannot resolve Warp HEAD"
git -C "$WARP_DIR" cat-file -e "$OFFICIAL_UPSTREAM^{commit}" 2>/dev/null \
    || fail "official_upstream commit is unavailable: $OFFICIAL_UPSTREAM"
git -C "$WARP_DIR" cat-file -e "$INTEGRATION_MIRROR^{commit}" 2>/dev/null \
    || fail "integration_mirror commit is unavailable: $INTEGRATION_MIRROR"
git -C "$WARP_DIR" merge-base --is-ancestor "$OFFICIAL_UPSTREAM" "$HEAD_SHA" \
    || fail "official_upstream is not an ancestor of Warp HEAD"
git -C "$WARP_DIR" merge-base --is-ancestor "$INTEGRATION_MIRROR" "$HEAD_SHA" \
    || fail "integration_mirror is not an ancestor of Warp HEAD"

DIRTY_STATUS="$(git -C "$WARP_DIR" status --porcelain --untracked-files=normal)"
if [[ "$STRICT" == true ]]; then
    [[ "$HEAD_SHA" == "$INTEGRATION_MIRROR" ]] \
        || fail "strict mode requires Warp HEAD $INTEGRATION_MIRROR, got $HEAD_SHA"
    [[ -z "$DIRTY_STATUS" ]] || fail "strict mode requires a clean Warp worktree"

    REMOTE_MATCH=false
    while IFS= read -r configured_remote; do
        if [[ "$configured_remote" == "$EXPECTED_REMOTE" ]]; then
            REMOTE_MATCH=true
            break
        fi
    done < <(git -C "$WARP_DIR" config --get-regexp '^remote\..*\.url$' | awk '{print $2}')
    [[ "$REMOTE_MATCH" == true ]] \
        || fail "strict mode cannot find expected Warp remote: $EXPECTED_REMOTE"
elif [[ -n "$DIRTY_STATUS" ]]; then
    printf 'warp compatibility warning: worktree is dirty; development ancestry checks still passed\n' >&2
fi

MODE="development"
if [[ "$STRICT" == true ]]; then
    MODE="strict"
fi
printf 'warp compatibility ok: mode=%s head=%s\n' "$MODE" "$HEAD_SHA"
