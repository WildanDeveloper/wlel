#!/bin/sh
# Render the package-manager manifests for a release.
#
#   scripts/packaging.sh                     # use the version in Cargo.toml,
#                                            # read hashes from dist/SHA256SUMS
#   scripts/packaging.sh 1.2.0 /path/to/SHA256SUMS
#
# Writes packaging/{homebrew/wlel.rb,scoop/wlel.json} with every archive's URL
# and SHA256. Run it after the release assets exist, commit the two files, then
# publish them to the tap/bucket with a `git push` (see packaging/README.md).
# Set WLEL_PACKAGING_OUT to write somewhere else.
set -eu

cd "$(dirname "$0")/.."

VERSION="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
SUMS="${2:-}"

if [ -z "$SUMS" ]; then
    if [ -f dist/SHA256SUMS ]; then
        SUMS=dist/SHA256SUMS
    else
        echo "packaging: no SHA256SUMS given and dist/SHA256SUMS is missing" >&2
        echo "packaging: download it from the release, or run scripts/release.sh first" >&2
        exit 1
    fi
fi

[ -f "$SUMS" ] || { echo "packaging: cannot read $SUMS" >&2; exit 1; }

hash_of() {
    # $1 = target triple
    awk -v name="wlel-${VERSION}-$1.tar.gz" '$2 == name { print $1 }' "$SUMS"
}

for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
              x86_64-apple-darwin aarch64-apple-darwin \
              x86_64-pc-windows-msvc; do
    if [ -z "$(hash_of "$target")" ]; then
        echo "packaging: $SUMS has no entry for $target" >&2
        exit 1
    fi
done

url() { printf 'https://github.com/WildanDeveloper/wlel/releases/download/v%s/wlel-%s-%s.tar.gz' "$VERSION" "$VERSION" "$1"; }

OUT="${WLEL_PACKAGING_OUT:-packaging}"
mkdir -p "$OUT/homebrew" "$OUT/scoop"
rm -f "$OUT/homebrew/wlel.rb" "$OUT/scoop/wlel.json"

# quoted heredoc so backticks and $version stay literal; the placeholders are
# substituted afterwards
cat > "$OUT/homebrew/wlel.rb" <<'EOF'
# typed: false
# frozen_string_literal: true

# Wlel is distributed as prebuilt archives. To publish, push this file to a
# tap repository (for example WildanDeveloper/homebrew-wlel) as Formula/wlel.rb;
# users then run `brew install WildanDeveloper/wlel/wlel`.
class Wlel < Formula
  desc "Arena-first systems programming language with a C backend"
  homepage "https://github.com/WildanDeveloper/wlel"
  version "@VERSION@"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "@AARCH64_DARWIN_URL@"
      sha256 "@AARCH64_DARWIN_SHA@"
    else
      url "@X86_64_DARWIN_URL@"
      sha256 "@X86_64_DARWIN_SHA@"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "@AARCH64_LINUX_URL@"
      sha256 "@AARCH64_LINUX_SHA@"
    else
      url "@X86_64_LINUX_URL@"
      sha256 "@X86_64_LINUX_SHA@"
    fi
  end

  def install
    bin.install Dir["*/wlel"].first
    doc.install "*/README.md"
  end

  test do
    assert_match "wlel #{version}", shell_output("#{bin}/wlel --version")
  end
end
EOF

replace() { sed -i.bak "s|@$1@|$2|g" "$OUT/homebrew/wlel.rb"; }
replace VERSION "$VERSION"
replace AARCH64_DARWIN_URL "$(url aarch64-apple-darwin)"
replace AARCH64_DARWIN_SHA "$(hash_of aarch64-apple-darwin)"
replace X86_64_DARWIN_URL "$(url x86_64-apple-darwin)"
replace X86_64_DARWIN_SHA "$(hash_of x86_64-apple-darwin)"
replace AARCH64_LINUX_URL "$(url aarch64-unknown-linux-gnu)"
replace AARCH64_LINUX_SHA "$(hash_of aarch64-unknown-linux-gnu)"
replace X86_64_LINUX_URL "$(url x86_64-unknown-linux-gnu)"
replace X86_64_LINUX_SHA "$(hash_of x86_64-unknown-linux-gnu)"
rm -f "$OUT/homebrew/wlel.rb.bak"

cat > "$OUT/scoop/wlel.json" <<'EOF'
{
  "version": "@VERSION@",
  "description": "Arena-first systems programming language with a C backend",
  "homepage": "https://github.com/WildanDeveloper/wlel",
  "license": "Apache-2.0",
  "architecture": {
    "64bit": {
      "url": "@WINDOWS_URL@",
      "hash": "@WINDOWS_SHA@",
      "extract_dir": "wlel-@VERSION@-x86_64-pc-windows-msvc"
    }
  },
  "bin": "wlel.exe",
  "checkver": {
    "github": "https://github.com/WildanDeveloper/wlel"
  },
  "autoupdate": {
    "architecture": {
      "64bit": {
        "url": "https://github.com/WildanDeveloper/wlel/releases/download/v$version/wlel-$version-x86_64-pc-windows-msvc.tar.gz",
        "extract_dir": "wlel-$version-x86_64-pc-windows-msvc"
      }
    }
  }
}
EOF

sed -i.bak \
    -e "s|@VERSION@|$VERSION|g" \
    -e "s|@WINDOWS_URL@|$(url x86_64-pc-windows-msvc)|g" \
    -e "s|@WINDOWS_SHA@|$(hash_of x86_64-pc-windows-msvc)|g" \
    "$OUT/scoop/wlel.json"
rm -f "$OUT/scoop/wlel.json.bak"

echo "packaging: wrote $OUT/homebrew/wlel.rb"
echo "packaging: wrote $OUT/scoop/wlel.json"
