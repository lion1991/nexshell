#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
VERIFY_SOURCE="$PROJECT_DIR/scripts/verify-warp-compatibility.sh"

if [[ ! -x "$VERIFY_SOURCE" ]]; then
    printf 'missing executable compatibility verifier: %s\n' "$VERIFY_SOURCE" >&2
    exit 1
fi

grep -F 'verify-warp-compatibility.sh" --strict' "$PROJECT_DIR/scripts/build-dmg.sh" >/dev/null
grep -F 'verify-warp-compatibility.sh" --strict' "$PROJECT_DIR/scripts/validate.sh" >/dev/null

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

expect_pass() {
    if ! "$@"; then
        printf 'expected command to pass: %s\n' "$*" >&2
        exit 1
    fi
}

expect_fail() {
    if "$@"; then
        printf 'expected command to fail: %s\n' "$*" >&2
        exit 1
    fi
}

FIXTURE="$TMP_DIR/fixture"
mkdir -p "$FIXTURE/project/scripts" "$FIXTURE/warp"
cp "$VERIFY_SOURCE" "$FIXTURE/project/scripts/verify-warp-compatibility.sh"

git -C "$FIXTURE/warp" init -q
git -C "$FIXTURE/warp" config user.name "Compatibility Test"
git -C "$FIXTURE/warp" config user.email "compatibility@example.invalid"
printf 'official\n' > "$FIXTURE/warp/state.txt"
git -C "$FIXTURE/warp" add state.txt
git -C "$FIXTURE/warp" commit -q -m official
OFFICIAL_SHA="$(git -C "$FIXTURE/warp" rev-parse HEAD)"

printf 'integration\n' > "$FIXTURE/warp/state.txt"
git -C "$FIXTURE/warp" commit -q -am integration
INTEGRATION_SHA="$(git -C "$FIXTURE/warp" rev-parse HEAD)"
git -C "$FIXTURE/warp" remote add private git@github.com:lion1991/warp-nexshell.git

cat > "$FIXTURE/project/warp-compatibility.toml" <<EOF
schema_version = 1

[warp]
official_upstream = "$OFFICIAL_SHA"
integration_mirror = "$INTEGRATION_SHA"
relative_path = "../warp"
remote = "git@github.com:lion1991/warp-nexshell.git"
EOF

expect_pass "$FIXTURE/project/scripts/verify-warp-compatibility.sh" --strict

printf 'successor\n' > "$FIXTURE/warp/state.txt"
git -C "$FIXTURE/warp" commit -q -am successor
expect_pass "$FIXTURE/project/scripts/verify-warp-compatibility.sh"
expect_fail "$FIXTURE/project/scripts/verify-warp-compatibility.sh" --strict

git -C "$FIXTURE/warp" reset -q --hard "$INTEGRATION_SHA"
printf 'dirty\n' > "$FIXTURE/warp/untracked.txt"
expect_pass "$FIXTURE/project/scripts/verify-warp-compatibility.sh"
expect_fail "$FIXTURE/project/scripts/verify-warp-compatibility.sh" --strict
rm "$FIXTURE/warp/untracked.txt"

git -C "$FIXTURE/warp" remote set-url private git@github.com:example/wrong.git
expect_fail "$FIXTURE/project/scripts/verify-warp-compatibility.sh" --strict

ARCHIVE="$TMP_DIR/archive"
mkdir -p "$ARCHIVE/project/scripts" "$ARCHIVE/warp"
cp "$VERIFY_SOURCE" "$ARCHIVE/project/scripts/verify-warp-compatibility.sh"
cp "$FIXTURE/project/warp-compatibility.toml" "$ARCHIVE/project/warp-compatibility.toml"
expect_fail "$ARCHIVE/project/scripts/verify-warp-compatibility.sh"

printf 'warp compatibility verifier tests passed\n'
