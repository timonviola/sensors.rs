#!/bin/sh
# sensors.rs installer
#
# Note: the /HEAD/ path follows the repo's default branch, whatever it is named.
#   curl -fsSL https://raw.githubusercontent.com/timonviola/sensors.rs/HEAD/install.sh | sh
#
# Environment variables:
#   SENSORS_VERSION   version to install (default: latest release)
#   SENSORS_INSTALL   install directory (default: ~/.local/bin, or /usr/local/bin when root)
#   SENSORS_REPO      GitHub repo slug (default: timonviola/sensors.rs)
set -eu

REPO="${SENSORS_REPO:-timonviola/sensors.rs}"
BIN="sensors"

say() { printf '%s\n' "$*" >&2; }
err() { say "install.sh: error: $*"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || err "'$1' is required but not installed"; }

need uname
need tar
need mktemp

if command -v curl >/dev/null 2>&1; then
  DL="curl -fsSL"
  DL_OUT="curl -fsSL -o"
elif command -v wget >/dev/null 2>&1; then
  DL="wget -qO-"
  DL_OUT="wget -qO"
else
  err "neither curl nor wget found"
fi

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) err "unsupported architecture: $arch" ;;
  esac

  case "$os" in
    Darwin) echo "${arch}-apple-darwin" ;;
    Linux)
      # Prefer the statically linked musl build; it runs everywhere.
      echo "${arch}-unknown-linux-musl"
      ;;
    *) err "unsupported OS: $os (macOS and Linux only)" ;;
  esac
}

latest_version() {
  $DL "https://api.github.com/repos/${REPO}/releases/latest" |
    tr ',' '\n' |
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' |
    head -n 1
}

TARGET="$(detect_target)"
VERSION="${SENSORS_VERSION:-}"
VERSION="${VERSION#v}"
if [ -z "$VERSION" ]; then
  VERSION="$(latest_version)" || true
  [ -n "$VERSION" ] || err "could not determine the latest version; set SENSORS_VERSION"
fi

if [ -n "${SENSORS_INSTALL:-}" ]; then
  DEST="$SENSORS_INSTALL"
elif [ "$(id -u)" = "0" ]; then
  DEST="/usr/local/bin"
else
  DEST="$HOME/.local/bin"
fi

PKG="${BIN}-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/v${VERSION}/${PKG}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

say "Downloading ${BIN} ${VERSION} (${TARGET})"
$DL_OUT "$TMP/$PKG" "$URL" || err "download failed: $URL"

# Verify the checksum when a hashing tool is available.
if $DL "https://github.com/${REPO}/releases/download/v${VERSION}/SHA256SUMS" > "$TMP/SHA256SUMS" 2>/dev/null; then
  expected="$(awk -v f="$PKG" '$2 == f || $2 == "*"f { print $1 }' "$TMP/SHA256SUMS")"
  if [ -n "$expected" ]; then
    if command -v shasum >/dev/null 2>&1; then
      actual="$(shasum -a 256 "$TMP/$PKG" | cut -d' ' -f1)"
    elif command -v sha256sum >/dev/null 2>&1; then
      actual="$(sha256sum "$TMP/$PKG" | cut -d' ' -f1)"
    else
      actual=""
      say "warning: no sha256 tool found, skipping checksum verification"
    fi
    if [ -n "$actual" ] && [ "$actual" != "$expected" ]; then
      err "checksum mismatch for $PKG"
    fi
  fi
fi

tar -xzf "$TMP/$PKG" -C "$TMP"
SRC="$(find "$TMP" -type f -name "$BIN" -perm -u+x | head -n 1)"
[ -n "$SRC" ] || err "binary not found in archive"

mkdir -p "$DEST"
install -m 0755 "$SRC" "$DEST/$BIN" 2>/dev/null || {
  cp "$SRC" "$DEST/$BIN" && chmod 0755 "$DEST/$BIN"
}

say "Installed $DEST/$BIN"
"$DEST/$BIN" --version >&2 || true

case ":$PATH:" in
  *":$DEST:"*) ;;
  *)
    say ""
    say "Note: $DEST is not on your PATH. Add it, e.g.:"
    say "  echo 'export PATH=\"$DEST:\$PATH\"' >> ~/.zshrc"
    ;;
esac
