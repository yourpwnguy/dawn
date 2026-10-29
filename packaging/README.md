# dawn packaging helpers

These files are reference configs, not an installer. A greeter owns
`/etc/greetd/config.toml`, so the admin should choose the session command that
fits the machine. Installation is manual and binary-only; see
`docs/installation.md` for the display manager switch and rollback.

## Recommended: cage on Wayland

```sh
sudo install -Dm644 packaging/greetd-cage.toml /etc/greetd/config.toml
```

That file runs dawn inside a kiosk cage session. The `greeter` user needs
access to the seat, which cage and greetd arrange together.

## X11 fallback

`greetd-x11.toml` is a starting point for machines that cannot run cage. X11
needs a window manager or a session that focuses the greeter window; a bare X
server often leaves the new window unfocused. For a headless check, `xvfb` plus
`--dry-run` is easier than a real X11 greeter.

## Files

- `greetd-cage.toml`: cage and dawn, the normal setup.
- `greetd-x11.toml`: xinit and dawn, only when Wayland is not an option.

See `docs/installation.md` for dependencies, paths, and verification.
