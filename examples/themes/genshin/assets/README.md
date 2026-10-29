# Genshin decor assets

These are the ornaments used by
`examples/themes/genshin/config.toml`. They are original vector drawings made for this example, not official Genshin or HoYoverse
artwork, so they can be copied and modified under the project license.

- `sigil.svg` is the hydro drop inside a gold diamond, used as the brand mark.
- `divider.svg` is the fading gold rule under the greeting.
- `corner.svg` is one corner of the open frame. The config rotates the same
  image by 90, 180, and 270 degrees for the other three corners.
- `rune-line.svg` is the vertical diamond thread on the far right.
- `scrim.svg` is the soft navy mist that sits behind the form. It has no hard
  edges, so it reads as atmosphere instead of a card.

The PNG files are the rendered assets that dawn loads. Re-render an edited SVG
with librsvg:

```sh
rsvg-convert -o sigil.png sigil.svg
```

The wallpaper (`examples/themes/genshin/wallpaper.mp4`) is example media, not part
of the drawn asset pack. Check its provenance before redistributing it.
