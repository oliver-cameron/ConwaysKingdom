#!/bin/sh
# Rebuild assets/fonts/Phosphor-subset.ttf from upstream Phosphor.
#
# **Here because a subset nobody can rebuild is a subset nobody can add to.**
# The font in `assets/` is twelve glyphs out of about twelve hundred -- four
# kilobytes against four hundred and eighty -- and the moment somebody wants a
# thirteenth icon they need this, or the honest answer is to ship the whole
# font and waste half a megabyte.
#
# The codepoints are the list in `client::views::glyph`, which is the one place
# that names them. Adding an icon is a line there and a line here, and the test
# beside it fails until this has been run.
#
#     sh tools/subset-icons.sh
#
# Needs fonttools: `pip install fonttools`, or a venv with it in.
set -eu

CODES=$(sed -n 's/.*= .\\u{\([0-9a-f]*\)}.*/U+\1/p' src/client/views/glyph.rs | paste -sd, -)
[ -n "$CODES" ] || { echo "no codepoints found in src/client/views/glyph.rs" >&2; exit 1; }
echo "subsetting to: $CODES"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
curl -sSL -o "$WORK/web.zip" https://github.com/phosphor-icons/web/archive/refs/heads/master.zip
unzip -q -o "$WORK/web.zip" -d "$WORK"

pyftsubset "$WORK/web-master/src/regular/Phosphor.ttf" \
    --unicodes="$CODES" \
    --output-file=assets/fonts/Phosphor-subset.ttf \
    --no-hinting --desubroutinize --drop-tables+=DSIG

cp "$WORK"/web-master/LICENSE* assets/fonts/PHOSPHOR-LICENSE.txt
ls -l assets/fonts/Phosphor-subset.ttf
