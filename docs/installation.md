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

`DAWN_THEME_DIRS` is a colon-separated list of extra read-only system theme
stores. `dawn add` never writes there, which keeps NixOS declarative. On a
mutable system the canonical store is `/usr/share/dawn/themes`, and `dawn add`
writes there by default (run it with sudo). `--user` is still available for the
deprecated XDG store used by packaging tests and sandboxes.

## Install the binary

Copy it to a directory on the greeter's `PATH`:

```sh
sudo install -Dm755 target/release/dawn /usr/local/bin/dawn
```

Use `/usr/bin/dawn` if your package manager owns that prefix. There is nothing
else to run.

## Install a config

Let dawn write the commented first-run config:

```sh
dawn init
```

It refuses to replace an existing file; pass `--force` to rewrite it. Use
`dawn init --config PATH` to write an explicit file, for example in a test or a
staging directory. dawn asks for sudo itself when the target is not writable and
the process has a terminal. Set `DAWN_NO_SUDO=1` to get the printed command
instead.

dawn reads, in order:

1. `--config PATH`
2. `DAWN_CONFIG`
3. `/etc/dawn/config.toml`

There is no per-user config search. A greeter usually has no useful `HOME`, so a
file under `$XDG_CONFIG_HOME` would not be the one the login screen reads.
When none of the three exists, dawn runs on the built-in defaults and prints
one line: `no machine config; create it: sudo dawn init`.

Keep `/etc/dawn/config.toml` readable by the `greeter` user:

```sh
sudo chmod 644 /etc/dawn/config.toml
```

If the config names assets or plugins, the `greeter` user must be able to read
them too. Theme bundles installed with `dawn add` are made readable for this
reason.

## Use a theme bundle

A DSB-2 bundle is one directory with exactly four required artifacts. Media
lives under `assets/`. The normative layout, manifest schema, and numbered
validation rules are in [bundle-spec.md](bundle-spec.md).

```text
midnight/
  theme.toml
  config.toml
  preview.webp
  assets/
    crest.png
    wallpaper.mp4
```

`theme.toml` describes the bundle:

```toml
[spec]
revision = 2

[bundle]
id = "midnight"
name = "Midnight"
version = "1.0.0"
description = "A dark cardless login."
preview = "preview.webp"

[bundle.engine]
min = "0.5.0"

[[author]]
name = "Ada Grey"
role = "layout"
```

`config.toml` is a normal dawn config. All asset paths in it must be relative to
the bundle root and must live under `assets/`. A bundle must not set
`layout.save = true` or select another theme with `[theme].package`.

Create a commented bundle and validate it before installing:

```sh
dawn new midnight --parent ./themes
dawn check ./themes/midnight
dawn add ./themes/midnight
```

`dawn check` exits 0 with zero issues and prints a numbered report such as
`rule 2: config.toml: image "wallpaper.mp4" must live under assets/` otherwise.
Install one bundle, or a directory containing several bundles:

```sh
dawn add ./midnight
dawn add ./themes
dawn list
```

The default store is `/usr/share/dawn/themes`. A real greeter runs as the
`greeter` user, which usually cannot read your home directory, so system
themes are the normal choice. `dawn add` asks for sudo itself when it has a
terminal and the store is not writable; `DAWN_NO_SUDO=1` prints the command
instead. `--user` installs into `$XDG_DATA_HOME/dawn/themes`, normally
`~/.local/share/dawn/themes`, for packaging tests and sandboxes. `--system` is
still accepted and is now the default. Lookup checks the system store and every
`DAWN_THEME_DIRS` entry first; a theme that exists only in the user store still
loads but warns that it should be reinstalled with `dawn add PATH --force`.

`dawn add` validates the manifest, the full config, every asset reference, and
the tree against the DSB-2 rules. It rejects symlinks, absolute asset paths,
references outside `assets/`, and bundles larger than 8 GiB. Installs are staged
and renamed into place, so a failed validation leaves the previous theme
untouched. Pass `--force` to replace an installed theme with the same id.

Revision 1 bundles still install as before, with one warning naming the
migration command. Upgrade a tree in place with:

```sh
dawn add ./old-theme --migrate --force
```

Migration moves root-level media into `assets/`, patches the matching config
references, fills in a placeholder author marked as a TODO, rewrites the
manifest as revision 2, and validates the result before replacing anything.

Select the boot theme by id:

```sh
dawn use midnight
```

That writes `[theme] package = "midnight"` into `/etc/dawn/config.toml` and
leaves the rest of the file as it is. The equivalent hand edit is:

```toml
# /etc/dawn/config.toml
[theme]
package = "midnight"
```

A pointer config is not merged with the bundle. The bundle's config is the
single source of truth, and the pointer file only chooses it.

To try a theme without changing the config:

```sh
dawn --theme midnight --dry-run
```

Remove a system theme with `dawn remove midnight` (the default store).
Use `dawn remove --user midnight` for the user store; `--system` is still
accepted. `dawn list` shows each theme's id, version, source, name, and which
one is active. Broken bundles are reported as warnings and skipped instead of
breaking the list.

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
