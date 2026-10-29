# Chibi Hu Tao system assets

These are the ornaments used by
`examples/themes/hutao-chibi/config.toml`. They are original vector drawings made for this example, not official Genshin or HoYoverse
artwork, so they can be copied and modified under the project license.

This theme is a cardless Wangsheng system terminal, not a character poster. The
wallpaper carries Hu Tao; the interface carries the login. The palette is ink,
vermilion, jade, and warm paper, pulled from the chibi art's black hat, red
flower, green hair, and beige background. Game details are small and
deliberate: a status dot, a login streak, a 77 seal, and a daily commission.

- `crest.png` is the Wangsheng system seal, a gate and flame inside a gold ring.
  It is the brand mark at the top left.
- `ring.png` is a thin HUD ring. It spins very slowly behind the heading and
  gives the right column depth without becoming a panel.
- `vline.png` is the tapered vertical rule that anchors the form column.
- `rule.png` is the small diamond rule used under the heading and above the
  footer.
- `streak.png` is the seven-day login streak row. The outlined last diamond is
  tomorrow's check-in.
- `dot.png` is the pulsing spirit-link status light.
- `seal.png` is the vermilion 77th Director seal.
- `ghost.png` is the small ghost flame near the footer. It is the one playful
  character motif, kept out of the form's way.
- `petal.png` is used twice, far from the form, for a little ambient drift.

The PNG files are the rendered assets that dawn loads. Re-render an edited SVG
with librsvg:

```sh
rsvg-convert -o crest.png crest.svg
```

The wallpaper (`examples/themes/hutao-chibi/wallpaper.mp4`) is example media, not
part of the drawn asset pack. Check its provenance before redistributing it.
