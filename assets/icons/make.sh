#!/usr/bin/env bash
#
# Rasterises the menu's glyphs from Lucide (ISC, https://lucide.dev) into
# glyph/<name>.png next to this script, which src/art.rs embeds in the binary.
# Black on clear: macOS tints them for light and dark menus, Windows draws them
# in the menu's text colour.
#
# Needs npm (to fetch the pinned package) and either rsvg-convert (brew install
# librsvg) or Node, which then renders with resvg. The output is committed, so
# building Luz needs none of this.
#
#   assets/icons/make.sh
set -euo pipefail
cd "$(dirname "$0")"
here="$(pwd)"

LUCIDE=0.545.0   # same pin as Porta's and Cafe's, so the apps' menus match
RESVG=2.6.2
EDGE=32          # menus draw icons at 16 pt: 32 px is sharp on Retina and at 200%
PAD=2            # breathing room, so glyphs sit at the size of the OS's own icons

# Luz name = Lucide name.
GLYPHS=(
  sun sunset sunrise moon bed pause play flame snowflake sun-dim sun-medium
  rotate-ccw map-pin palette alarm-clock settings file-cog lock-open power
)

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
(cd "$work" && npm pack --silent "lucide-static@$LUCIDE" >/dev/null)
mkdir -p "$work/lucide"
tar xzf "$work/lucide-static-$LUCIDE.tgz" -C "$work/lucide"
icons="$work/lucide/package/icons"

rm -rf glyph
mkdir -p glyph
if command -v rsvg-convert >/dev/null; then
  inner=$((EDGE - 2 * PAD))
  for name in "${GLYPHS[@]}"; do
    rsvg-convert -w "$inner" -h "$inner" --keep-aspect-ratio \
      --page-width "$EDGE" --page-height "$EDGE" --left "$PAD" --top "$PAD" \
      -o "glyph/$name.png" "$icons/$name.svg"
  done
else
  # The same placement, by putting the 24-unit icon in a padded viewBox.
  (cd "$work" && npm install --silent --no-save "@resvg/resvg-js@$RESVG" >/dev/null)
  # Run from $work, so `require` finds the package installed there.
  (cd "$work" && ICONS="$icons" OUT="$here/glyph" EDGE="$EDGE" PAD="$PAD" NAMES="${GLYPHS[*]}" \
    node -e '
      const fs = require("fs");
      const { Resvg } = require("@resvg/resvg-js");
      const edge = +process.env.EDGE, pad = +process.env.PAD;
      const margin = 24 * pad / (edge - 2 * pad);
      for (const name of process.env.NAMES.split(" ")) {
        const svg = fs.readFileSync(`${process.env.ICONS}/${name}.svg`, "utf8")
          .replace(/viewBox="[^"]*"/, `viewBox="${-margin} ${-margin} ${24 + 2 * margin} ${24 + 2 * margin}"`);
        const png = new Resvg(svg, { fitTo: { mode: "width", value: edge } }).render().asPng();
        fs.writeFileSync(`${process.env.OUT}/${name}.png`, png);
      }')
fi

{
  echo "# Icon license"
  echo
  echo "The PNGs in \`glyph/\` are rasterised by \`make.sh\` from Lucide $LUCIDE."
  echo
  echo "## glyph/ — Lucide $LUCIDE"
  echo
  echo "https://github.com/lucide-icons/lucide"
  echo
  cat "$work/lucide/package/LICENSE"
} > LICENSES.md

echo "$(ls glyph | wc -l | tr -d ' ') glyphs"
