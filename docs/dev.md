# Theme development with `dawn dev`

`dawn dev` is a windowed, dry-run greeter with a file watcher. It exists so a
theme author can edit a config or a bundle and see the result without logging
out or rebooting.

```sh
dawn dev examples/themes/kinich
dawn dev examples/themes/kinich/config.toml
dawn dev --theme kinich --size 1600x900
dawn dev --no-hud
```

`PATH` may be a `config.toml` or a DSB bundle directory. A directory is checked
with the same validator as `dawn check` first; the revision and every rule issue
are printed, and an invalid tree exits nonzero before a window opens. With no
`PATH` and no `--theme`, the normal discovery order applies (`--config`,
`DAWN_CONFIG`, then `/etc/dawn/config.toml`).

`--theme ID` resolves an installed bundle exactly like `--theme` on a normal
run. The window title is `dawn dev <bundle id or config path>`.

## Keys

| Key | Action |
| --- | --- |
| `F5` | Reload now, even if no file changed. |
| `F1` | Show or hide the HUD. `--no-hud` starts hidden and keeps F1 from showing it. |
| `Ctrl+Q` | Quit the studio. |
| `Escape` | The normal login key: clear the focused field. Dev does not steal it. |

Everything else is the real login form. Authentication is simulated, so any
non-empty password works, but no session is ever started.

## HUD

The top-left corner shows `DEV`, the source (a bundle id or config path), the
last reload cost in milliseconds, an fps estimate, and
`F5 reload · F1 hud · Ctrl+Q quit`. A failed reload replaces the cost line with
a crimson `reload failed: <first line>` and the last good scene stays on screen.
The HUD is only constructed by `dawn dev`; it never appears in a normal run or
in `--screenshot` output.

## Reload guarantees

- A reload is triggered by a settled burst of filesystem events. Bursts within
  90 ms are coalesced into one reload, which handles editors that write a
  temporary file and rename it into place.
- The config is re-read, parsed, and validated. If it has any issue, the window
  keeps the last good theme and reports the first line. The process does not
  exit and the terminal is not disturbed.
- Only one warning line is logged per distinct error. A later valid reload
  clears the error.
- Wallpaper work is skipped unless the `[background]` section actually changed.
  A different image path is the only reason to decode pixels again; an
  fps-only edit reuses the loaded frame set.
- The text engine and font database are kept unless `[ui].font` or
  `[ui].font_size` changed.
- Decoding, blur, and video playback stay on the existing background workers,
  so a slow wallpaper never stalls the event loop.

## Why it never touches greetd

Dev mode is built through the same window and dry-run path as `dawn --dry-run`,
but it has no `--dry-run` flag to set incorrectly. The window entry point for
the studio constructs the auth mode as `DryRun` directly; there is no branch
that can build a greetd mode from the config. The studio also forces a windowed
surface and never asks for fullscreen, even when the bundle does.

File watching is best effort. `notify` is preferred; if it cannot be created or
watched, a 120 ms modification-time poller takes over silently. A watcher error,
a missing file, or a malformed edit can only change what the window paints.
