#!/bin/sh
# Dicturs installer: downloads the latest release for this machine (Linux, macOS).
#
#   curl -fsSL https://dicturs.app/install.sh | sh
#   curl -fsSL https://dicturs.app/install.sh | sh -s -- --bins cli,tui --dir ~/bin
#
# Options (or environment variables):
#   --bins LIST      binaries to install: cli,tui,gui (default: all)       DICTURS_BINS
#   --dir DIR        install directory (default: ~/.local/bin)              DICTURS_INSTALL_DIR
#   --version TAG    release tag such as v0.2.0 (default: latest)           DICTURS_VERSION
#   --uninstall      remove the installed binaries
#   DICTURS_BASE_URL overrides the download location (mirror).
set -eu

REPO="${DICTURS_REPO:-cyrill62/dicturs}"
BINS="${DICTURS_BINS:-cli,tui,gui}"
DIR="${DICTURS_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${DICTURS_VERSION:-latest}"
UNINSTALL=0

say() { printf '\033[1;36mdicturs\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror\033[0m %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --bins) BINS="${2:?--bins needs a value}"; shift 2 ;;
        --dir) DIR="${2:?--dir needs a value}"; shift 2 ;;
        --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
        --uninstall) UNINSTALL=1; shift ;;
        -h|--help)
            echo "usage: install.sh [--bins cli,tui,gui] [--dir DIR] [--version TAG] [--uninstall]"
            exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
done

# cli,tui,gui -> dicturs dicturs-tui dicturs-gui
names=""
for b in $(echo "$BINS" | tr ',' ' '); do
    case "$b" in
        cli) names="$names dicturs" ;;
        tui) names="$names dicturs-tui" ;;
        gui) names="$names dicturs-gui" ;;
        *) die "unknown binary '$b' (use cli, tui, gui)" ;;
    esac
done
[ -n "$names" ] || die "no binary selected"

if [ "$UNINSTALL" = 1 ]; then
    for n in $names; do rm -f "$DIR/$n" && say "removed $DIR/$n"; done
    exit 0
fi

case "$(uname -s)" in
    Linux) os=unknown-linux-gnu ;;
    Darwin) os=apple-darwin ;;
    *) die "unsupported system $(uname -s); on Windows download the .zip from https://github.com/$REPO/releases" ;;
esac
case "$(uname -m)" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) die "unsupported architecture $(uname -m)" ;;
esac
target="$arch-$os"
archive="dicturs-$target.tar.gz"
if [ -n "${DICTURS_BASE_URL:-}" ]; then
    base="$DICTURS_BASE_URL"
elif [ "$VERSION" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
else
    base="https://github.com/$REPO/releases/download/$VERSION"
fi

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
else
    die "curl or wget is required"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $archive ($VERSION)"
fetch "$base/$archive" "$tmp/$archive" || die "download failed: $base/$archive"
if fetch "$base/$archive.sha256" "$tmp/$archive.sha256" 2>/dev/null; then
    expected="$(cut -d' ' -f1 "$tmp/$archive.sha256")"
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$tmp/$archive" | cut -d' ' -f1)"
    else
        actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
    fi
    [ "$expected" = "$actual" ] || die "checksum mismatch for $archive"
    say "checksum ok"
fi

tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$DIR"
for n in $names; do
    src="$(find "$tmp" -type f -name "$n" | head -n 1)"
    [ -n "$src" ] || die "$n is missing from $archive"
    install -m 755 "$src" "$DIR/$n"
    say "installed $DIR/$n"
done

case ":$PATH:" in
    *":$DIR:"*) ;;
    *) say "add $DIR to your PATH (e.g. in ~/.profile, or \$env.PATH in nushell)" ;;
esac
say "next: dicturs model use base   # download a speech model, then run dicturs-tui or dicturs-gui"
