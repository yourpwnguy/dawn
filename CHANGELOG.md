# Changelog

All notable changes to dawn are recorded here.

## Unreleased

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
- A `kind = "panel"` decor item: an anchored solid or gradient plate with an
  optional border, opacity, and the usual motion table.
- Outlined text decor with `stroke` and `stroke_width`, drawn as an eight-copy
  ring behind the fill pass.

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
