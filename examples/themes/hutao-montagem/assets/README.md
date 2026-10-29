# Hu Tao After Dark

A decor pack for a dawn login greeter, styled as a late-night Liyue concert
poster: crimson lacquer, old gold marquee bulbs, plum shadow, a cheeky ghost
flame, and one butterfly for Papilio Charontis. The character cues are motifs
only. Every path in this pack was drawn from scratch. There is no official
HoYoverse artwork, logo, screenshot, font, or traced material here, and no asset
depicts game UI.

All nine SVGs use the same viewBox as their PNG. None of them contains a `text`
element, and the 77 marks are drawn as geometry. Every background is
transparent and no asset paints a full-canvas opaque rectangle. The sources use
gradients, layered opacity, halftone patterns, and plain geometry only, with no
filters that librsvg cannot render.

## Palette

| Role | Hex |
| --- | --- |
| ink | `#070408` |
| lacquer | `#16090d` |
| crimson | `#cf1230` |
| hot red | `#ff3b52` |
| bone | `#f3e7d2` |
| antique gold | `#d8a84b` |
| ember | `#ff7a3c` |
| ghost cyan, accent | `#b9ede6` |
| plum shadow, accent | `#4a1a33` |

The ghost cyan and plum are small supporting accents, used only for the spirit
and its shadow. The seven core colours carry the rest of the pack.

## Assets

- `poster.svg` / `poster.png` (840x1000): black lacquer panel with a crimson edge, double gold border, halftone corners, a faint 77 watermark, art deco corner cuts, and a bottom marquee rule.
- `sigil.svg` / `sigil.png` (512x512): gold bulb crest with a crimson centre, four butterfly wings and eyespots, and a ghost flame face.
- `divider.svg` / `divider.png` (620x28): fading crimson and gold rule with a small butterfly and an eighth note.
- `corner.svg` / `corner.png` (220x220): one gold and crimson art deco corner, made to be rotated into the other three.
- `ghost.svg` / `ghost.png` (240x280): pale cyan ghost flame with a plum shadow, tiny grin, flame crest, and ember tail.
- `note.svg` / `note.png` (110x150): bold bone and gold eighth note with an offset crimson plate.
- `ring.svg` / `ring.png` (320x320): thirty-two gold bulbs on a transparent ring, made for a slow spin.
- `ticket.svg` / `ticket.png` (280x130): bone concert stub with crimson print, a perforated tear line, a barcode, and a drawn 77.
- `spark.svg` / `spark.png` (64x64): four point ember and gold sparkle with a faint halo.

The poster keeps its middle dark and quiet on purpose, so the login form stays
readable when the panel sits behind it. The ring has a fully transparent centre
so it can turn around the form without boxing it in. The ticket, divider, and
corner are meant to be layered at the edges of the greeting.

## Rendering

The PNGs are rendered from the SVGs with librsvg. From this directory:

```sh
for f in poster sigil divider corner ghost note ring ticket spark; do
  rsvg-convert -o "$f.png" "$f.svg"
done
```

Or render one file:

```sh
rsvg-convert -o poster.png poster.svg
```

The PNGs are RGBA and keep the transparent page outside the artwork. They were
checked with rsvg-convert 2.63.4 and ImageMagick 7.1.2. Re-render after editing
an SVG so the PNG and the source stay in sync.
