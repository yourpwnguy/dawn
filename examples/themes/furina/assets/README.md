# Furina: Dawn Opera Art Pack

A small decor pack for a Genshin-inspired dawn login theme. The art uses the
theatre, the opera house, and the Hydro Archon as mood references: a playbill,
stage curtains, a spotlight, a crest, and a few baroque ornaments.

This is original vector art made for this theme. It contains no official
HoYoverse artwork, logos, character renders, or traced material. Everything here
is drawn from simple geometry and gradients, and every asset has a transparent
background with no text elements.

## Palette

| Role | Hex |
| --- | --- |
| Abyss navy | `#06122b` |
| Deep stage blue | `#0b2350` |
| Hydro | `#46c8ee` |
| Pale hydro | `#8fe6ff` |
| Antique gold | `#e8c987` |
| Porcelain | `#f7fbff` |
| Muted violet | `#6a5acd` |

## Assets

| File | Size | Description |
| --- | --- | --- |
| `playbill.svg` / `playbill.png` | 820x980 | Arched opera playbill panel with a double gold border, faint damask and ray texture, crown watermark, and a bottom fringe. The centre is kept deep and low-luminance for form legibility. |
| `curtain.svg` / `curtain.png` | 1600x150 | Three deep blue velvet swags with gold fringe, tieback tassels, a thin gold valance, and softly faded outer edges. The source keeps a 1600x300 coordinate space and stretches vertically with `preserveAspectRatio="none"`, which makes a shallow valance that does not cover the character. |
| `sigil.svg` / `sigil.png` | 512x512 | Circular Hydro crest with a beaded gold ring, stylised crown, central droplet, and wave arcs. No text. |
| `divider.svg` / `divider.png` | 620x28 | Thin gold flourish rule with a central hydro droplet and two small wave curls. |
| `corner.svg` / `corner.png` | 220x220 | One ornate gold baroque corner with acanthus leaves and a hydro droplet. Rotate it for the other three corners. |
| `crown.svg` / `crown.png` | 300x240 | The Hydro Archon crown: elegant gold points, blue gems, filigree band, and a central droplet pendant. No text. |
| `droplet.svg` / `droplet.png` | 100x140 | Luminous hydro droplet with a porcelain highlight, pale cyan rim, and a small sparkle. |
| `spotlight.svg` / `spotlight.png` | 800x1100 | Soft translucent stage light from the top centre, white to pale cyan, fading to zero at the bottom. The cone uses a Gaussian blur, so it has no hard edges. |
| `mask.svg` / `mask.png` | 200x200 | Small original comedy mask in porcelain white and gold, with a hydro tear and ribbon ties. |

## Render

The PNGs were rendered with librsvg 2.63.4. From this directory:

```sh
rsvg-convert -o playbill.png playbill.svg
```

To rebuild every PNG:

```sh
for f in playbill curtain sigil divider corner crown droplet spotlight mask; do
  rsvg-convert -o "$f.png" "$f.svg"
done
```

All nine PNGs are RGBA with an alpha channel and transparent or semi-transparent
edges. The SVG sources are each under 14 KB.
