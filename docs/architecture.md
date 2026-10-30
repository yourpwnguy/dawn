# Architecture

dawn is a greetd greeter, not a desktop environment. The design goal is that a
person can always get to a login prompt, even when the pretty parts are broken.
That goal shapes the module boundaries and every failure path.

## Layers

```
main / cli / tty / ui::window
        |
     app (form state, key policy)
        |
  +-----+----------+-------------+
  |                |             |
  ui             plugin       config, session, theme
  |                |             |
  +---- auth <-----+             |
        |                       |
     greetd socket          filesystem
```

Dependencies point inward. `auth` does not know about pixels, `plugin` does not
know about credentials, and `config` and `session` are pure data producers. The
only module that sees every layer is `app`, and it still never reads a raw
greetd frame or a filesystem path at draw time.

## Authentication

`auth::client` owns the wire format: a native-endian `u32` length followed by
a UTF-8 JSON document, capped at 1 MiB. Requests are `create_session`,
`post_auth_message_response`, `start_session`, and `cancel_session`. The socket
has read and write timeouts, and any transport error drops the connection so
the next attempt starts clean.

`auth::flow` is a synchronous state machine with four states:

| State | Meaning | Accepted next step |
| --- | --- | --- |
| `Idle` | no attempt | `begin` |
| `AwaitingReply` | request in flight | reply, cancel, transport error |
| `AwaitingPrompt` | PAM asked a question | `answer`, cancel |
| `Starting` | `start_session` in flight | final success or error |

The machine validates every transition and returns an explicit `Effect`
(`Request`, `Ask`, `Completed`, `Failed`, `Cancelled`). A bug in the machine
becomes a visible failure and a reset, not a panic. Informational PAM messages
are acknowledged automatically with an empty response and capped at eight in a
row, so a broken PAM stack cannot make the greeter loop forever.

`auth::worker` runs the machine on a dedicated thread. The UI sends commands
through a channel and receives events through a callback, which the window
frontend adapts to winit's event-loop proxy. A blocking PAM module therefore
never freezes the window, and a closed window drops any queued secret.

`auth::secret` is the only credential type. It wraps a zeroizing string, has no
`Clone` or `Display`, prints a redaction marker in `Debug`, and exposes its
bytes only inside the crate. The password moves from the input, through the
worker channel, into the JSON serializer, and is wiped afterwards.

The TTY fallback drives the same `Flow` and `Backend` synchronously. It is a
different presentation, not a second auth implementation.

## Rendering

The window has two presentation paths with one pixel source. The UI is always
rasterised on the CPU with tiny-skia, and text is shaped and rasterised by
cosmic-text. The embedded Inter face guarantees readable Latin text on a minimal
boot image; system fonts cover other scripts.

The preferred path is a wgpu compositor. `ui::gpu` keeps the UI overlay in an
RGBA texture and composites it over a wallpaper texture with one full-screen
triangle. The UI texture is uploaded only when the overlay actually changes;
a video frame costs one wallpaper upload and the draw call, not a full tiny-skia
repaint and a softbuffer copy. Textures and the surface use sRGB 8-bit values
throughout (the surface must offer `Bgra8Unorm` or `Rgba8Unorm`), so the GPU
result matches the CPU path instead of round-tripping through linear space.
Presentation uses FIFO so the compositor paces the video.

The fallback is the original softbuffer path. It is selected when wgpu cannot
create an adapter, device, or surface, or when the surface only offers HDR or
sRGB formats. The failure is logged with its reason, the login form still
appears, and `DAWN_GPU=0` forces this path for driver debugging. A greeter must
never depend on a GPU driver to show a password prompt.

A frame is described by `ui::view::View`: already-safe strings, focus state,
masked password text, picker contents, an `authenticated` flag, and warnings.
`ui::render::Scene` combines that view with the resolved theme, widget list,
monitor name, time, motion frame, and current drag offsets.
`ui::render::paint_overlay` turns it into the UI layer and returns a `HitRegion`
for every drawable, which the window stores for pointer hit testing. There is no
retained widget tree, so a screenshot can be rendered without a window through
the exact same overlay code. `ui::render::paint` is the full CPU frame used by
screenshots and the software fallback.

Redraws happen on input, resize, a visible clock or media change, a wallpaper
frame, and a motion frame. A one-second tick recomputes a signature over every
widget's visible value; a minute-resolution clock therefore does not request a
frame sixty times a minute. An idle static screen waits on the clock boundary.
The GPU path tracks UI dirtiness separately: a wallpaper frame only uploads the
wallpaper, while input, auth changes, motion, and hover effects mark the overlay
dirty. The glass cache is the one exception; when a new blurred backdrop
arrives, the overlay is re-rasterised because the panel pixels themselves
changed.

Animated decor is ambient rather than input-driven, so it gets its own cadence.
`[motion].decor_fps` caps how often a moving ornament forces a fresh overlay
frame; boot, focus, typing, and button hover still run at the full motion rate.
Decor images are sampled directly through one transform (`Canvas::image_transformed`)
instead of being copied into a scratch pixmap first, and a 1:1 blit uses nearest
sampling because it lands exactly on the source grid. This keeps a handful of
drifting petals affordable while the wallpaper itself stays at video rate.

Plugin widgets are painted first, then the built-in clock and media widgets,
then the login card, then overlays. This ordering is a security property: no
plugin can cover the password field. The built-in widgets are ordinary
`Widget` values, not a separate code path.

Font sizes and offsets are in logical pixels and multiplied by the output scale
factor at draw time. The screenshot path uses a scale of 1. `ui::text::Style`
also carries a weight and letter spacing, which is what the hero submit button
and themed headings use.

The login card has two layouts. `card` is the content-sized glass panel.
`hero` is a fixed logical size that is uniformly scaled down when the display
cannot fit it, so a website-style clone still leaves the form reachable on a
small laptop panel. The hero surface is painted directly instead of through the
glass cache: a configurable panel colour fades in from one side over the
wallpaper, and `[login.decor]` images and text are painted inside the card
before the fields. Decor is decoded once at startup and is never interactive.
The submit button is a normal key path: a pointer click synthesises Enter, so
the keyboard and pointer obey the same policy. Its hover colour, gradient,
reveal direction, scale/lift, glow, and effect are configuration rather than a
second button implementation.

Rotation is a first-class property. The renderer paints a rotated item into a
bounded transparent layer and composites it back with `Canvas::draw_layer`, so
text, icons, images, brackets, and effects rotate together. A rotated card can
contain rotated fields and a rotated button; their hit regions carry the sum of
both angles and inverse-rotate the pointer instead of using a loose bounding
box. A fully transparent card is `surface = "none"`: it skips the panel, border,
and shadow while keeping the form and its hit regions.

## Background engine

`ui::background` owns the placeholder, the decoded frame set, frame timing, and
the crossfade. Decoding runs in `ui::image` on a dedicated loader thread. The
window asks for a load when it is first mapped and after a resize, and a stale
generation is ignored when the result arrives. The placeholder is painted from
the first frame, so a slow or broken wallpaper never delays the login form.

Still images (PNG, JPEG, WebP) are decoded once on the loader thread and
pre-fitted to the output size. Animated GIF and WebP use `ui::stream` instead:
a decoder thread publishes one frame at a time and sleeps for that frame's
delay, so a long 1080p animation keeps only the current frame in memory. Video
uses `ui::video`: it spawns the system `ffmpeg`, pipes raw RGBA frames, and
publishes them through the same `LiveSource` interface. ffmpeg runs with `-re`
and `-fps_mode passthrough`, so it follows the source timestamps instead of
being decoded in a burst or resampled to a fixed rate. A published frame calls
the stream's waker, which sends an event-loop wakeup; the UI paints the newest
frame immediately instead of polling on a 16 ms timer. A slow decode shows the
previous frame rather than blocking the login form. `background.fps = 0` means
native timing; a positive value is still a cap. The view pauses when the window
loses focus or after `background.idle_pause_secs`, and freezes on one frame in
low-power or reduced-motion mode. A paused video simply stops reading the pipe,
which back-pressures ffmpeg and costs no decode work.

The view exposes a `BackgroundSnapshot` with shared `Arc<Pixmap>` current and
previous frames plus the crossfade weight. Sharing is what makes the GPU path
cheap: taking a snapshot at video rate never copies a frame, and the uploader
compares the `Arc` identity to skip unchanged stills. `paint_snapshot` draws the
same snapshot without mutating crossfade state, so the glass source and the GPU
compositor cannot disagree about which frame is current. A unit test compares
`paint` and `paint_snapshot` for placeholders, solid, gradient, animated, and
crossfade cases.

The ffmpeg process still performs the actual decode and pixel-format conversion
on the CPU. The GPU path removes the full-screen UI compositing and presentation
cost, not the codec work; hardware decode is deliberately not a requirement for
a greeter that may start before the user's GPU session is ready.

## Glass and widgets

`ui::glass` ports Hyprland's `decoration:blur` to the CPU. It prepares the
background with contrast and brightness, runs a dual Kawase pyramid (`blur1`
five-tap downsample, `blur2` eight-tap tent upsample), then finishes with grain
and brightness. The work happens on a reduced copy of the background, and
panels clip the cached texture and layer tint, border, shadow, and optional
noise. The first blur is synchronous so the first painted frame has its backdrop;
later refreshes run on a `dawn-blur` worker and are picked up on a following
frame, so a video wallpaper does not drop a frame every time the backdrop
updates. The cache refreshes at `theme.blur.refresh`, which is much slower than
the wallpaper frame rate. The result is the same vocabulary and visual
behaviour as the compositor: `size`, `passes`, `noise`, `contrast`,
`brightness`, `vibrancy`, and `vibrancy_darkness`.

The GPU compositor still uses this cache. It paints a background snapshot into
the blur source only at the configured refresh cadence, and re-rasterises the UI
overlay when `prepare` reports that a new blurred pixmap was applied. The blur
therefore keeps its cost and its look; only the wallpaper compositing moved to
the GPU.

`ui::theme` resolves config tokens into a palette and adjusts text colours that
would miss the contrast target. Clock and media text sit on bare wallpaper, so
the renderer paints a theme-tinted scrim behind each of them. `ui::motion` owns
the boot slide and fade, focus settle, error shake, and the drag easing time
constant, all driven by `Instant`.

`ui::drag` owns pointer dragging. The renderer returns a hit region per widget;
the window resolves a press to the topmost region, starts a drag only when that
region is marked draggable, and eases the offset toward its target on every
frame. Regions are returned in paint order, so a non-draggable field on top of a
draggable card wins the hit test. `[layout] save = true` writes the targets to a
small versioned TOML file. Dragging never blocks input and never touches auth.

`ui::media` polls MPRIS on its own thread. Metadata and album art live in a
shared `MediaState`; the UI reads a snapshot. Album art goes through the same
bounded decode and downscale path as wallpapers, and is cached until the art
URI changes. When the player disappears, `keep_last` turns the snapshot into a
`Last played` state instead of blanking the widget. `show_before_login` controls
whether that snapshot is drawn before `View::authenticated`; the default is on
for single-user machines and can be turned off on shared ones.

## Theme store

`theme` owns the bundle format and the on-disk stores. A DSB-2 bundle is a
directory with `theme.toml`, a complete `config.toml`, `preview.webp`, and an
`assets/` root. `theme::spec` is the single validator behind both `dawn check`
and `dawn add`; it enforces the numbered rules in `docs/bundle-spec.md`,
including asset references confined to `assets/`, full config validation, a
known manifest revision, and at least one author. `dawn add` validates before
copying, copies into a staging directory, validates the staged tree again, and
only then renames it into place, with the previous bundle moved aside for a
rollback if the rename fails. Revision 1 trees still install with one warning;
`dawn add --migrate` upgrades them through a validated sibling copy.

Lookup checks `/usr/share/dawn/themes`, then any `DAWN_THEME_DIRS` entries, and
only then the deprecated user store. System stores always win, and a user-store
fallback still loads but logs a warning with the sudo command that reinstalls
the bundle. `dawn add` writes to the system store by default; `--user` opts
back into the XDG store. `--theme ID` asks `theme` for the directory and loads
its `config.toml` with package resolution disabled. A config can also contain
`[theme].package = "ID"`; that file is a pointer and the bundle config replaces
it wholesale. This keeps a bundle portable: the same directory is the config
and the assets, wherever it is copied.

## Failure strategy

| Failure | Behaviour |
| --- | --- |
| Config file missing | Built-in defaults plus one `sudo dawn init` hint |
| One bad config field | That field falls back; the rest applies; a warning is shown |
| Unreadable config file | Defaults plus a warning |
| Broken plugin | Logged and skipped |
| Broken background image | Colour or gradient instead, with a named warning |
| Wallpaper exceeds the memory cap | A single oversized frame is downscaled; the warning names the cap |
| No MPRIS player or no session bus | Calm `No media` state; login is unaffected |
| `ffmpeg` missing for a video wallpaper | Gradient fallback plus a warning naming the file; login is unaffected |
| Corrupt saved layout | Ignored; widgets return to their configured anchors |
| No sessions | Visible error on the login form; auth is not attempted |
| greetd transport error | Visible error, flow reset, next attempt reconnects |
| Worker thread gone | Visible error instead of a permanent spinner |
| Window creation or drawing failure | Terminal fallback when `/dev/tty` is usable |

The config warning is painted at the bottom of the screen so it cannot push the
password field around. Full details go to the log, and `--check-config` prints
them with file, line, and column.

## Security boundaries

- PAM and greetd text is untrusted. Control characters are replaced, whitespace
  is squeezed, and the result is capped before it reaches layout.
- Plugin manifests are strict: unknown keys and unknown permissions fail the
  plugin rather than being ignored.
- Plugin files are read only after a grant check and a canonicalisation check
  that rejects symlink escapes.
- All decoders have size and pixel limits before allocation. Wallpapers and
  album art share that path; plugin images keep their own PNG-only path.
- The media widget can be configured to hide metadata until
  `View::authenticated` is true (`media.show_before_login = false`). The default
  is visible because a single-user greeter is the common case; shared machines
  should opt out.
- The saved layout file contains only widget offsets and ids. It never contains
  credentials, usernames, media metadata, or input text.
- The runtime forbids `unsafe`, and clippy denies `unwrap` and `expect` outside
  tests.

## Testing

The state machine, config validator, session parser, plugin loader, input
editor, and app interaction model all have unit tests that do not need a
display. The greetd client is tested against a real Unix socket with a scripted
server. The graphical path is exercised by `--screenshot` and by a short
Xvfb plus xdotool smoke test during development:

```sh
dawn --dry-run
# type a username, Tab, a password, Enter, then Enter again for the prompt
```
