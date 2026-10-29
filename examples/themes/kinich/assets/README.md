# Kinich: Contract Terminal Art Pack

A small decor pack for a Genshin-inspired dawn login theme. The mood is a
contract terminal on an acid-yellow field: ink chrome in the corners, a mission
bar along the bottom, a few levitating chips, and one deliberately 8-bit dragon
companion.

This is original vector art made for this theme. It contains no official
HoYoverse artwork, logos, character renders, or traced material. Everything here
is drawn from simple geometry, and the SVG sources keep no text elements. The
references are thematic only: the Scions of the Canopy, the Huitztlan jungle,
and Kinich's palette.

## Palette

| Role | Hex |
| --- | --- |
| Ink | `#17191C` |
| Muted ink | `#4E5148` |
| Acid yellow | `#E7DA19` |
| Citrus | `#C6D62B` |
| Coral | `#E2734F` |
| Sky | `#2E6FB2` |
| Bone | `#F7F3E9` |

## Assets

| File | Size | Description |
| --- | --- | --- |
| `metabar.svg` / `metabar.png` | 1920x48 | Opaque ink mission bar with a yellow top rule, ten tick notches, and a diamond tail on the right. It is meant to sit flush along the bottom edge. |
| `corner-bracket.svg` / `corner-bracket.png` | 224x224 | One ink viewport bracket, drawn for the top left and rotated for the other three corners. |
| `chip-ink.svg` / `chip-ink.png` | 96x96 | Outlined ink diamond that spins beside the wordmark. |
| `chip-coral.svg` / `chip-coral.png` | 96x96 | Filled coral bolt chip, the first of the floating echoes. |
| `chip-sky.svg` / `chip-sky.png` | 96x96 | Filled sky-blue bolt chip, the second floating echo. |
| `chip-citrus.svg` / `chip-citrus.png` | 96x96 | Filled citrus bolt chip, the third floating echo. |
| `rule-coral.svg` / `rule-coral.png` | 672x8 | Thin coral rule under the console heading. |
| `cursorbar.svg` / `cursorbar.png` | 40x104 | Solid ink block cursor for the end of the typing boot log. |
| `ajaw.svg` / `ajaw.png` | 180x180 | Original 8-bit dragon sprite with blocky wings, curled tail, horns, coral eyes, and teal highlights. Kept from the first version of the pack. |

The PNGs were rendered with librsvg 2.63.4. The bar and cursor are solid ink
plates with no transparency; the brackets, chips, and rule keep an alpha
channel.

## Render

From this directory:

```sh
rsvg-convert -o metabar.png metabar.svg
```

To rebuild the pack at the shipped scale:

```sh
rsvg-convert -w 1920 -h 48 -o metabar.png metabar.svg
rsvg-convert -w 224 -h 224 -o corner-bracket.png corner-bracket.svg
rsvg-convert -w 96 -h 96 -o chip-ink.png chip-ink.svg
rsvg-convert -w 96 -o chip-coral.png chip-coral.svg
rsvg-convert -w 96 -o chip-sky.png chip-sky.svg
rsvg-convert -w 96 -o chip-citrus.png chip-citrus.svg
rsvg-convert -w 672 -h 8 -o rule-coral.png rule-coral.svg
rsvg-convert -w 40 -o cursorbar.png cursorbar.svg
```

## Wallpaper

`wallpaper.mp4` and `preview.webp` are example media for this bundle. They are
not covered by the project licence; check their provenance before
redistributing them. Playing the video needs ffmpeg on `PATH`.
