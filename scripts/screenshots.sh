#!/bin/sh
# Takes screenshots of dicturs-gui into docs/screenshots (used by the README and the website).
#
#   scripts/screenshots.sh [output-dir]
#
# Runs a debug build (the screenshot tour only exists there) on a throwaway demo config:
# your own commands, settings and models are never shown. Needs a graphical session;
# the window opens for a few seconds and closes by itself.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/docs/screenshots}"
demo="$(mktemp -d)"
trap 'rm -rf "$demo"' EXIT INT TERM

export DICTURS_CONFIG_DIR="$demo/config"
export DICTURS_MODELS_DIR="$demo/models"

cd "$root"
cargo build --quiet --bin dicturs --bin dicturs-gui
bin="$root/target/debug"

# Demo setup: built-ins on, a few samples on, English, two "downloaded" models.
"$bin/dicturs" init --lang en >/dev/null
"$bin/dicturs" settings set language en
"$bin/dicturs" settings set launcher fuzzel
enable_command() {
    # `dicturs list` columns: state, id, category, keyword, command (separated by 2+ spaces).
    id="$("$bin/dicturs" list | awk -F ' {2,}' -v k="$1" '$4 == k { print $2 }')"
    [ -n "$id" ] && "$bin/dicturs" enable "$id" >/dev/null
}
for keyword in "open terminal" "open browser" "search for" "volume up" "volume down" \
    "mute" "take screenshot" "lock screen" "play pause" "next track"; do
    enable_command "$keyword" || true
done
mkdir -p "$DICTURS_MODELS_DIR"
: > "$DICTURS_MODELS_DIR/ggml-base.bin"
: > "$DICTURS_MODELS_DIR/ggml-small.bin"

mkdir -p "$out"
DICTURS_SCREENSHOTS="$out" "$bin/dicturs-gui"
if command -v oxipng >/dev/null 2>&1; then
    oxipng --quiet --opt 3 --strip safe "$out"/*.png
fi
echo "screenshots in $out"
