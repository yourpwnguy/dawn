# Contributing to dawn

Keep the greeter boring where it matters: authentication, input focus, and the
TTY fallback must keep working when one optional feature breaks.

## Before you start

- Rust 1.89 or newer.
- Linux with a Wayland session or X11 display for window checks.
- `ffmpeg` if you touch video wallpapers.
- A GPU with Vulkan or OpenGL if you touch the compositor. `DAWN_GPU=0` gives
  you the software path.

Read `docs/architecture.md` and `docs/config.md` first. The config file is the
user-facing API, so a new key needs a validator range, a default, docs, and a
test.

## Build and test

```sh
cargo fmt
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

The full test suite is fast. Do not mark a change done because the build
succeeds; run the tests and read the output.

Visual changes need a screenshot check at 1920x1080 and 1366x768:

```sh
./target/release/dawn --config examples/config.toml \
  --screenshot /tmp/dawn.png --size 1920x1080
```

For live checks, use `--dry-run` with a config. It needs a display server for
the window. A headless check can use Xvfb:

```sh
Xvfb :99 -screen 0 1280x800x24 &
DISPLAY=:99 XDG_STATE_HOME=/tmp/dawn-verify \
  ./target/release/dawn --config examples/config.toml --dry-run
```

## Dependency policy

CI runs `cargo deny check` against `deny.toml`. It treats known advisories,
unsound releases, yanked versions, and unmaintained crates in the whole
dependency graph as errors. It also fails on wildcard requirements and on any
source other than crates.io.

Run the same check before opening a PR that touches `Cargo.toml` or
`Cargo.lock`:

```sh
cargo install cargo-deny --locked   # once
cargo deny check
```

Or use `make audit`. If a check fails, update or remove the dependency. Ignoring
an advisory is a last resort: add a `reason` in `deny.toml` and open an issue to
track the upgrade. An ignore without a reason will not be merged.

The workflow in `.github/workflows/ci.yml` is part of the supply chain too. Keep
actions pinned to a commit SHA and the workflow token read-only. Dependabot
opens updates for both cargo dependencies and the pinned actions.

## Code style

- `cargo fmt` owns layout. Do not hand-format around it.
- No warnings. Clippy runs with `-D warnings`.
- Keep modules focused. A file should have one reason to exist.
- Prefer explicit error handling over `unwrap` or `expect` in library code.
- Do not add a dependency for something the standard library or an existing
  crate already does. If a new dependency is justified, pin it and explain why.
- No em dashes in code, comments, docs, configs, or commit messages. Use
  commas, periods, parentheses, colons, or normal hyphens.
- Comments explain why, not what. A comment that restates the next line is
  noise.

## Config changes

A new key needs all of these:

1. A field on the raw TOML struct.
2. A validated field on the resolved struct, with a safe default and range.
3. A merge rule if the key belongs to `[login.field]`.
4. A line in `docs/config.md`.
5. At least one parse test, including a bad value if the range matters.
6. A render or behaviour test if the key changes pixels or input.

Unknown keys must keep falling back one field at a time. A typo in the accent
colour cannot take the login form down with it.

## Theme changes

A theme is a bundle under `examples/themes/<id>/`, not a loose TOML file:

- `theme.toml` has the id, name, version, description, and optional preview.
- `config.toml` is a complete config.
- Every asset lives inside the bundle and every path in the config is relative.
- `layout.save` must stay `false`; bundles are read-only data.
- Do not add `[theme].package` inside a bundle.

After editing one, run:

```sh
dawn add examples/themes/<id> --force
dawn --theme <id> --check-config
dawn --theme <id> --screenshot /tmp/theme.png --size 1920x1080
```

`dawn add` validates the manifest, config, assets, symlinks, and size limits.
Do not bypass that validation with a manual copy in a test.

## Rendering and motion

- Screenshots use `Scene.started = frame_now`, so decor motion is captured at
  its neutral pose. Keep new animations neutral at `elapsed = 0`.
- Respect `[motion].reduced` and `[power].low_power`. New motion must freeze in
  both modes.
- Animated decor runs inside the cached UI overlay. Keep animated assets small
  and use `motion.decor_fps` for ambient movement.
- Static decor is painted before the form. Nothing decorative may cover an input
  or the submit button.
- Do not weaken a visual test to make a change pass. Fix the layout or the
  asset.

## Security

- Passwords never reach logs, plugin data, media metadata, or GPU uploads.
- Keep the password field masked and the secret in a zeroizing buffer.
- Do not add a network fetch to the login path.
- Wallpapers and plugin files are untrusted input. Keep size and allocation
  limits in place.
- A failed optional subsystem must fall back, not panic and not block auth.

## Pull requests

Include:

- what changed and why
- the user-visible behaviour
- the commands you ran and their results
- screenshots for visual changes
- known risks or follow-up work

Keep unrelated refactors out of a bug fix. One reviewable idea per change is
easier to trust.
