# Changelog

All notable changes to dawn are recorded here.

## 0.6.1 - 2026-10-30

### Fixed

- `-v` no longer fills the log with cosmic-text's "No default font match"
  debug lines. Dependency chatter stays at WARN while dawn's own debug records
  remain.

### Changed

- `[gpu] adapter` accepts `integrated`, `discrete`, and `software`. Those set
  the device class and power ranking, so most configs no longer need the
  separate `device` and `power` keys. The keys still work for filters and
  orderings the keywords cannot express.

## 0.6.0 - 2026-10-30

### Added

- `dawn use ID` switches the theme the next login starts. It rewrites only
  `[theme] package` in the machine config, so comments and every other key stay
  untouched, and it creates the config from the built-in defaults when none
  exists.
- `dawn list` now prints the active theme below the table.
- `dawn new NAME [--id ID] [--parent DIR]` takes a display name and derives the
  bundle id from it (`Crimson Stage` becomes `crimson-stage`). `--id` is
  optional and `--parent` defaults to the current directory.
- Commands that write `/etc` or `/usr/share` ask for sudo themselves when they
  have a terminal. `DAWN_NO_SUDO=1` disables that and prints the command
  instead.

### Changed

- `dawn init`, `dawn add`, `dawn remove`, and `dawn use` no longer need a
  manual `sudo` in an interactive shell.
- `dawn new` takes a name instead of a parent directory. The bundle is created
  at `DIR/ID`, and the printed next steps now end with `dawn use ID`.

## 0.5.0 - 2026-10-30

### Added

- DSB-2, the strict bundle format. `docs/bundle-spec.md` is the normative text.
  A bundle has four required artifacts (`theme.toml`, `config.toml`,
  `preview.webp`, `assets/`), a revision 2 manifest with an engine minimum and
  at least one author, and no license fields.
- `dawn check PATH` validates an uninstalled bundle tree and reports every
  problem as `rule <n>: <file>: <what>`.
- `dawn new PARENT --id ID` scaffolds a commented, valid bundle with a generated
  640x400 WebP preview.
- `dawn add PATH --migrate` upgrades a revision 1 tree to revision 2. It moves
  root-level media into `assets/`, patches matching `image` and `art` values,
  fills a placeholder author, and validates before replacing anything. Without
  `--force` the source tree is left untouched.
- `dawn dev [PATH] [--theme ID] [--size WxH] [--no-hud]` opens a windowed
  dry-run greeter that reloads a config or DSB bundle when it changes. It
  validates a bundle tree first, keeps the last good theme on a broken edit,
  and never opens a greetd socket or starts a session. `docs/dev.md` documents
  the keys, the HUD, and the reload guarantees.
- `dawn completions <SHELL>` prints a bash, elvish, fish, PowerShell, or zsh
  completion script to stdout. The shell also has a `-s` short form.
- Short flags for the remaining run options: `-c/--config`, `-t/--theme`,
  `-d/--dry-run`, `-y/--tty`, `-k/--check-config`, `-g/--list-gpus`,
  `-s/--screenshot`, `-z/--size`, and `-f/--fullscreen`.
- `dawn add`/`dawn remove` aliases: `install`/`i` and `uninstall`/`rm`, plus
  `ls` for `list`.
- `-F/--force` and `-u/--user` on theme commands.
- `dawn init` writes a commented first-run config to `/etc/dawn/config.toml`
  by default, with `--config PATH` for an explicit target. It refuses to
  overwrite without `--force`, stages the write and renames it into place, and
  prints the exact `sudo dawn init ...` rerun line when the destination is not
  writable.
- The README shows the Hu Tao Montagem theme in motion next to the other
  bundled themes.

### Changed

- Config discovery is now exactly three layers: `--config PATH`, `DAWN_CONFIG`,
  then `/etc/dawn/config.toml`. The `$XDG_CONFIG_HOME` and `~/.config` search
  paths are gone, because a greeter with no useful `HOME` never read them.
- With no config file, dawn prints exactly one line: `no machine config; create
  it: sudo dawn init`.
- `dawn init` writes only `/etc/dawn/config.toml` by default. `--config PATH`
  remains as the explicit testing seam. The `--user` flag is removed.
- The init permission hint is always the single line
  `rerun with sudo: sudo dawn init --force`.
- The system store `/usr/share/dawn/themes` and `DAWN_THEME_DIRS` entries are
  now checked before the XDG user store, no matter which copy is newer. A
  theme that exists only in the user store still loads, but dawn warns that it
  should be reinstalled into the system store.
- `dawn add` and `dawn remove` target the system store by default. `--user`
  opts back into `$XDG_DATA_HOME/dawn/themes` for packaging tests and sandboxes;
  `--system` is still accepted and is now the default. A non-root `dawn add`
  prints the exact `sudo dawn add ... --force` command to rerun instead of
  trying to escalate.
- A successful install prints one next-step line for selecting the theme.

### Fixed

- Sign-in no longer asks for Enter twice when the password was already typed
  before the first submit; PAM re-asks after a failure still wait for the user.

## 0.4.0 - 2026-10-30

### Added

- `[login.transition].trigger` accepts `boot` (one cut at the first presented
  frame) and `failure` (one cut when an auth attempt is rejected). `boot`
  never touches authentication, `failure` only paints after the form is back,
  and neither can hold the process open. `both` keeps its old enter-plus-success
  meaning, so existing configs do not start cutting at boot or on a wrong
  password.
- `[particles]` one-shot burst: `burst_on = "success"`, `burst_sprite`, and
  `burst_count` (capped at 64). The burst is a closed-form field of the time
  since success, so it spawns exactly once, stays inside the screen, and fades
  out over 1.8 seconds. The new `streak` sprite is a thin vertical light ray for
  both ambient and burst use.
- Per-keystroke pop on input fields: `key_pop_ms`, `key_pop_scale`, and
  `key_pop_color` draw the newest character with an expo-out overshoot and a
  colour blend back to the field text. The run is split with `char_indices`, so
  multibyte glyphs and the password mask are safe.
- `label_size` and `label_tracking` on fields, for a micro label that is
  independent of the large typed value.
- `[clock].label`, `label_color`, `label_size`, and `label_tracking` put a small
  tracked caps line above the digital clock.
- `[ticker].session_first` puts the selected session at the head of the marquee,
  uppercased for the poster voice, and flashes the run gold for 80 ms when the
  selection changes.

### Changed

- `[login] layout = "poster"`: a full-bleed fixed frame with no implied surface,
  for a cardless poster where decor and the form are placed with
  `form_offset`. It shares the hero geometry, so it still scales to fit a
  smaller output.
- The hutao-montagem example is rebuilt as AFTER DARK: THE STAGE CALL. The
  login has no card or plate: a poster headline, inline hairline fields, a
  support-act marquee, and a success cut with a butterfly burst. The old ticket
  stub and hero panel are gone.
- The live drag outline is only drawn for draggable regions, so a full-bleed
  cardless theme no longer shows a rectangle around the whole frame just
  because the pointer is inside it.

## 0.3.0 - 2026-10-30

### Added

- `[login.transition]`: an optional cut sequence played around a submit or a
  successful authentication. Full-screen colour cuts, a decaying shake and zoom
  punch, and one slammed line. Off by default, never part of the auth path, and
  the success hold is capped at two seconds.
- `[particles]`: procedural flame, butterfly, and dot emitters with a hard cap
  of 96, deterministic positions, and a behind or front layer. No image files.
- `[ticker]`: a kinetic marquee that scrolls one measured text run seamlessly
  at the top or bottom edge, with the same plate vocabulary as the clock.
- The hutao-montagem example is rebuilt around the montage: hard cuts, ember
  particles, a glass ticket marquee, a matching clock plate, and the crimson,
  black, bone, gold, and ember palette.

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
