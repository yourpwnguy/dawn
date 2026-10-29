# Fischl dawn decor pack

An original vector decor set for the dawn login greeter, built around the
Fischl mood: a midnight hangings banner, a raven crest, a small crown, an
electro diamond, a crescent moon, and a few quiet sparkles. It is meant to
sit behind or around the login form, not to be a game asset.

All geometry was hand-authored for this pack. There is no official HoYoverse
artwork, logo, font, or traced material here, and no asset contains text.
The references are thematic only: night raven, gold royalty, violet electro,
Corvus-like stars, and a lonely theatrical royal fantasy.

## Palette

| Token | Hex | Use |
| --- | --- | --- |
| midnight | `#100b26` | banner cloth body, deep shadows |
| deep violet | `#2a1b58` | banner top, crest disc, feather roots |
| royal violet | `#6b4bd6` | glow, feather creases, crystal midtones |
| electro lilac | `#b48cff` | sparks, rim light, stars |
| antique gold | `#e6c56e` | rings, borders, crown, moon |
| moon white | `#f4efff` | star cores, moon highlights |
| raven black | `#07050f` | raven silhouette, deep outlines |

## Assets

Every SVG has a transparent background, uses only paths, gradients, opacity,
and clipping, and renders cleanly with `rsvg-convert`. The matching PNG is a
1:1 raster of the SVG viewBox with an alpha channel.

| Asset | Size | Role |
| --- | --- | --- |
| `banner.svg` / `banner.png` | 760x900 | Hanging midnight banner with a gold rod and finials, starfield, constellation, double gold border, swallowtail bottom, and small tassels. The centre is kept near black so form fields stay legible. |
| `sigil.svg` / `sigil.png` | 512x512 | Circular royal crest: gold double ring, violet electro diamond, stylized raven, and a small crown above. No text. |
| `divider.svg` / `divider.png` | 620x28 | Thin gold rule with a central violet electro diamond and tiny four-point stars. |
| `corner.svg` / `corner.png` | 220x220 | Ornate open corner bracket in gold with a violet crystal at the elbow. Drawn for the top left and rotated for the other three corners. |
| `raven.svg` / `raven.png` | 340x300 | Stylized spread-wing night raven with violet rim light, wedge tail, crest tufts, and a gold eye. |
| `crown.svg` / `crown.png` | 280x220 | Five-point royal crown in gold with violet gems and a small electro spark. No text. |
| `star.svg` / `star.png` | 72x72 | Crisp four-point sparkle in pale lilac and white with a soft aura. |
| `moon.svg` / `moon.png` | 240x240 | Pale gold crescent moon with a few small companion stars. The shipped config leaves it out of the banner so it cannot compete with the title; it is available for other layouts. |

## Render

From this directory:

```sh
cd examples/themes/fischl
rsvg-convert -o banner.png banner.svg
```

Or render the whole pack:

```sh
cd examples/themes/fischl
for f in banner sigil divider corner raven crown star moon; do
  rsvg-convert -o "$f.png" "$f.svg"
done
```

The checked-in PNGs were produced with rsvg-convert 2.63.4. If you need larger
raster copies, pass an explicit width or height, for example:

```sh
rsvg-convert -w 1520 -h 1800 -o banner@2x.png banner.svg
```

## Usage notes

- `corner.svg` is an optional top-left bracket. Rotate it by 90, 180, and 270
  degrees for the other corners; the crystal stays on the elbow. The shipped
  config leaves it out because the banner already has its own gold corners and
  the extra bracket would overlap the cloth inset.
- `moon.svg` is also optional. The shipped config keeps it out of the banner so
  it cannot compete with the title.
- The banner is deliberately dark in the middle. Place it behind the form
  fields, not over them, and keep any text on the banner off the gold border.
- The raven and sigil are heraldic silhouettes rather than anatomical
  illustrations, so they stay readable at small sizes.
- Every PNG has an alpha channel and was rendered at the exact viewBox size.
  No filters or text elements are used, so the assets stay portable.
