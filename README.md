# dawn

dawn is a login greeter for [greetd](https://git.sr.ht/~kennylevinsen/greetd). It
replaces the plain text prompt with a graphical login screen, but it is still a
login screen: you type a username and password, pick a session, and sign in.

The renderer is small and self-contained. Frames are laid out with tiny-skia and
presented through wgpu when a GPU is available, or softbuffer when it is not. If
there is no display server at all, dawn falls back to a text prompt so the
machine stays reachable.

| Hu Tao | Genshin | Kinich |
| --- | --- | --- |
| [![Hu Tao theme in motion](docs/dawn-hutao.webp)](docs/dawn-hutao.mp4) | [![Genshin theme in motion](docs/dawn-genshin.webp)](docs/dawn-genshin.mp4) | [![Kinich theme in motion](docs/dawn-kinich.webp)](docs/dawn-kinich.mp4) |

Three bundled themes in motion. Click a preview to open the 60 fps clip.

## How it works

Most of dawn's behaviour comes from being a greeter first and a pretty screen
second.

It speaks the greetd socket protocol directly and handles the PAM conversation
as it comes: visible prompts, hidden prompts, informational messages, and
failures. Sessions are read from `.desktop` files, so whatever the system
already advertises shows up in the picker.

Wallpapers can be still images, GIFs, animated WebP, or video through `ffmpeg`.
There are fit modes, per-output overrides, a frame cap, idle pause, and a memory
ceiling for decoded frames.

Layouts are configured rather than hard-coded. There is a compact card, a wider
hero panel, and a cardless form with line fields. Colours, sizes, offsets,
labels, and hints are all configurable, and the card and widgets can be dragged
with their positions saved. A theme bundle packs one of those configs together
with the assets it uses.

The rest is small conveniences: a clock and an MPRIS media widget, plus
declarative plugin widgets that draw text, clocks, media, or PNGs. Plugins are
inert. They cannot run code, read keystrokes, or see credentials.

- Calendar, battery, and system widgets for the corners of a HUD theme. They
  read the local date, `/sys/class/power_supply`, and `/proc` only.

## When something is broken

Failure is handled field by field, not by throwing away the whole config. A bad
colour falls back to its default and the screen shows a short warning naming the
key. A missing wallpaper falls back to the gradient. A broken plugin is skipped.
A broken GPU path falls back to software, and a broken graphical frontend falls
back to the terminal. A login screen should never be the reason you cannot log
in.

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

## Install

dawn is one binary. There is no installer script. Build it, copy it somewhere
the greeter can execute, and let the package manager own greetd, cage, and
ffmpeg.

```sh
cargo build --release --locked
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
sudo install -Dm644 examples/config.toml /etc/dawn/config.toml
```

Then point greetd at it:

```toml
# /etc/greetd/config.toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- /usr/local/bin/dawn"
user = "greeter"
```

Then disable whatever display manager you currently use and enable greetd in
its place. Only one unit can own the `display-manager.service` alias, so do not
leave the old one enabled:

```sh
sudo systemctl disable <your-display-manager>.service   # gdm, sddm, lightdm, ly, ...
sudo systemctl enable greetd.service
sudo reboot
```

If `enable` complains that the alias already exists, remove the stale symlink
first and try again:

```sh
sudo rm -f /etc/systemd/system/display-manager.service
sudo systemctl enable greetd.service
```

Package lists for Arch, Debian, Ubuntu, and Nix, plus the X11 fallback, theme
installation as the greeter user, verification, and uninstall steps are in
[docs/installation.md](docs/installation.md).

## Themes

A theme is a directory, not a loose TOML file. It carries a manifest, a complete
config, and every asset that config names.

```text
midnight/
  theme.toml      # id, name, version, description, preview
  config.toml     # a complete dawn config
  preview.webp
  assets/
  wallpaper.mp4
```

Install one bundle or a directory of them, then list what you have:

```sh
dawn add ./midnight
dawn add ./themes
dawn list
```

`dawn add` validates the manifest, the config, every referenced asset, and the
tree before it copies anything. It rejects symlinks and absolute asset paths, so
a bundle can move between machines intact. Installs are staged and renamed into
place, which means a failed validation leaves the previous copy untouched.

The default store is `$XDG_DATA_HOME/dawn/themes`. A real greeter runs as the
`greeter` user and usually cannot read your home, so boot-time themes go in the
system store:

```sh
sudo dawn add --system ./midnight
```

Run a bundle for one session, or make it the default:

```sh
dawn --theme midnight
```

```toml
# /etc/dawn/config.toml
[theme]
package = "midnight"
```

A pointer config is not merged with the bundle. The bundle's config is the whole
config.

## Try it without touching greetd

```sh
dawn --dry-run                                  # window with simulated auth
dawn --screenshot preview.png --size 1920x1080  # one frame, no display server
dawn --check-config                             # validate everything and exit
```

`--dry-run` opens a real window but never contacts greetd and never starts a
session. Any non-empty password works. It is for looking at a config, not for
logging in.

## Configuration

dawn reads, in order:

1. the path given with `--config`
2. `$XDG_CONFIG_HOME/dawn/config.toml`
3. `~/.config/dawn/config.toml`
4. `/etc/dawn/config.toml`

A missing file means built-in defaults. A bad field falls back on its own
without discarding the rest of the file, and the login screen shows a short
warning naming the key.

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

## Example themes

The renderer is driven by a small design-token system, not a fixed screen. The
same engine produces the frosted card, the hero layout, and the cardless
terminal form. There are six complete bundles in
[`examples/themes/`](examples/themes/), each with editable SVG decor sources
next to the rendered PNGs.

Example wallpapers are optional media and are not covered by the project
licence. Use your own, or let a config fall back to its gradient.

## Plugins

A plugin is a directory with a `plugin.toml`. It describes inert text, clock,
media, and PNG widgets. There is no code execution, no callback into the host,
and no access to the username, password, or keyboard. See
[docs/plugins.md](docs/plugins.md) and
[`examples/plugins/hello-world`](examples/plugins/hello-world).

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

## Docs

- [docs/installation.md](docs/installation.md): packages, greetd and cage setup,
  X11, fonts, GPU fallback, upgrade, and packaging.
- [docs/config.md](docs/config.md): configuration reference.
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
