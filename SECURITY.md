# Security policy

## Supported versions

Security fixes land on the latest release line. At the time of writing that is
0.2.x. Older versions may receive a fix when the issue is trivial to backport,
but do not assume it.

## Reporting a vulnerability

Do not open a public issue with exploit details, credentials, or a working
bypass. Use the private security advisory feature of the repository host. If
that is not available, contact the maintainer through the address on the
project page and include only enough detail to establish the report.

Please include:

- affected version
- distribution, compositor, and GPU driver
- a minimal reproduction
- whether the issue is in dawn or in greetd, PAM, a driver, or ffmpeg
- logs with secrets removed

## Scope

In scope:

- authentication bypass or credential disclosure
- password or PAM secret appearing in logs, pixels, plugin data, or crash output
- a crash that blocks the login path or the TTY fallback
- unsafe file handling for wallpapers, decor, plugins, theme bundles, or layout state
- privilege issues in the install or greetd instructions
- a renderer or GPU path that can be made to read memory it should not

Out of scope:

- a compromised root account, greetd, PAM, compositor, or kernel
- physical access attacks against the machine
- a malicious user who can already write to `/etc/dawn` or the greeter binary
- screenshots that the user deliberately configured to show private media
- upstream bugs that are already documented and have a safe fallback

## Design promises

- The UI thread never blocks on PAM, the greetd socket, D-Bus, or image
  decoding.
- Plugins are declarative. They do not execute code and never receive input or
  credentials.
- Media metadata stays off the screen before authentication unless
  `media.show_before_login = true`.
- Theme bundles are data. They cannot execute code, contain symlinks, or point
  assets outside their own directory.
- A failed GPU, config, plugin, theme, or wallpaper path falls back without
  locking the user out.
- The terminal fallback remains available through greetd.

## Disclosure

Give the maintainer time to ship a fix before publishing details. Credit is
offered unless you prefer to stay anonymous.
