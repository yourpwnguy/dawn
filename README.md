# dawn

[![CI](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml/badge.svg)](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-red)](#security)
[![greetd](https://img.shields.io/badge/greetd-native-orange)](https://git.sr.ht/~kennylevinsen/greetd)
[![spec](https://img.shields.io/badge/bundle%20spec-DSB--2.0-informational)](#the-bundle-specification)

| Hu Tao | Genshin | Kinich | Hu Tao Montagem |
| --- | --- | --- | --- |
| [![Hu Tao theme in motion](docs/dawn-hutao.webp)](docs/dawn-hutao.mp4) | [![Genshin theme in motion](docs/dawn-genshin.webp)](docs/dawn-genshin.mp4) | [![Kinich theme in motion](docs/dawn-kinich.webp)](docs/dawn-kinich.mp4) | [![Hu Tao Montagem in motion](docs/dawn-hutao-montagem.webp)](docs/dawn-hutao-montagem.mp4) |

Four bundled themes in motion. Click a preview to open the clip.

## Overview

dawn is a graphical login greeter for
[greetd](https://git.sr.ht/~kennylevinsen/greetd). It renders the sign-in
session, runs the PAM conversation through greetd, and hands the user to their
chosen desktop session. Anything else it does, it does without ever putting the
login at risk.

The project has a specific opinion about what happened to the Linux login
screen: it either became a bare terminal prompt, or it became a widget
environment managed by a display manager with years of accumulated
configuration surface. dawn takes the third position. The login screen is a
fully rendered scene with a defined spec behind it. The scene is programmable
through data, the data is versioned, the manifest carries authorship and
credits, and the same binary that renders the scene is also the tool that
installs, validates, and replaces it. There are no installer scripts, no
hand-copied asset trees, and no second place where the truth about a theme
lives. dawn is the single source of truth for the whole system: one binary
specifies the bundle format, validates it against the spec, installs it
atomically, resolves it at boot, and renders it.

The renderer is self-contained. Frames are laid out with tiny-skia and
presented through wgpu on Vulkan or OpenGL, or softbuffer when no GPU is
available. If the graphical frontend cannot start at all, dawn degrades to a
terminal authentication prompt, so the machine remains reachable by definition.
That property is not an emergency measure bolted on after the fact; it is the
first line of the design.

## Design principles

1. **The login always completes.** Credentials live in zeroizing buffers and
   pass from the input field to the auth worker and to greetd. Every failure
   mode has a defined fallback, and every fallback is less pretty and more
   reliable than the layer above it.
2. **Configuration is data.** Layouts, palettes, motion, widgets, and surfaces
   are TOML tables read at boot. Bad values fall back per field with a spoken
   warning on screen; the rest of the file is honoured.
3. **Themes are bundles, and bundles are versioned artifacts.** A bundle
   declares the spec revision it was written against. An engine resolves,
   validates, and installs it, or rejects it with the reason. Nothing in
   between is allowed.
4. **Authorship carries forward.** Anyone who puts work into a bundle, from
   layout to illustration to type, is named in the manifest, and dawn displays
   that credit where the bundle asks for it.
5. **The CLI is the interface of record.** Everything a machine needs, from
   first-run configuration to theme installation to shell completion, is a
   subcommand of `dawn`. There is no second tool to install.

## How it works

dawn speaks the greetd socket protocol directly and mirrors the PAM
conversation as it happens: visible prompts, hidden prompts, informational
messages, and failures. Session lists come from `.desktop` entries already
present on the system, so the picker reflects what the machine actually
advertises.

Rendering is a three-part pipeline:

1. **Scene.** Wallpaper (still, GIF, animated WebP, or video through `ffmpeg`),
   boxy widgets, hairline forms, and per-theme decor are laid out as data. The
   CPU rasteriser guarantees every rounded corner, gradient, and glyph in the
   configured layout.
2. **Composite.** The UI layer and the wallpaper frame are handed to wgpu. The
   backdrop blur pipeline runs two separable Gaussian passes plus the colour
   pipeline entirely on the GPU; readback feeds the shared glass cache. When
   the GPU path rejects an adapter, the same scene presents from the CPU cache.
3. **Present.** One full-screen triangle per frame. The UI texture is
   re-uploaded only when the UI changes, so a 30 fps video wallpaper costs one
   upload per frame and a handful of uniforms.

Widgets ship as part of the scene data: clock variants, calendar, battery,
system telemetry, an MPRIS media widget with transport controls, and
declarative plugin widgets drawing text, clocks, media, or PNG sprites. Motion
primitives include parallax, choreographed entrances, text wave, scramble,
glitch, particle emitters, and per-theme cut transitions. Plugins cannot run
code. See [Plugins](#plugins).

## The bundle specification

Status: **shipped** as DSB-2. [docs/bundle-spec.md](docs/bundle-spec.md) is the
normative text; this section is a summary.

A dawn theme is a **bundle**: an immutable directory tree, versioned, credited,
and addressed by a stable id. dawn installs bundles, validates them, and renders
them; nothing else does. Revision numbers are literal: a bundle written for
revision 2 states revision 2, and an engine that does not know a revision
rejects it instead of guessing.

Each bundle is exactly four artifacts:

```text
<id>/
  theme.toml      DSB-2 manifest. Required; the directory name must equal id.
  config.toml     A complete dawn configuration. Required.
  preview.webp    Valid WebP, at most 1600x1080 and 2 MiB. Required.
  assets/         The asset root. Required; may be empty.
```

The manifest carries the id, name, semver, description, preview, an engine
minimum, at least one author, and optional credits. License fields are not part
of DSB-2. The engine enforces eight numbered rules: the four valid artifacts and
a matching directory name, asset references confined to `assets/`, relative
paths with no `..` or symlinks, full config validation at install time, a known
manifest revision and a satisfied engine minimum, the id pattern and per-store
uniqueness, at least one author, and a staged atomic install. Rejections are
reported as `rule <n>: <file>: <what>`.

`dawn check PATH` validates an uninstalled tree and exits nonzero on any
problem. `dawn new PARENT --id ID` writes a commented, ready-to-check bundle.
`dawn add PATH --migrate` upgrades a revision 1 tree; the six bundles in
`examples/themes/` stay at revision 1 as the compatibility demo. See
[docs/bundle-spec.md](docs/bundle-spec.md) for the full layout, schema, rules,
and migration semantics.

### Resolution and stores

There is one canonical system store and optional read-only additional stores.
The system store is `/usr/share/dawn/themes`; a greeter runs as the `greeter`
user and cannot read your home, so that store is the source of boot-time
themes. `DAWN_THEME_DIRS` adds extra read-only stores, which is how NixOS
profiles and package-managed themes are attached.

Resolution, per spec, is a lookup in this order:

1. `/usr/share/dawn/themes`,
2. colon-separated `DAWN_THEME_DIRS` entries,
3. the per-user store, `$XDG_DATA_HOME/dawn/themes`. A bundle found only here
   is a packaging mistake; dawn loads it and names the corrective `add`
   command on screen rather than pretending the system store is complete.

`dawn add` targets the system store by default and says `sudo dawn add ...`
when the process lacks permission to write it. `--user` is available for
private verification on a system you administer.

## Session rituals

Status: **designed**, engine work landing in waves. DSB-2 reserves the
manifest vocabulary for these mechanics, and the glossary below is the shape
they will land in. Inert until a bundle or the config enables it.

dawn treats repeated sign-ins as a game loop rather than as disposable events.
The mechanics borrow from the engagement patterns that make live games sticky,
with the line dawn will not cross stated up front: no mechanic delays
authentication, hides the password field, fetches anything from the network,
or makes a user feel punished.

| Mechanic | Rule | Principle | Default |
| --- | --- | --- | --- |
| Boot drop | On boot, one title card with weighted rarity: `common` 70%, `rare` 25%, `saga` 5%. | variable reward | off |
| Sign-in streak | Consecutive successful sign-ins, shown as a corner badge. One `grace day` per week so real life does not erase the number. | endowed progress, loss aversion (softened) | off |
| Milestones | Permanent 3, 7, 14, 30, 100, 365-day salutes rendered as the boot drop. | milestone gratitude | off |
| Season palette | Wallpaper and accent from the local date, not the clock time. | scarcity cadence | off |
| Mystery playlist | The media widget surfaces one album per day from the local library in a weighted shuffle. | variable reward, curiosity | off |

All state is local, in `XDG_DATA_HOME`, single-player, and off unless a config
turns it on. The spec makes one thing explicit: dawn computes the streak from
its own logs and never from outside data.

## Command line

Everything runs through `dawn`. The full reference lives in `dawn --help` and
in [docs/installation.md](docs/installation.md); the spine is:

| Command | What it does |
| --- | --- |
| `dawn init` | Write the first-run `/etc/dawn/config.toml`. Refuses to overwrite without `--force`; prints the greetd line when it is done. `--config PATH` writes elsewhere and is the testing seam. |
| `dawn add [paths]` | Install one bundle or a directory of bundles into the system store. Validates against DSB-2 first; `--migrate` upgrades revision 1 trees. |
| `dawn check <path>` | Validate an uninstalled bundle tree. Exits nonzero with a numbered `rule <n>: <file>: <what>` report. |
| `dawn new <parent> --id <id>` | Create a commented DSB-2 bundle with all four required artifacts and a generated preview. |
| `dawn dev [path]` | Windowed dry-run studio with hot reload, a dev HUD, and optional `--theme`, `--size`, and `--no-hud`. |
| `dawn list` | Resolvable ids with authors, versions, and spec revisions. |
| `dawn rm <id>` | Remove a bundle. Atomic; keeps the system store consistent. |
| `dawn completions [shell]` | Emit bash, zsh, or fish completions. |
| `dawn --theme <id>` | Run a bundle by id. |
| `dawn --dry-run` | Windowed run with simulated auth. |
| `dawn --check-config` | Validate the config, sessions, plugins, and bundle. |
| `dawn --screenshot path` | Render one frame and exit. |
| `dawn --list-gpus` | List adapters and the selection this config makes. |

All switches have short forms (`-d`, `-t`, `-c`, `-k`, `-g`, `-s`, `-f`); the
full map is in `dawn --help`.

## Try it without touching greetd

```sh
dawn --dry-run                                  # window with simulated auth
dawn --screenshot preview.png --size 1920x1080  # one frame, no display server
dawn --check-config                             # validate everything and exit
```

`--dry-run` opens a real window but never contacts greetd and never starts a
session. Any non-empty password works. It is for looking at a config, not for
logging in.

## Theme development

`dawn dev` is the hot-reload studio for theme authors:

```sh
dawn dev examples/themes/kinich            # watch a bundle tree
dawn dev path/to/config.toml               # watch one config file
dawn dev --theme kinich --size 1600x900    # installed bundle, custom size
```

It opens a windowed dry-run greeter at 1280x800 by default, validates a bundle
tree before starting, and reloads on a settled edit. A broken edit keeps the
last good theme and shows a crimson error in the HUD instead of exiting. F5
forces a reload, F1 toggles the HUD, and Ctrl+Q quits; Escape stays the normal
field-clearing key. See [docs/dev.md](docs/dev.md) for the watching rules,
reload guarantees, and why the studio never contacts greetd.

## Install

Everything is driven by the `dawn` CLI itself. No installer scripts, no
hand-copied files.

### 1. Build and place the binary

```sh
cargo build --release --locked
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
```

### 2. First run: let dawn write its own config

```sh
sudo dawn init          # creates /etc/dawn/config.toml with sane defaults
sudo dawn init --force  # rewrite it after an upgrade
```

### 3. Install themes the same way

```sh
sudo dawn add examples/themes/kinich      # into /usr/share/dawn/themes
sudo dawn add examples/themes/hutao-montagem --force
dawn list                                 # what is installed, with ids
```

### 4. Hook greetd to it, once

greeters are wired through a single line in greetd's config:

```toml
# /etc/greetd/config.toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- /usr/local/bin/dawn"
user = "greeter"
```

Then make greetd the only display manager on the machine:

```sh
sudo systemctl disable <your-dm>.service   # gdm, sddm, lightdm, ly, ...
sudo systemctl enable greetd.service
sudo reboot
```

If `enable` complains that the `display-manager.service` alias already exists,
remove the stale symlink first:

```sh
sudo rm -f /etc/systemd/system/display-manager.service
sudo systemctl enable greetd.service
```

### 5. Shell completions

```sh
dawn completions zsh > "${fpath[1]}/_dawn"   # zsh
dawn completions bash | sudo tee \
  /etc/bash_completion.d/dawn > /dev/null    # bash
dawn completions fish > ~/.config/fish/completions/dawn.fish
```

`dawn completions` emits completion scripts for bash, zsh, and fish, including
theme ids for `--theme` and file paths where your shell supports them.

### 6. Pick a theme for boot time

Either on the command line or in the config:

```sh
sudo dawn --theme kinich        # one-off override
```

```toml
# /etc/dawn/config.toml
[theme]
package = "kinich"
```

A theme pointer is not merged with the bundle. The bundle's config is the whole
config.

## Configuration

dawn reads, in order:

1. the path given with `--config PATH`
2. the path named by `DAWN_CONFIG`
3. `/etc/dawn/config.toml`

There is no per-user config search. A greeter runs as a different user with no
useful `HOME`, so a config in an XDG directory would not be the file that the
login screen reads. `dawn init` writes the system file; `dawn init --config
PATH` writes an explicit file for a test or a custom install. A missing file
means built-in defaults, and dawn prints one line: `no machine config; create
it: sudo dawn init`. A bad field falls back on its own without discarding the
rest of the file, and the login screen shows a short warning naming the key.

A minimal wallpaper and form setup:

```toml
[theme]
preset = "glass-dark"

[background]
kind = "auto"
image = "wallpapers/dawn.gif"
fit = "cover"
fps = 30
memory_limit_mb = 128

[login]
greeting = "Welcome back"
width = 416

[login.username]
label = "Account"
placeholder = "Username"

[login.password]
placeholder = "Password"
mask = "*"
```

Relative paths resolve against the config file's directory.
[docs/config.md](docs/config.md) covers every key, and [`examples/`](examples/)
has complete configs and bundles.

## What dawn does not do

Boundaries are stated because they are load-bearing:

- dawn does not manage the user's desktop session or the greeter user's
  environment.
- dawn does not show or process web content, and it does not open network
  connections for artwork.
- dawn does not run theme or plugin code, ever. A bundle's behaviour is its
  data.
- dawn does not keep display-manager state in its own formats; greetd owns the
  session lifecycle.

## Security

- Credentials live in zeroizing buffers and go from the input field to the auth
  worker and then to greetd. Nothing outside `auth` can read them.
- The UI thread never blocks on PAM, the greetd socket, image decoding, or
  D-Bus.
- greetd and PAM text is untrusted: control characters are stripped, whitespace
  is squeezed, and length is bounded before display.
- greetd frames, plugin manifests, plugin PNGs, and wallpapers have explicit
  size, dimension, allocation, and decode limits.
- Plugin paths are relative, allow-listed, and canonicalised inside the plugin
  directory.
- Album art is read only from local `file://` URIs. Network artwork is never
  fetched.
- The crate forbids unsafe code and denies `unwrap` and `expect` in runtime
  paths.

Read [SECURITY.md](SECURITY.md) before reporting an authentication or credential
issue.

## Requirements

- Linux with greetd. On Wayland, run dawn under a kiosk compositor such as
  `cage`; dawn does not start its own. On X11, run it in a session that gives
  the window focus.
- Rust 1.89 or newer to build. The release binary has no runtime dependency on
  GTK, Qt, a web engine, or a compositor library.
- `ffmpeg` on the greeter's `PATH` for video wallpapers. Stills, GIF, and WebP
  do not need it.
- A Vulkan or OpenGL driver for the GPU path. Optional; software presentation
  is supported.

Package lists for Arch, Debian, Ubuntu, and Nix, plus the X11 fallback, fonts,
GPU fallback, upgrade, and uninstall steps are in
[docs/installation.md](docs/installation.md).

## Specs and proposals

- The bundle spec, DSB-2, is part of this repository and is enforced by the
  engine. Themes written against it install identically on any machine running
  a dawn binary that knows the revision. The normative text is
  [docs/bundle-spec.md](docs/bundle-spec.md).
- Session rituals above are a designed proposal. The manifest vocabulary is
  reserved in DSB-2 so early bundles do not need a breaking change later.

## Docs

- [docs/installation.md](docs/installation.md): packages, greetd and cage setup,
  X11, fonts, GPU fallback, upgrade, and packaging.
- [docs/config.md](docs/config.md): configuration reference.
- [docs/dev.md](docs/dev.md): the `dawn dev` hot-reload theme studio.
- [docs/architecture.md](docs/architecture.md): modules, auth state machine,
  threading, rendering, and the background engine.
- [docs/plugins.md](docs/plugins.md): plugin API v1.
- [docs/troubleshooting.md](docs/troubleshooting.md): black screen, focus, auth,
  video, fonts, CPU, and reporting.
- [CONTRIBUTING.md](CONTRIBUTING.md): build, test, and review rules.
- [CHANGELOG.md](CHANGELOG.md): release history.

## License

The code is MIT OR Apache-2.0. The embedded Inter font is under the SIL Open
Font License; see [`assets/LICENSE.txt`](assets/LICENSE.txt). Example artwork
and sample media carry their own provenance, so check before redistributing
them.
