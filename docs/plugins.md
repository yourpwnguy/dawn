# Plugin API v1

A plugin is a directory under one of the configured plugin directories. It
contains a `plugin.toml` and, optionally, PNG files. Plugins are declarative:
they describe widgets and the greeter paints them. There is no code execution,
no scripting host, no event hook, and no shared memory with the login form.

This is the whole sandbox, and it is deliberately boring. A plugin never sees a
keystroke, the username, the password, or the greetd conversation. All plugin
pixels are drawn before the login card, so a widget cannot cover the form even
if it is huge. A broken or hostile plugin can only waste screen space.

The current API version is `1`. A manifest with another version is rejected
with a clear log line; it is not guessed at.

The built-in clock and media widgets use this same widget model. Their config
lives in `[clock]` and `[media]`, but they end up as ordinary `Widget` values
with the same placement and rendering path as a plugin widget.

## Directory layout

```
/usr/share/dawn/plugins/hello-world/
  plugin.toml
  assets/logo.png        # optional, must be granted below
```

The directory name is the plugin id. It may contain ASCII letters, digits,
`_`, and `-`, up to 64 characters. If two configured directories contain the
same id, the earlier directory wins and the duplicate is reported.

`plugin.toml` must be a regular file no larger than 64 KiB.

## Manifest

```toml
api_version = 1
name = "Hello World"
version = "1.0.0"
description = "A greeting, a clock, and a media widget."

[permissions]
filesystem = ["assets"]

[[widget]]
kind = "text"
text = "Welcome"
size = 20
color = "#8b949e"
anchor = "top-center"
offset = [0, 28]

[[widget]]
kind = "clock"
format = "%H:%M"
size = 14
color = "#6e7681"
anchor = "bottom-right"
offset = [28, 24]

[[widget]]
kind = "media"
size = 16
anchor = "bottom-left"
offset = [28, 24]
```

### Top level

| Key | Required | Notes |
| --- | --- | --- |
| `api_version` | yes | Must be `1`. |
| `name` | yes | 1 to 64 characters. |
| `version` | no | Up to 32 characters, defaults to `"unspecified"`. |
| `description` | no | Up to 200 characters. |
| `permissions` | no | See below. |
| `widget` | no | Array of tables, at most 16. |

Unknown keys are an error. That is intentional: a typo should be visible in the
log rather than silently doing nothing.

### Permissions

v1 has exactly one permission: `filesystem`. It is a list of plugin-relative
files or directories that `image` widgets may read.

```toml
[permissions]
filesystem = ["assets", "logo.png"]
```

The default is deny. A path must be relative, must not use `..`, and must
canonicalise to a path still inside the plugin directory, which also blocks
symlink escapes. Grants are simple prefix matches on whole path components:
`assets` covers `assets/logo.png` but not `assets2/logo.png`. At most 8 grants
are accepted.

### Widgets

Every widget has `kind`, `anchor`, and `offset`. Widgets may also name a
`monitor` to restrict them to one output, and `rotation` to rotate the widget
around its own centre in degrees (-360 to 360).

| `anchor` | Position |
| --- | --- |
| `top-left` | screen top-left plus offset |
| `top-center` | centred horizontally at the top |
| `top-right` | top-right minus offset |
| `bottom-left` | bottom-left plus offset |
| `bottom-center` | centred horizontally at the bottom |
| `bottom-right` | bottom-right minus offset, the default |

`offset` is `[x, y]` in logical pixels, each between -2048 and 4096. The
default is `[24, 24]`. Offsets are multiplied by the output scale factor at
draw time. A missing `monitor` means every output; an unknown output name in a
preview shows the widget rather than hiding it.

#### Text

```toml
[[widget]]
kind = "text"
text = "Welcome"
size = 20          # 8 to 160, default 16
color = "#8b949e"  # defaults to the theme's muted colour
max_width = 400    # optional, 40 to 2000; enables wrapping
anchor = "top-center"
offset = [0, 28]
```

`text` is required, 1 to 200 characters, and control characters are rejected.
Without `max_width` the text is measured as one line.

#### Clock

```toml
[[widget]]
kind = "clock"
style = "digital"    # or "analog", default digital
format = "%H:%M:%S"  # optional, default "%H:%M"
date_format = "%A, %B %d"
hour12 = false
seconds = true
timezone = "UTC"
posix_locale = false
size = 14            # analog uses this as the dial diameter
date_size = 12
color = "#6e7681"
date_color = "#4d5566"
anchor = "bottom-right"
offset = [28, 24]
```

The format is validated at load time. A time zone name must resolve at load
time. The greeter redraws when the displayed value changes, not once per second
unconditionally. A plugin clock and the built-in clock share the same
`ClockSpec`, so their formatting rules cannot drift apart.

#### Media

```toml
[[widget]]
kind = "media"
size = 16                    # 8 to 48, default 16
show_art = true              # local file:// album art only
show_before_login = true     # false hides metadata until authentication
color = "#f2f6ff"            # optional title colour
anchor = "bottom-left"
offset = [28, 24]
```

The host owns the MPRIS connection, so there is no manifest field for a player
name. The widget shows `No media` when no player exists, and it keeps the last
track it saw if the player disappears. `show_before_login` mirrors
`media.show_before_login` in the main config: the default draws the last track
before authentication, and a shared machine should set it to `false`. The
manifest media widget is not draggable; drag is a first-party config feature.

#### Image

```toml
[[widget]]
kind = "image"
path = "assets/logo.png"
width = 64         # 8 to 512, default 64
```

Only PNG is accepted in v1. The aspect ratio is preserved. The file must be
covered by a `filesystem` grant, be at most 8 MiB, at most 2048 pixels on either
side, and at most 4 megapixels decoded. Both the PNG header and the decoded
pixmap are checked so a malicious header cannot force a large allocation.

## Failure behaviour

- A widget with an invalid value is skipped and logged with its index. The rest
  of the plugin still loads.
- A structurally invalid manifest, an unsupported API version, or an unreadable
  `plugin.toml` disables that plugin.
- A missing image or a denied path skips that widget.
- A media widget with no player or no session bus stays in its idle state and
  never affects the login card.
- No plugin failure can stop the greeter or change the login form.

`dawn --check-config` lists built-in widgets, loaded plugins, their widget
counts, and every plugin error without starting a window.

## Versioning

The API is versioned so a future format can be rejected rather than
misinterpreted. v1 plugins are expected to keep working within the v1 schema;
new optional keys may be added, but existing keys will not change meaning
without a version bump. The `media` widget is an additive v1 change.
