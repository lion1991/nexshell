#!/usr/bin/env bash
set -euo pipefail

pattern='/Applications/NexShell\.app/Contents/MacOS/NexShell|/\.cache/cargo-target/debug/nexshell|/target/debug/nexshell'
duration="${NEXSHELL_TERMINAL_DECORATIONS_TRACE_SECONDS:-8s}"
template="${NEXSHELL_TERMINAL_DECORATIONS_TRACE_TEMPLATE:-Logging}"

process_exe() {
    ps -p "$1" -o args= 2>/dev/null | awk '{print $1}'
}

has_signpost_shim() {
    local exe="$1"
    [[ -x "$exe" ]] && nm -g "$exe" 2>/dev/null | rg -q 'warp_terminal_decorations_signpost_begin'
}

find_nexshell_pid() {
    local fallback=""
    local candidate
    local exe

    while IFS= read -r candidate; do
        [[ -n "$candidate" ]] || continue
        fallback="${fallback:-$candidate}"
        exe="$(process_exe "$candidate")"
        if has_signpost_shim "$exe"; then
            printf '%s\n' "$candidate"
            return
        fi
    done < <(pgrep -f "$pattern" || true)

    printf '%s\n' "$fallback"
}

if [[ "${1:-}" =~ ^[0-9]+$ ]]; then
    pid="$1"
    duration="${2:-$duration}"
else
    duration="${1:-$duration}"
    pid="$(find_nexshell_pid)"
fi

if [[ -z "${pid:-}" ]]; then
    echo "No running NexShell process found. Start the new build first." >&2
    exit 1
fi

exe="$(process_exe "$pid")"
if ! has_signpost_shim "$exe"; then
    echo "NexShell PID ${pid} does not contain the terminal decorations signpost shim." >&2
    echo "Binary: ${exe:-unknown}" >&2
    echo "Rebuild and restart NexShell, then run this script again." >&2
    exit 2
fi

timestamp="$(date +%Y%m%d-%H%M%S)"
base="${TMPDIR:-/tmp}/nexshell-terminal-decorations-signpost-${pid}-${timestamp}"
trace="${base}.trace"
toc="${base}-toc.xml"
export_dir="${base}-tables"
schema_file="${base}-schemas.txt"

mkdir -p "$export_dir"

echo "Recording ${template} trace for PID ${pid} (${duration})..."
echo "Trigger undercurl/dotted/dashed terminal output during this window."
xcrun xctrace record \
    --template "$template" \
    --attach "$pid" \
    --time-limit "$duration" \
    --output "$trace" \
    --quiet

echo "Exporting trace table of contents..."
xcrun xctrace export --input "$trace" --toc --output "$toc" --quiet

awk -F'schema="' '/<table/{split($2, a, "\""); print a[1]}' "$toc" \
    | sort -u \
    | grep -E '^os-signpost(-arg|-interval)?$' \
    > "$schema_file" || true

if [[ ! -s "$schema_file" ]]; then
    echo "No os-signpost xctrace schemas found in $toc" >&2
    exit 2
fi

echo "Exporting candidate tables..."
while IFS= read -r schema; do
    safe_schema="$(printf '%s' "$schema" | tr -c 'A-Za-z0-9_.-' '_')"
    xcrun xctrace export \
        --input "$trace" \
        --xpath "/trace-toc/run[@number='1']/data/table[@schema='$schema']" \
        --output "${export_dir}/${safe_schema}.xml" \
        --quiet >/dev/null 2>&1 || true
done < "$schema_file"

if rg -n --no-heading 'TerminalDecorations' "$toc" "$export_dir"; then
    echo "PASS: found TerminalDecorations os_signpost in exported xctrace data."
    echo "Trace: $trace"
    exit 0
fi

echo "FAIL: TerminalDecorations was not found in exported xctrace data." >&2
echo "Trace: $trace" >&2
echo "Exported tables: $export_dir" >&2
exit 3
