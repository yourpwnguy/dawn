# Installing dawn

dawn is a greetd greeter for Linux. The install artifact is one binary. There
is no installer script and no daemon of its own: your package manager owns the
dependencies, your display manager config owns the launch command, and themes
live in a data directory that `dawn add` manages.

That keeps the greeter portable. You can build the binary on one machine, copy
`target/release/dawn` to another, and bring a theme bundle with it.

## Requirements

| Component | Needed for | Notes |
| --- | --- | --- |
| Linux with greetd | Real login | greetd sets `GREETD_SOCK` for the greeter process. |
| cage or another kiosk compositor | Wayland greeter | cage is the simplest supported setup. |
| Xorg plus a focusing session manager | X11 fallback | A bare X server often leaves the window unfocused. |
| `ffmpeg` on the greeter's `PATH` | MP4/WebM/MKV wallpapers | GIF and WebP do not need it. |
| Vulkan or OpenGL driver | GPU compositor | Optional. dawn falls back to softbuffer. |
| Fontconfig and fonts | Display type | The embedded Inter face is the last fallback. |

The greeter itself has no runtime dependency on GTK, Qt, a web engine, or a
compositor library. It needs the usual Linux graphics stack for its window.

## Build from source

You need Rust 1.89 or newer. rustup is the least surprising way to get it:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustc --version
```

Then build:

```sh
cargo build --release --locked
```

The result is `target/release/dawn`.

### Arch Linux

```sh
sudo pacman -S --needed base-devel pkgconf wayland libxkbcommon \
  libx11 libxcb vulkan-icd-loader mesa fontconfig rustup
rustup default stable
cargo build --release --locked
```

Runtime packages:

```sh
sudo pacman -S --needed greetd cage ffmpeg noto-fonts polkit
```

### Debian and Ubuntu

Build dependencies:

```sh
sudo apt update
sudo apt install build-essential pkg-config \
  libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-dev libxcb1-dev libvulkan-dev \
  libegl1-mesa-dev libgles2-mesa-dev libfontconfig1-dev
```

Install Rust with rustup if the distro toolchain is older than 1.89:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
cargo build --release --locked
```

Runtime packages:

```sh
sudo apt install greetd cage ffmpeg fonts-noto-core
```

If your Debian or Ubuntu release does not package `greetd` or `cage`, build
those from source or use a release that does. The greeter only needs the
`greetd` binary, its PAM service, and a compositor that can give the window
focus.

### Nix

The repository includes a flake:

```sh
nix build .#dawn
./result/bin/dawn --version
```

Install it into a profile:

```sh
nix profile install .#dawn
```

For NixOS, add this repository as a flake input, pass its package to your
configuration, and point greetd at it. Put theme bundles in a directory in the
Nix store and point `DAWN_THEME_DIRS` at it:

```nix
{ pkgs, dawn, ... }:
let
  themes = ./themes; # a directory of theme bundles
in
{
  environment.systemPackages = [ dawn pkgs.cage ];

  services.greetd = {
    enable = true;
    settings = {
      terminal.vt = 1;
      default_session = {
        command =
          "env DAWN_THEME_DIRS=${themes} " +
          "${pkgs.cage}/bin/cage -s -- ${dawn}/bin/dawn --theme midnight";
        user = "greeter";
      };
    };
  };
}
```

`DAWN_THEME_DIRS` is a colon-separated list of extra read-only theme stores.
`dawn add` never writes there, which keeps NixOS declarative. On a mutable
system you can still use `/usr/share/dawn/themes` and `dawn add --system PATH`.

## Install the binary

Copy it to a directory on the greeter's `PATH`:

```sh
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
```

Use `/usr/bin/dawn` if your package manager owns that prefix. There is nothing
else to run.

## Install a config

The neutral example is a complete config:

```sh
sudo install -Dm644 examples/config.toml /etc/dawn/config.toml
```

dawn reads, in order:

1. `--config PATH`
2. `$XDG_CONFIG_HOME/dawn/config.toml`
3. `~/.config/dawn/config.toml`
4. `/etc/dawn/config.toml`

A greeter usually has no useful `HOME`, so `/etc/dawn/config.toml` is the
normal choice. Keep it readable by the `greeter` user:

```sh
sudo chmod 644 /etc/dawn/config.toml
```

If the config names assets or plugins, the `greeter` user must be able to read
them too. Theme bundles installed with `dawn add` are made readable for this
reason.

## Use a theme bundle

A theme bundle is one directory containing a manifest, a complete config, and
every asset the config names:

```text
midnight/
  theme.toml
  config.toml
  preview.webp
  assets/
    crest.png
    rule.svg
  wallpaper.mp4
```

`theme.toml` describes the bundle:

```toml
schema = 1
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "A dark cardless login."
preview = "preview.webp"
```

`config.toml` is a normal dawn config. All paths in it must be relative to the
bundle directory. A bundle must not set `layout.save = true` or select another
theme with `[theme].package`.

Install one bundle, or a directory containing several bundles:

```sh
dawn add ./midnight
dawn add ./themes
dawn list
```

The default store is `$XDG_DATA_HOME/dawn/themes`, normally
`~/.local/share/dawn/themes`. A real greeter runs as the `greeter` user, which
usually cannot read your home directory. Install system themes with:

```sh
sudo dawn add --system ./midnight
```

That writes to `/usr/share/dawn/themes`. `dawn add` validates the manifest, the
full config, every referenced image, plugin directory, and the asset tree. It
rejects symlinks, absolute asset paths, and bundles larger than 8 GiB. Installs
are staged and renamed into place, so a failed validation leaves the previous
theme untouched. Pass `--force` to replace an installed theme with the same id.

Run a theme for one session:

```sh
dawn --theme midnight
```

Or make it persistent by pointing the system config at it:

```toml
# /etc/dawn/config.toml
[theme]
package = "midnight"
```

A pointer config is not merged with the bundle. The bundle's config is the
single source of truth, and the pointer file only chooses it.

Remove a user theme with `dawn remove midnight`. Use `dawn remove --system
midnight` for the system store. `dawn list` shows each theme's id, version,
source, and name. Broken bundles are reported as warnings and skipped instead
of breaking the list.

The repository has six complete bundles under `examples/themes/` you can add,
inspect, or copy as a starting point.

## Configure greetd

The recommended setup uses cage as the kiosk compositor:

```toml
# /etc/greetd/config.toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- /usr/local/bin/dawn"
user = "greeter"
```

If you do not use a pointer config, select the theme in the command instead:

```toml
command = "cage -s -- /usr/local/bin/dawn --theme midnight"
```

Ready copies live in `packaging/greetd-cage.toml` and
`packaging/greetd-x11.toml`. X11 is a fallback, not the default.

Disable the old display manager before enabling greetd. The
`display-manager.service` alias can point at only one unit:

```sh
sudo systemctl disable sddm.service
sudo systemctl enable greetd.service
```

If `enable greetd` reports an existing symlink, remove the stale alias first:

```sh
sudo rm -f /etc/systemd/system/display-manager.service
sudo systemctl enable greetd.service
```

Do not start greetd while your current graphical session is active. Reboot
when you are ready. Keep a TTY available (`Ctrl+Alt+F3`) for the first boot.

## Verify

Validate the config as the greeter user:

```sh
sudo -u greeter /usr/local/bin/dawn \
  --config /etc/dawn/config.toml --check-config
```

With a theme bundle:

```sh
sudo -u greeter /usr/local/bin/dawn --theme midnight --check-config
```

Test the actual window from your own session:

```sh
dawn --config /etc/dawn/config.toml --dry-run
```

`--dry-run` opens a window, but never connects to greetd and never starts a
session. Any non-empty password succeeds. Do not leave it in the real greetd
command.

Render one frame without a display server:

```sh
dawn --config /etc/dawn/config.toml --screenshot /tmp/dawn.png --size 1920x1080
```

If the greeter does not appear, read the log before changing anything:

```sh
sudo journalctl -u greetd -b --no-pager
```

Check that `cage` and `dawn` exist on the greeter user's `PATH`, that the
theme assets are readable, and that the display manager alias points at greetd:

```sh
ls -l /etc/systemd/system/display-manager.service
systemctl is-enabled greetd.service
```

## Upgrade

Replace the binary:

```sh
cargo build --release --locked
sudo install -m755 target/release/dawn /usr/local/bin/dawn
```

Configs and theme bundles are not touched. Re-run `--check-config` after an
upgrade and review `docs/config.md` for new keys.

## Uninstall

Restore the previous display manager first:

```sh
sudo systemctl disable greetd.service
sudo systemctl enable sddm.service
```

Then remove the files you installed:

```sh
sudo rm -f /usr/local/bin/dawn
sudo rm -rf /etc/dawn
sudo rm -rf /usr/share/dawn/themes
sudo rm -f /etc/greetd/config.toml
```

Remove `greetd`, `cage`, and `ffmpeg` only if nothing else uses them. Reboot
after restoring the old display manager.

## Packaging notes

- Install the binary at `/usr/bin/dawn` or `/usr/local/bin/dawn`.
- Install the default config at `/etc/dawn/config.toml` with mode 0644.
- Put system themes under `/usr/share/dawn/themes/<id>/` with readable
  permissions. Each theme must have `theme.toml` and `config.toml`.
- Ship only media you have the right to redistribute.
- Depend on `greetd` and `cage` when the package provides the whole greeter.
- Recommend `ffmpeg` and a Vulkan or OpenGL driver, but do not require them for
  a text login.
- Keep the terminal fallback available. A package that removes the TTY path is
  harder to recover on a broken driver.

## Security notes

- dawn never logs passwords.
- Run the greeter as the unprivileged `greeter` user, not root.
- Keep `/etc/dawn/config.toml` and every referenced asset readable but not
  world-writable.
- Treat wallpapers, plugin directories, and theme bundles as untrusted input.
  The theme installer rejects symlinks and absolute asset paths. The wallpaper
  decoder has size and allocation limits.
- Read `SECURITY.md` before reporting an auth or credential issue.
