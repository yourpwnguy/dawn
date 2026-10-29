# Showcase wallpapers

These are the wallpapers used by the showcase configs in `examples/`.

- `aurora.svg` and `ember.svg` are the editable sources. The star field is
  deterministic, so re-rendering gives the same result.
- `aurora.webp` and `ember.webp` are the sizes dawn ships. They are 2560x1600,
  which gives `fit = "cover"` some room to crop on 16:9 and 16:10 screens.
- `output.gif` is the animated example used by `showcase-gif.toml`. Replace it
  with your own GIF and keep the same filename, or change `background.image`.
- `input.mp4` is the video example used by `showcase-video.toml`. It is
  1920x1080 H.264 at 25 fps and needs `ffmpeg` to play. Replace it with your own
  clip and keep the same filename, or change `background.image`.
- The theme packs use additional clips and stills. Those files are optional
  example media and are not part of the dawn artwork set. Check their
  provenance before redistributing them.

Re-render an edited SVG with ImageMagick:

```sh
magick aurora.svg -strip -quality 92 aurora.webp
```

The aurora and ember wallpapers are original artwork generated for these
examples, so they can be copied and modified with the rest of the project under
the same license.
