#!/bin/sh
# Stage the Linguist pull-request payload in one directory.
#
#   ./editors/linguist/prepare.sh /tmp/linguist-wlel
#
# The result is what gets copied into a github/linguist checkout:
#   languages.yml  -> append to lib/linguist/languages.yml
#   grammars.yml   -> append under `grammars:`
#   samples/       -> samples/Wlel/
#
# Samples are the repository's own examples, so they stay in sync with the
# compiler instead of being copied into the tree and drifting.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
out=${1:-./linguist-wlel}

rm -rf "$out"
mkdir -p "$out/samples/Wlel"

cp "$repo/editors/linguist/languages.yml" "$out/languages.yml"
cp "$repo/editors/linguist/grammars.yml" "$out/grammars.yml"

for sample in kitchen enums loops strings generics collections; do
    cp "$repo/examples/$sample.wlel" "$out/samples/Wlel/"
done

echo "staged in $out:"
echo "  languages.yml  (append to lib/linguist/languages.yml)"
echo "  grammars.yml   (append under 'grammars:')"
echo "  samples/Wlel/  ($(ls "$out/samples/Wlel" | wc -l | tr -d ' ') files)"
