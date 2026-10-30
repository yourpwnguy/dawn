# Troubleshooting

Start with the log. A greeter that fails before the window appears usually says
why in `journalctl`:

```sh
journalctl -u greetd -b --no-pager
```

The terminal fallback is still available through greetd. Keep a root shell open
while changing a greeter.

## The screen is black or the greeter never appears

- Check that greetd is running: `systemctl status greetd`.
- Check the command in `/etc/greetd/config.toml`. `cage -s -- /usr/local/bin/dawn`
  needs both binaries on the `greeter` user's `PATH`.
- Run `dawn --dry-run` from a normal session. If that works, the problem is the
  greetd or cage setup, not the binary.
- Try `cage -- /usr/local/bin/dawn` manually from a VT if the service command is
  unclear.
- Look for a missing library with `ldd /usr/local/bin/dawn`.
- If the GPU path is the suspect, run `DAWN_GPU=0 dawn --dry-run`.

## Wrong GPU or a black screen with the GPU compositor

Run `dawn --list-gpus` to see the adapters this config can address. The table
prints an index, name, backend, device class, driver, and marks the selection.
`index:N` and name substrings match that list, so start with:

```toml
[gpu]
adapter = "..."
```

Some machines expose both a discrete and an integrated adapter. Force the one
you want with `device = "discrete"` (or `"integrated"`), and try
`backend = "gl"` when a Vulkan driver enumerates but renders incorrectly. If
the GPU path still fails, `fallback = "cpu"` uses softbuffer immediately
instead of spending time on adapter retries. `DAWN_GPU=0` remains a one-off
diagnostic equivalent to `enabled = false`.

## The window opens but the keyboard does not work

- Under X11, a bare X server does not always focus the new window. Use a small
  window manager, or use cage on Wayland.
- Under Wayland, confirm the greeter is inside the kiosk compositor, not a
  nested desktop session.
- `dawn` grabs keyboard focus on its own window. If another window is on top,
  the login will not receive keys.

## Authentication fails

- Check `journalctl -u greetd -b` for the PAM message.
- Test with `dawn --dry-run` to separate UI problems from auth problems.
- Confirm the greeter is talking to greetd through `GREETD_SOCK`. Do not set
  `GREETD_SOCK` yourself unless you are using a fake greetd for development.
- Verify the target user and session with a normal terminal login.
- If every login fails after an upgrade, check `/etc/pam.d/greetd` and the
  installed greetd package before changing dawn.

## The config check reports problems

```sh
dawn --config /etc/dawn/config.toml --check-config
```

Each issue names the dotted key and source location. Unknown keys, wrong types,
and out-of-range values fall back one field at a time. The login still works
with the fallback values.

Common causes:

- A relative path resolves from the config file, not from the current directory.
- A wallpaper or decor image is missing or unreadable by the `greeter` user.
- A plugin directory contains a broken `plugin.toml`.
- A field is outside its documented range, for example a width above 2400.
- A font family name is misspelled. This is a warning, not a fatal error.

## The video wallpaper is black or frozen

- Confirm `ffmpeg` exists for the greeter user:
  `sudo -u greeter env PATH=/usr/local/bin:/usr/bin:/bin ffmpeg -version`.
- Check that the codec is supported by the installed `ffmpeg`.
- Raise `background.memory_limit_mb` if a 4K clip is downscaled or rejected.
- Set `background.fps = 30` to cap a very heavy source.
- Remember that `idle_pause_secs` freezes the wallpaper after no input. Set it
  to `0` for a permanently animated login.
- `[power].low_power = true` freezes the wallpaper on one frame by design.
- A missing or corrupt clip falls back to the gradient and names the file in
  the log.

## GIF or WebP wallpaper does not animate

- `background.kind = "image"` shows one frame. Use `auto`, `animated`, or
  `video` for animation.
- An animated file larger than the memory budget is downscaled or rejected.
- A still WebP is intentionally treated as a still image.

## A theme is missing or will not install

- `dawn list` shows the stores dawn will read. A theme must be a directory with
  `theme.toml` and `config.toml`.
- A theme installed in your user store is not readable by the `greeter` user.
  Install it with `sudo dawn add --system PATH`.
- `dawn add` refuses absolute asset paths, `..`, symlinks, `layout.save = true`,
  and nested `[theme].package` pointers. Those rules keep a bundle portable.
- If the theme lives outside the normal stores on an immutable system, set
  `DAWN_THEME_DIRS` to a colon-separated list of directories.
- `dawn --theme ID --check-config` prints the exact key or asset that failed.

## Fonts look wrong or text is missing

- The embedded Inter face is always available for Latin text.
- Showcase configs name optional families such as Noto Serif or Plus Jakarta
  Sans. Install the family or accept the fallback.
- Check the family name with `fc-list : family`.
- Very large text can be clipped by a short hero card. Raise
  `login.width` or `login.height`, or reduce the display size.

## High CPU use

- Lower `motion.decor_fps` when animated ornaments are active.
- Set `background.fps = 30` or `60` instead of `0` for a very high frame rate
  source.
- Turn off `[theme.blur]` if you do not need it.
- Set `[power].low_power = true` on weak or battery-powered hardware.
- `DAWN_GPU=0` is a diagnostic fallback, not a CPU saving. The GPU path is
  normally cheaper for video.

## The wallpaper has bars or is cropped

- `fit = "cover"` fills the screen and crops.
- `fit = "contain"` shows the whole image and leaves bars.
- `fit = "fill"` stretches.
- Check per-output overrides under `[background.monitors.NAME]`; a matching
  output name wins over the base settings.

## The greeter shows a warning

A warning means a config key, plugin, or wallpaper could not be used and dawn
fell back. The login remains usable. Run `--check-config` and read the named
key. Warnings are intentionally short on screen so they cannot cover the form.

## Reporting a problem

Include:

- dawn version: `dawn --version`
- distribution and version
- compositor or X11 setup
- `dawn --config PATH --check-config` output
- the relevant `journalctl -u greetd` lines
- GPU vendor and driver version if the issue is graphical
- whether `DAWN_GPU=0` changes the result

Never include a password, a PAM conversation secret, or a full config that
contains private paths you do not want public. See `SECURITY.md` for sensitive
reports.
