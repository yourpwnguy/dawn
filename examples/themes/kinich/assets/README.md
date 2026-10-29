# Kinich Dawn Login Art Pack

A small decor set for a Genshin-inspired dawn login greeter. The mood is a calm
jungle hunter's quest screen: hi-vis lime light, charcoal gear, hot coral
accents, angular HUD geometry, and one deliberately 8-bit dragon companion.

This is original vector art. It contains no HoYoverse artwork, logos, sprites,
or traced material. The motifs are inspired by Kinich's palette and story only.

## Palette

- charcoal `#0a0f0c`
- panel green `#111a12`
- hi-vis lime `#b8f34a`
- jungle teal `#2ad3a8`
- hot coral `#ff6a3d`
- signal yellow `#f2e14c`
- bone white `#e9eadf`

## Assets

- `hud.svg` / `hud.png` - 800x940 tactical panel with stepped corners, faint grid, scanlines, coral bars, and a quiet dark centre for the login form.
- `sigil.svg` / `sigil.png` - 512x512 octagonal clan crest with an original pixel dragon head, lime ring, coral tabs, and teal inner line.
- `divider.svg` / `divider.png` - 620x24 pixel-stepped divider with a lime centre diamond, coral side diamonds, and small teal ticks.
- `corner.svg` / `corner.png` - 200x200 top-left lime bracket with a coral notch. Rotate it for the other three corners.
- `ajaw.svg` / `ajaw.png` - 180x180 original 8-bit dragon sprite with blocky wings, curled tail, horns, coral eyes, and teal highlights.
- `leaf.svg` / `leaf.png` - 80x80 pixel jungle leaf in lime and teal.
- `gauge.svg` / `gauge.png` - 360x28 unlabelled status frame with a dark track, lime fill, and stepped ends.
- `spark.svg` / `spark.png` - 64x64 8-bit plus spark in lime with a signal yellow core.

Every SVG has a transparent background and no text elements. The HUD keeps a
clean centre by design, so the form and its labels remain readable.

## Render

Run from this directory:

```sh
rsvg-convert -o name.png name.svg
```

For the whole pack:

```sh
for name in hud sigil divider corner ajaw leaf gauge spark; do
  rsvg-convert -o "$name.png" "$name.svg"
done
```

All assets are drawn with simple geometry and gradients that librsvg renders
without CSS filters or external fonts.
