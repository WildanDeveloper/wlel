#!/bin/sh
# Install the Wlel compiler.
#
#   curl -fsSL https://raw.githubusercontent.com/WildanDeveloper/wlel/main/install.sh | sh
#
# Options (environment variables, so `curl | sh` works):
#   WLEL_VERSION=v1.2.0        install a specific release (default: latest)
#   WLEL_INSTALL_DIR=DIR       where to put the binary
#                              (default: /usr/local/bin if writable, else ~/.local/bin)
#   WLEL_NO_VERIFY=1           skip the SHA256 check (not recommended)
#
# The script never runs sudo: if the target directory is not writable it
# installs under ~/.local/bin and tells you to add that to PATH.
set -eu

REPO="WildanDeveloper/wlel"
BIN="wlel"

say() { printf '%s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

# ---- platform -------------------------------------------------------------

os=$(uname -s 2>/dev/null || echo unknown)
arch=$(uname -m 2>/dev/null || echo unknown)

case "$os" in
    Linux) os=unknown-linux-gnu ;;
    Darwin) os=apple-darwin ;;
    *) die "unsupported operating system: $os (prebuilt binaries exist for Linux and macOS; build from source instead)" ;;
esac

case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture: $arch" ;;
esac

target="${arch}-${os}"

# ---- tooling --------------------------------------------------------------

for tool in curl tar; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done

have_sha256=0
if command -v sha256sum >/dev/null 2>&1; then
    have_sha256=1
elif command -v shasum >/dev/null 2>&1; then
    have_sha256=2
fi

# ---- release --------------------------------------------------------------

version=${WLEL_VERSION:-}
if [ -z "$version" ]; then
    # trust the redirect instead of the API: /releases/latest answers with
    # the tag URL, costs no rate limit, and works behind a proxy
    latest=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)
    case "$latest" in
        */tag/*) version=${latest##*/} ;;
        *) die "cannot determine the latest release; set WLEL_VERSION (for example WLEL_VERSION=v1.2.0)" ;;
    esac
fi

case "$version" in
    v*) ;;
    *) version="v${version}" ;;
esac
plain=${version#v}

archive="${BIN}-${plain}-${target}.tar.gz"
base="https://github.com/${REPO}/releases/download/${version}"

# ---- destination ----------------------------------------------------------

dest=${WLEL_INSTALL_DIR:-}
if [ -z "$dest" ]; then
    if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
        dest=/usr/local/bin
    elif [ "$(id -u 2>/dev/null || echo 1)" = 0 ] && [ -d /usr/local/bin ]; then
        dest=/usr/local/bin
    else
        dest=${HOME:-}/.local/bin
    fi
fi

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t wlel)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "install: downloading wlel ${plain} for ${target}"
curl -fsSL --retry 3 -o "$tmp/$archive" "$base/$archive" \
    || die "download failed: $base/$archive"

# ---- checksum -------------------------------------------------------------

if [ "$have_sha256" -ne 0 ] && [ "${WLEL_NO_VERIFY:-0}" != 1 ]; then
    curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" \
        || die "cannot download SHA256SUMS"
    expected=$(awk -v name="$archive" '$2 == name { print $1 }' "$tmp/SHA256SUMS")
    [ -n "$expected" ] || die "$archive is not listed in SHA256SUMS"
    if [ "$have_sha256" -eq 1 ]; then
        actual=$(sha256sum "$tmp/$archive" | awk '{ print $1 }')
    else
        actual=$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')
    fi
    [ "$expected" = "$actual" ] || die "checksum mismatch for $archive (expected $expected, got $actual)"
    say "install: checksum ok"
else
    say "install: WARNING checksum not verified (WLEL_NO_VERIFY=1 or no sha256 tool)"
fi

tar -xzf "$tmp/$archive" -C "$tmp" || die "cannot unpack $archive"
src="$tmp/${BIN}-${plain}-${target}/${BIN}"
[ -f "$src" ] || die "the archive did not contain ${BIN}"

# ---- install --------------------------------------------------------------

mkdir -p "$dest" 2>/dev/null || die "cannot create $dest"
if [ -w "$dest" ]; then
    install -m 0755 "$src" "$dest/$BIN" 2>/dev/null || cp "$src" "$dest/$BIN"
else
    die "$dest is not writable; set WLEL_INSTALL_DIR to a writable directory"
fi

say "install: wlel ${plain} -> ${dest}/${BIN}"

case ":${PATH:-}:" in
    *":${dest}:"*) ;;
    *) say "install: ${dest} is not on PATH; add it with:"
       say "         export PATH=\"${dest}:\$PATH\"" ;;
esac

"$dest/$BIN" --version || die "the installed binary does not run"
say "install: done. Run 'wlel --help'."
