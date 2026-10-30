# dawn

dawn is the login screen for greetd on Linux. It draws the sign-in screen from a
theme, takes the username and password, and hands the login back to greetd.

[![CI](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml/badge.svg)](https://github.com/yourpwnguy/dawn/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![greetd](https://img.shields.io/badge/greetd-native-orange)](https://git.sr.ht/~kennylevinsen/greetd)
[![spec](https://img.shields.io/badge/bundle%20spec-DSB--2.0-informational)](docs/bundle-spec.md)

| Hu Tao | Genshin | Kinich | Hu Tao Montagem |
| --- | --- | --- | --- |
| [![Hu Tao theme in motion](docs/dawn-hutao.webp)](docs/dawn-hutao.mp4) | [![Genshin theme in motion](docs/dawn-genshin.webp)](docs/dawn-genshin.mp4) | [![Kinich theme in motion](docs/dawn-kinich.webp)](docs/dawn-kinich.mp4) | [![Hu Tao Montagem in motion](docs/dawn-hutao-montagem.webp)](docs/dawn-hutao-montagem.mp4) |

Four of the six example themes in `examples/themes`: `hutao-chibi`, `genshin`,
`kinich`, and `hutao-montagem`. Click a preview to open the clip.

## What is this

greetd is a small login manager. It starts a program called a greeter, waits for
that program to collect a username and password, then starts the desktop session
you picked. dawn is that greeter.

A theme is a folder with a settings file and some images. dawn reads it when it
starts, so changing the folder changes what the login screen looks like. A
theme cannot run code, and dawn does not use the network while the login screen
is up.

If the window cannot start, dawn drops to a text login on the same console. A
missing GPU driver does not leave the machine unreachable.

## What you need

Before you build anything, install the tools and libraries on the machine that
will run the greeter.

- Linux with greetd. greetd starts the greeter and owns the login.
- cage. cage is a small Wayland compositor that shows the greeter window
  fullscreen. The X11 alternative is covered in
  [packaging/greetd-x11.toml](packaging/greetd-x11.toml).
- Rust 1.89 or newer.
- ffmpeg, only if you want video wallpapers. Stills and GIFs do not need it.

Arch:

```sh
sudo pacman -S --needed rustup base-devel git curl pkgconf wayland \
  libxkbcommon libx11 libxcb vulkan-icd-loader mesa fontconfig
sudo pacman -S --needed greetd cage noto-fonts polkit
sudo pacman -S --needed ffmpeg   # optional
rustup default stable
```

Debian and Ubuntu:

```sh
sudo apt update
sudo apt install build-essential git curl pkg-config \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libxcb1-dev libvulkan-dev \
  libegl1-mesa-dev libgles2-mesa-dev libfontconfig1-dev
sudo apt install greetd cage fonts-noto-core
sudo apt install ffmpeg   # optional
```

Check the Rust version:

```sh
rustc --version
```

If `rustc` is missing or older than 1.89, install Rust with rustup:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

On Nix, skip the manual build. [docs/installation.md](docs/installation.md#nix)
has the flake and the NixOS greetd setup.

## Get the code

```sh
git clone https://github.com/yourpwnguy/dawn.git
cd dawn
```

Run the rest of the commands in this README from the `dawn` directory.

## Build

```sh
make build
```

That runs `cargo build --release --locked`. The binary is
`target/release/dawn`.

The Makefile has no install target, on purpose. It builds and runs checks; you
install the result with the steps below.

## Try it before you touch the login

From a running desktop session:

```sh
./target/release/dawn --dry-run
```

A window opens with the built-in defaults: a clock, a username field, a password
field, and a session list. Type any non-empty password and press Enter. The
login is simulated and no session starts.

If there is no config file yet, dawn prints
`no machine config; create it: sudo dawn init` and runs on the built-in
defaults. That line is expected at this point.

To render a still frame instead of opening the window:

```sh
make preview CONFIG=examples/themes/kinich/config.toml
```

It writes `dawn-preview.png` at 1920x1080. Pass `CONFIG` because the Makefile
default points at `examples/config.toml`, which this tree does not contain.

## Install

Steps 2 to 4 write system paths. When you run them from a terminal, dawn asks
for sudo itself. Outside a terminal, or with `DAWN_NO_SUDO=1`, it prints the
exact command to run instead.

### Put the binary on the path

```sh
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
```

Use `/usr/bin/dawn` if your package manager owns that prefix.

### Write the config

```sh
dawn init
```

This writes `/etc/dawn/config.toml` with comments and the built-in defaults. The
file is mode 0644, so the account that runs the greeter can read it. If you edit
it by hand, keep it readable (`chmod 644`).

`dawn init` will not replace an existing file. Use `dawn init --force`
after an upgrade, or `dawn init --config PATH` to write somewhere else.

### Install a theme

List what the repository ships:

```sh
ls examples/themes
```

The folder names are the theme ids: `fischl`, `furina`, `genshin`,
`hutao-chibi`, `hutao-montagem`, and `kinich`. Install one, or all of them:

```sh
dawn add examples/themes/kinich --migrate
dawn add examples/themes --migrate
dawn list
```

`dawn add` checks the folder before copying it, then installs into
`/usr/share/dawn/themes`, which is where the greeter reads from. The examples in
this repository are revision 1, so `--migrate` upgrades the copy as it installs;
without it you get a warning naming the migration command. Add `--force` if a
theme with the same id is already installed.

### Choose the theme for the login screen

```sh
dawn use kinich
```

That sets `[theme] package = "kinich"` in `/etc/dawn/config.toml` and leaves the
rest of the file alone. You can add the key by hand instead:

```toml
[theme]
package = "kinich"
```

A theme's own `config.toml` replaces this machine config while that theme is
active. The two files are not merged.

To see the result before you log out:

```sh
dawn --theme kinich --dry-run
```

`--theme` uses an installed theme for one run. It does not install anything and
does not change the config.

## Connect greetd

Edit `/etc/greetd/config.toml`:

```toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- /usr/local/bin/dawn"
user = "greeter"
```

`cage` runs the greeter fullscreen. `-s` lets you switch to a text-only virtual
console while it is running. `user` is the account greetd runs the greeter as,
and it needs to read `/etc/dawn/config.toml` and `/usr/share/dawn/themes`. The
packaging examples use `greeter`; check yours with `id greeter`, and pick another
unprivileged account if that one does not exist.

If you would rather choose the theme here than in the config, pass it to dawn:

```toml
command = "cage -s -- /usr/local/bin/dawn --theme kinich"
```

A ready copy of the file is in
[packaging/greetd-cage.toml](packaging/greetd-cage.toml).

Then disable the display manager you use now and enable greetd:

```sh
sudo systemctl disable gdm.service   # your current display manager
sudo systemctl enable greetd.service
```

If `enable` complains that `display-manager.service` already exists, remove the
old alias and run it again:

```sh
sudo rm -f /etc/systemd/system/display-manager.service
sudo systemctl enable greetd.service
```

## Keep a way back

Do these before you reboot into the new login screen.

- Switch to a text console with `Ctrl+Alt+F3` and log in there. If that does not
  work, fix it before you go further.
- Do not start greetd from inside a running graphical session. Reboot instead.
- After the reboot, the old display manager can be restored with:

  ```sh
  sudo systemctl disable greetd.service
  sudo systemctl enable gdm.service   # your old display manager
  ```

- If the screen stays black, switch back to a text console and read the log:

  ```sh
  journalctl -u greetd -b
  ```

More cases are covered in
[docs/troubleshooting.md](docs/troubleshooting.md).

## Completions

Optional. `dawn completions` prints a script to stdout:

```sh
dawn completions bash | sudo tee /etc/bash_completion.d/dawn > /dev/null
dawn completions zsh > "${fpath[1]}/_dawn"
dawn completions fish > ~/.config/fish/completions/dawn.fish
```

It also supports `elvish` and `powershell`.

## Commands

| Command | What it does |
| --- | --- |
| `dawn use ID` | Switch the theme the next login starts. |
| `dawn init` | Write `/etc/dawn/config.toml`. Add `--force` to replace it. |
| `dawn add PATH` | Check and install a theme folder. `--migrate` upgrades revision 1 first. |
| `dawn remove ID` | Remove an installed theme from the system store. |
| `dawn list` | Show installed themes and which one is active. |
| `dawn check PATH` | Check a theme folder without installing it. |
| `dawn new NAME` | Create a theme folder to start from. `--id` sets the id. |
| `dawn dev PATH` | Open a theme in a window that reloads when you save. |
| `dawn completions SHELL` | Print shell completions. |
| `dawn --theme ID` | Use an installed theme for this run. |
| `dawn --dry-run` | Open the login screen and simulate authentication. |
| `dawn --check-config` | Check the config, sessions, and plugins, then exit. |
| `dawn --screenshot PATH` | Write one frame as a PNG. `--size WxH` sets the size. |
| `dawn --list-gpus` | Show GPU adapters and the one dawn would use. |

Every run option has a short form, such as `-c`, `-t`, `-d`, `-k`, `-g`, and
`-s`. `dawn --help` lists them all.

## Themes

A theme folder contains:

```text
name/
  theme.toml      name, id, version, authors, minimum engine version
  config.toml     the settings for this theme
  preview.webp    preview image, at most 1600x1080 and 2 MiB
  assets/         images and media used by the config
```

The folder name must match the id inside `theme.toml`. `dawn add` checks the
config and every file the config points at before it installs anything. Asset
paths are relative and stay inside `assets/`.

```sh
dawn new midnight --parent ~/themes
dawn check ~/themes/midnight
dawn dev ~/themes/midnight
```

The name is the display name. The id defaults to the name in lowercase with
dashes, and the folder uses the id. Override it with `--id` when you want a
different one:

`dawn dev` keeps the last working theme on screen when an edit breaks the
config. F1 toggles the overlay text, F5 reloads, and Ctrl+Q quits. It never
contacts greetd, so you can leave it open while you work.

The full theme format is in [docs/bundle-spec.md](docs/bundle-spec.md), and
`dawn dev` is covered in [docs/dev.md](docs/dev.md).

## Configuration

dawn reads the first of these files that exists:

1. the path passed with `--config PATH`
2. the path named by `DAWN_CONFIG`
3. `/etc/dawn/config.toml`

There is no per-user config. The greeter runs as another account, which usually
has no useful home directory. With no file at all, dawn uses the built-in
defaults and prints `no machine config; create it: sudo dawn init`.

A short example:

```toml
[theme]
preset = "glass-dark"

[background]
kind = "auto"
image = "/path/to/wallpaper.jpg"
fit = "cover"

[login]
greeting = "Welcome back"
width = 416
```

Relative paths resolve from the config file's directory. A bad value falls back
to its default and puts a short warning on screen; the rest of the file still
applies. [docs/config.md](docs/config.md) covers every key.

## Security

Passwords stay in memory owned by the authentication code and are not logged.
Text that arrives from the login conversation is cleaned before it is drawn.
The window never waits on the greetd socket or on image decoding, so a slow
wallpaper cannot hold up a password prompt. Read [SECURITY.md](SECURITY.md)
before reporting a credential issue.

## Docs

- [docs/installation.md](docs/installation.md): package lists, greetd and cage setup, X11, upgrade, uninstall.
- [docs/config.md](docs/config.md): every configuration key.
- [docs/bundle-spec.md](docs/bundle-spec.md): the theme folder format.
- [docs/dev.md](docs/dev.md): the hot-reload theme window.
- [docs/architecture.md](docs/architecture.md): modules, authentication flow, rendering.
- [docs/plugins.md](docs/plugins.md): widgets provided by a plugin.
- [docs/troubleshooting.md](docs/troubleshooting.md): black screen, focus, video, fonts.
- [CONTRIBUTING.md](CONTRIBUTING.md): build and test rules.
- [CHANGELOG.md](CHANGELOG.md): release notes.

## License

The code is MIT OR Apache-2.0. The embedded Inter font is under the SIL Open
Font License; see [`assets/LICENSE.txt`](assets/LICENSE.txt). Example artwork
and sample media carry their own provenance, so check before redistributing
them.
