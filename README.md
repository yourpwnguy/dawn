# dawn

[![CI](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml/badge.svg)](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-red)](#security)
[![greetd](https://img.shields.io/badge/greetd-native-orange)](https://git.sr.ht/~kennylevinsen/greetd)
[![spec](https://img.shields.io/badge/bundle%20spec-DSB--2.0-informational)](docs/bundle-spec.md)

| Hu Tao | Genshin | Kinich | Hu Tao Montagem |
| --- | --- | --- | --- |
| [![Hu Tao theme in motion](docs/dawn-hutao.webp)](docs/dawn-hutao.mp4) | [![Genshin theme in motion](docs/dawn-genshin.webp)](docs/dawn-genshin.mp4) | [![Kinich theme in motion](docs/dawn-kinich.webp)](docs/dawn-kinich.mp4) | [![Hu Tao Montagem in motion](docs/dawn-hutao-montagem.webp)](docs/dawn-hutao-montagem.mp4) |

Four bundled themes. Click a preview to open the clip.

dawn is a graphical login greeter for
[greetd](https://git.sr.ht/~kennylevinsen/greetd). It renders the sign-in screen
from a versioned theme bundle, runs the PAM conversation, and starts the session
you pick. If the window cannot start, it falls back to a terminal login prompt,
so the machine stays reachable.

Themes are data only: a bundle cannot run code, and dawn makes no network
connections at login.

## What it does

- **Renders from data.** The layout, palette, wallpaper, motion, and widgets of
  a theme are TOML tables read at boot. A bad value falls back on its own, with
  a short warning on screen naming the key, while the rest of the file is kept.
- **Mirrors the PAM conversation.** Visible prompts, hidden prompts,
  informational messages, and failures are rendered as they arrive. The session
  list comes from the `.desktop` entries already on the machine.
- **Renders on the GPU when it can.** Layout is rasterised with tiny-skia and
  presented through wgpu on Vulkan or OpenGL, with the glass and backdrop blur
  running on the GPU. Without a usable adapter the same scene presents from the
  CPU cache.
- **Ships widgets and motion.** Clock variants, calendar, battery, system
  telemetry, and an MPRIS media widget with transport controls. Motion includes
  parallax, entrances, text wave, scramble, particle emitters, and per-theme cut
  transitions.
- **Accepts declarative plugins.** Plugins draw text, clocks, media art, or PNG
  sprites from a manifest. Plugin code never runs.

## Install

### 1. Build and place the binary

```sh
cargo build --release --locked
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
```

### 2. Write the config

```sh
sudo dawn init          # creates /etc/dawn/config.toml from the defaults
sudo dawn init --force  # rewrite it after an upgrade
```

### 3. Install a theme

```sh
sudo dawn add examples/themes/kinich      # into /usr/share/dawn/themes
sudo dawn add examples/themes/hutao-montagem --force
dawn list                                 # installed ids, authors, versions
```

### 4. Hook greetd to it

In `/etc/greetd/config.toml`:

```toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- /usr/local/bin/dawn"
user = "greeter"
```

Then disable your current display manager and enable greetd:

```sh
sudo systemctl disable <your-dm>.service   # gdm, sddm, lightdm, ly, ...
sudo systemctl enable greetd.service
```

If `enable` complains that `display-manager.service` already exists, remove the
stale symlink first:

```sh
sudo rm -f /etc/systemd/system/display-manager.service
sudo systemctl enable greetd.service
```

### 5. Completions and the boot theme

```sh
dawn completions zsh > "${fpath[1]}/_dawn"
dawn completions bash | sudo tee /etc/bash_completion.d/dawn > /dev/null
dawn completions fish > ~/.config/fish/completions/dawn.fish
```

Choose the boot theme on the command line or in the config:

```sh
sudo dawn --theme kinich        # one-off override
```

```toml
# /etc/dawn/config.toml
[theme]
package = "kinich"
```

A theme pointer is not merged with the bundle. The bundle's `config.toml` is
the whole config for that theme.

Package lists for Arch, Debian, Ubuntu, and Nix, the X11 path, fonts, GPU
fallback, and upgrade steps are in [docs/installation.md](docs/installation.md).

## Themes

A theme is a bundle: one directory with four required files.

```text
name/
  theme.toml      manifest with id, version, engine minimum, and authors
  config.toml     the whole dawn configuration for this theme
  preview.webp    preview image, at most 1600x1080 and 2 MiB
  assets/         media referenced by the config
```

The directory name must match the manifest id. The engine enforces the format
at install time:

- the four artifacts must exist and the directory name must equal the id;
- asset paths are relative, stay inside `assets/`, and contain no `..` or
  symlinks;
- the config is fully validated, not just parsed;
- the manifest revision must be known and its engine minimum satisfied;
- at least one author is required, and credits are optional;
- the install is staged and swapped atomically.

Rejections read `rule <n>: <file>: <what>`.

```sh
dawn check examples/themes/kinich          # validate without installing
dawn new my-themes --id midnight           # scaffold a valid bundle
sudo dawn add examples/themes/kinich       # validate and install
sudo dawn add old-theme --migrate          # upgrade a revision 1 tree
```

Themes resolve from `/usr/share/dawn/themes` first, then `DAWN_THEME_DIRS`
entries, then `$XDG_DATA_HOME/dawn/themes`. A bundle found only in the user
store still loads, but dawn names the corrective `add` command on screen. The
normative format is [docs/bundle-spec.md](docs/bundle-spec.md).

## Theme development

`dawn dev` opens a windowed greeter that reloads when the config or bundle
changes:

```sh
dawn dev examples/themes/kinich            # watch a bundle tree
dawn dev path/to/config.toml               # watch one config file
dawn dev --theme kinich --size 1600x900    # installed bundle, custom size
```

It opens at 1280x800 and never contacts greetd. A broken edit keeps the last
good theme on screen and shows the error in the HUD. F1 toggles the HUD, F5
forces a reload, and Ctrl+Q quits; Escape stays the normal field-clearing key.
[docs/dev.md](docs/dev.md) covers the watching rules and reload limits.

## Configuration

dawn reads the first of these that exists:

1. `--config PATH`
2. `DAWN_CONFIG`
3. `/etc/dawn/config.toml`

There is no per-user config search. The greeter runs as another user without a
useful `HOME`, so an XDG config would not be the file the login screen reads. A
missing file means built-in defaults, and dawn prints one line:
`no machine config; create it: sudo dawn init`.

A small wallpaper and form setup:

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
[docs/config.md](docs/config.md) documents every key, and [`examples/`](examples/)
has complete configs and bundles.

## Command line

| Command | What it does |
| --- | --- |
| `dawn init` | Write `/etc/dawn/config.toml` (add `--force` to replace it). |
| `dawn add PATH` | Validate and install a bundle. `--migrate` upgrades rev 1 trees. |
| `dawn remove ID` | Remove an installed bundle. |
| `dawn list` | List installed bundles with authors and versions. |
| `dawn check PATH` | Validate a bundle tree without installing it. |
| `dawn new PARENT --id ID` | Create a minimal bundle. |
| `dawn dev [PATH]` | Windowed studio with hot reload. |
| `dawn completions SHELL` | Print bash, zsh, or fish completions. |
| `dawn --theme ID` | Run a bundle by id. |
| `dawn --dry-run` | Windowed run with simulated auth. |
| `dawn --check-config` | Validate the config, sessions, plugins, and bundle. |
| `dawn --screenshot PATH` | Render one frame and exit. |
| `dawn --list-gpus` | List adapters and the selection this config makes. |

Every run switch has a short form (`-c`, `-t`, `-d`, `-k`, `-g`, `-s`, `-f`);
`dawn --help` has the full map.

Try a theme without touching greetd:

```sh
dawn --dry-run                                  # window with simulated auth
dawn --screenshot preview.png --size 1920x1080  # one frame, no display server
dawn --check-config                             # validate everything and exit
```

`--dry-run` opens a real window but never contacts greetd and never starts a
session. Any non-empty password works.

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

## Docs

- [docs/installation.md](docs/installation.md): packages, greetd and cage setup,
  X11, fonts, GPU fallback, upgrade, and packaging.
- [docs/config.md](docs/config.md): configuration reference.
- [docs/bundle-spec.md](docs/bundle-spec.md): the DSB-2 bundle format.
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
