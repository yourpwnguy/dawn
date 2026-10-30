# Changelog

All notable changes to dawn are recorded here.

## 0.2.6 - 2026-10-30

### Fixed

- Live glass blur stayed on the placeholder: an image background kept the same
  frame id before and after loading, so the GPU blur of the empty placeholder
  was never refreshed once the wallpaper arrived. Unloaded backgrounds now use
  a sentinel id, so the blur refreshes on the first frame after the wallpaper
  lands.

## 0.2.5 - 2026-10-30

### Added

- GPU-accelerated full-resolution backdrop blur. The live window runs a
  separable 33-tap Gaussian in two passes, reads the result back, and feeds the
  existing glass cache; the CPU pyramid remains the fallback and the
  `--screenshot` path.
- Glass surfaces beyond the login card: `style = "glass"` (alias `"blur"`) on
  every widget plate, and `glass = true` on `kind = "panel"` decor items.

### Changed

- The GPU blur now runs the entire colour pipeline in `blur.wgsl`: contrast and
  brightness prepare, HSL vibrancy, grain, and the final brightness. The CPU no
  longer pre-processes the upload or post-processes the readback, which cuts the
  per-refresh cost to the upload, two passes, and one readback.

## 0.2.4 - 2026-10-30

### Added

- First-party calendar, battery, and system widgets. `[calendar]` draws a month
  grid, `[battery]` reads the local power supply as a ring, bar, or pill, and
  `[system]` shows hostname, kernel, uptime, load, and memory from `/proc`. All
  three reuse the clock plate, motion, and state controls and are disabled by
  default.

## 0.2.3 - 2026-10-30

### Added

- Configurable CRT scanlines in `[theme]`: `scanlines`, `scanline_gap`, and
  `scanline_ms` add static rows and a moving sweep band over the wallpaper,
  under the login form.
- Clickable media transport controls. With `[media].controls = true`, the
  previous, play/pause, and next glyphs now send the matching MPRIS command
  when the widget is showing real metadata.

## 0.2.2 - 2026-10-30

### Added

- A decor motion pack: cursor parallax with a global `[motion].parallax`
  strength, one-shot `enter` choreography (`fade`, `slide`, `scale`), a
  per-character text `wave`, a `scramble_ms` text reveal, and a text `glitch`
  effect with coloured offset copies. All eleven per-item keys work on
  `[[login.decor]]`, and the built-in clock and media widget looks reuse the
  same motion table.

## 0.2.1 - 2026-10-30

### Added

- Theme bundles and a managed theme store. A bundle is a directory with
  `theme.toml`, `config.toml`, and its assets, validated as one portable unit.
- `dawn list`, `dawn add`, and `dawn remove`, with user and system stores.
- `--theme ID` and `[theme].package = "ID"` for selecting a bundle.
- `DAWN_THEME_DIRS` for extra read-only stores, including NixOS.
- A Nix flake and package expression.
- HUD text decor motions: a one-shot typewriter reveal, square-wave blink, and a
  seamless horizontal ticker with configurable width and gap.
- A `caret_style = "block"` option for input fields, alongside the existing bar
  caret.
- `layer = "behind" | "front"` on `[clock]` and `[media]`, so a built-in widget
  can paint inside a decor-built HUD bar without covering the form.
- Themed plates for the built-in clock and media widgets: `[clock.plate]` and
  `[media.plate]` support tint, solid, and gradient fills, borders, glow,
  shadow, padding, radius, and opacity. `[clock.motion]` and `[media.motion]`
  animate the whole widget with the decor motion table, and `show_on` /
  `hide_on` gate it on the login state.
- `layer = "behind" | "front"` on every plugin widget, so a manifest widget can
  join the front layer without ever covering the login form.
- A `kind = "panel"` decor item: an anchored solid or gradient plate with an
  optional border, opacity, and the usual motion table.
- Outlined text decor with `stroke` and `stroke_width`, drawn as an eight-copy
  ring behind the fill pass.
- A `[gpu]` table for backend, adapter, device class, power preference, present
  mode, and fallback policy, plus `--list-gpus` to inspect adapters and the
  selected one.
- Two clock faces: `flip` split-flap cards and `segment` seven-segment digits,
  with `ghost` unlit segments, a segment `glow`, a blinking colon, and a
  `tick` or `sweep` analog second hand.
- Three media layouts: `card`, `compact`, and `vinyl`, with cover or circular
  disc art, line/segments/wave progress, optional art-derived tinting, and
  visual-only transport glyphs.

### Security

- CI now runs `cargo deny check` against `deny.toml`, failing on advisories,
  yanked or unmaintained crates, wildcard requirements, non-crates.io sources,
  and disallowed licenses.
- Workflow actions are pinned to commit SHAs, the workflow token is read-only,
  and Dependabot keeps cargo dependencies and action pins current.

### Changed

- The kinich example theme is rebuilt as a full-bleed contract-terminal HUD.
- Installation is binary-only. The shell installers and the `make install`
  targets are gone; distro package steps are documented for Arch, Debian,
  Ubuntu, and Nix.
- The example themes moved to self-contained bundles under `examples/themes/`.
- README previews are animated clips of the Hu Tao, Genshin, and Kinich example
  themes, with full 60 fps MP4s under `docs/`.
- CLI parsing uses clap.

## 0.2.0 - 2026-10-29

### Added

- Glass design system with three presets and per-token overrides.
- Compact card and wide website-style hero layouts.
- Pointer submit button with gradients, hover wipes, glow, and sweep, sheen,
  pulse, and corner effects.
- Card-free `surface = "none"` layouts and per-element rotation.
- Card-relative decor for images and text, including animated decor with spin,
  sway, float, pulse, fade, and phase controls.
- `motion.decor_fps` to cap ambient decor redraws separately from input motion.
- Still PNG, JPEG, and WebP wallpapers plus animated GIF, WebP, and MP4 video.
- Per-output background overrides, fit modes, frame limits, memory caps, and
  idle pause.
- Built-in digital and analog clock widgets.
- Built-in MPRIS media widget with privacy controls and last-track retention.
- Declarative plugin API for text, clock, media, and PNG widgets.
- Drag and drop for the card, clock, media, and plugin widgets, with optional
  saved positions.
- GPU compositor using wgpu with a softbuffer fallback and `DAWN_GPU=0` escape
  hatch.
- CPU dual Kawase backdrop blur with Hyprland-compatible settings.
- Reduced motion, low power, and a terminal fallback.
- Six showcase configs with original decor art packs, covering cardless
  terminal layouts, banner-led hero panels, an opera playbill, a tactical HUD,
  and an after-dark poster.
- Line fields with `login.field.underline`, `underline_size`, and
  `label_color` for cardless forms.

### Changed

- The UI overlay is cached and uploaded only when it changes. Video frames cost
  one wallpaper upload and one full-screen triangle on the GPU path.
- The default card remains frosted glass, but `surface = "none"` is a supported
  first-class layout.
- Config validation reports exact dotted keys and source locations while
  falling back one field at a time.

### Fixed

- Still WebP files no longer take the animation path.
- Low-power mode cannot advance the wallpaper frame during clock repaints.
- MPRIS property caching no longer spams `NoActivePlayer`.
- A missing or broken optional subsystem degrades to a visible warning instead
  of blocking the form.

## 0.1.0

Initial greetd client, terminal fallback, compact card, config loading, session
discovery, and the first glass theme.
