# dawn configuration

A dawn config is one TOML file of plain data. It describes where the login card sits, what it says, which widgets are visible, how the wallpaper is drawn, and which colours the renderer uses. There is no scripting layer and no include mechanism.

dawn looks for the file in exactly this order:

1. `--config PATH`
2. the path named by `DAWN_CONFIG`
3. `/etc/dawn/config.toml`

There is no per-user XDG config search. A greeter runs as its own user with no
useful `HOME`, so a config under `$XDG_CONFIG_HOME` would not be the file the
login screen reads. `dawn init` writes the system file; `dawn init --config
PATH` writes an explicit file for a test or a custom install.

The first path that exists and is a regular file wins. If none exists, dawn runs
on the built-in defaults, which are the values listed in every table below, and
prints one hint line: `no machine config; create it: sudo dawn init`. Relative
paths inside the file resolve against the directory that contains the config
file, and a leading `~/` expands through `HOME`.

Bad config never stops a login. Each field is validated on its own: a typo, a wrong type, or an out-of-range number makes that one field fall back to its default, every other field still applies, and the login screen shows a short warning such as `Config warning: ui.font_size: must be between 10 and 72 (got 5) (+1 more, see the log)`. A file that cannot be parsed at all falls back to the complete built-in defaults. Run `dawn --config PATH --check-config` to print the exact key, file, line, and column for every problem.

## 5-minute starter

This is a complete config. Every line is optional, but the comments explain what each line changes. It uses no external files, so it is safe to paste into an empty file and run.

```toml
# The base font. An unknown family falls back to the embedded Inter face.
[ui]
font = "Inter"            # family name; the embedded Inter is the final fallback
font_size = 17            # base logical pixel size, 10 to 72

# A flat wallpaper. No image file, no video, no codec dependency.
[background]
kind = "solid"            # auto, solid, gradient, image, animated, or video
color = "#0b1020"         # deep navy fill; alpha is allowed but a wallpaper is usually opaque
gradient = false          # false keeps the fill perfectly flat

# The compact login card.
[login]
width = 380               # card width in logical pixels, 260 to 2400
greeting = "Welcome back" # one line above the fields; an empty string hides it
username.placeholder = "Username" # text inside the empty username field
password.placeholder = "Password" # text inside the empty password field

# A small clock above the card.
[clock]
enabled = true            # draw the clock at all
time_format = "%H:%M"     # 24-hour hours and minutes

# The default dark glass palette.
[theme]
preset = "glass-dark"     # glass-dark, glass-light, or minimal
```

## `[ui]`

This section controls the global font and two window-wide legacy switches.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `font` | string, at most 64 characters | `"Inter"` | Family name passed to the text engine. An unknown family quietly falls back to the embedded Inter face, so text stays readable. |
| `font_size` | number, 10 to 72 | `17` | Base logical pixel size. Fields, labels, hints, and widgets derive their own sizes from this when their own size is zero. |
| `card_width` | number, 260 to 900 | `380` | Legacy default for `login.width`. It only matters when `login.width` is not set. |
| `fullscreen` | bool | `true` | Ask the compositor for a fullscreen window. `--fullscreen` forces it on. |
| `greeting` | string, at most 64 characters | `""` | Legacy default for `login.greeting`. It only matters when `login.greeting` is not set. |

The font family name is matched by fontconfig. If a theme names a display face that is not installed, the embedded Inter face is used instead. See [installation.md](installation.md) for the font packages dawn expects and [troubleshooting.md](troubleshooting.md) when fonts look wrong or text is missing.

When it goes wrong: `font_size = 5` reports `ui.font_size: must be between 10 and 72 (got 5)` and keeps `17`. A non-string font reports `expected a font family name` and keeps `Inter`. A misspelled family is not a config error; the text engine falls back at render time.

## `[background]`

This section controls the wallpaper: a solid colour, a derived gradient, a still image, an animated GIF or WebP, or a video decoded through `ffmpeg`.

### Video wallpaper that fits any monitor

This is the shape used by the showcase bundles. `cover` keeps the aspect ratio and crops the overflow; `contain` shows the whole frame and fills the spare area with `color`; `fill` stretches. `fps = 0` follows the source clock, and `idle_pause_secs = 0` keeps the clip running after the pointer stops.

```toml
[background]
kind = "video"            # "animated" means the same thing
image = "wallpaper.mp4"   # relative to this config file
fit = "cover"             # crop to fill the screen without distorting
fps = 0                   # 0 follows the source; 24 or 30 caps a heavy clip
memory_limit_mb = 256     # per-frame decode budget
idle_pause_secs = 0       # never freeze on idle
color = "#0b1020"         # shows through letterbox bars and during load
```

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `kind` | enum: `auto`, `solid`, `gradient`, `image`, `animated`, `video` | `"auto"` | Selects the wallpaper path. `auto` detects from the file: a multi-frame GIF or WebP is animated, any other image is still, and no image uses the colour or gradient. `video` is an alias for `animated` that states intent. |
| `color` | colour, `#rgb`, `#rgba`, `#rrggbb`, or `#rrggbbaa` | preset background colour (`#0b1020` for glass-dark) | Base colour, fallback colour, and the colour behind letterbox bars. The preset supplies a light value for glass-light and a near-black value for minimal. |
| `gradient` | bool | `true` | Draw a subtle vertical gradient derived from `color`. Set to `false` for a perfectly flat fill. |
| `image` | path, at most 1024 characters | unset | PNG, JPEG, WebP, GIF, animated WebP, or a video container: MP4, M4V, MOV, WebM, MKV, AVI, MPG, or MPEG. Relative paths resolve against the config directory. |
| `fit` | enum: `cover`, `contain`, `fill` | `"cover"` | How the decoded frame meets the screen. `cover` crops, `contain` letterboxes, `fill` stretches and distorts. |
| `fps` | integer, 0 to 240 | `0` | Upper bound on animated playback. `0` follows the source exactly. A positive value caps GIF and WebP delays and gives video an `ffmpeg` `fps` filter, which is useful on a slow panel or a heavy 4K clip. |
| `memory_limit_mb` | integer, 16 to 1024 | `128` | Hard per-frame decode budget in MiB. Animated GIF, WebP, and video stream one frame at a time; a single frame larger than the budget is downscaled before it is published. |
| `idle_pause_secs` | integer, 0 to 3600 | `45` | Freeze an animated wallpaper after this many seconds without input. `0` disables the idle pause. `[power].low_power = true` freezes it regardless. |
| `monitors` | table with at most 16 entries | empty | Per-output overrides. The output name comes from the compositor and must be 1 to 64 printable characters. |

Still wallpapers are decoded once on a loader thread. Animated GIF and WebP keep only the current frame, so a long 1080p animation does not fill memory. Video asks the system `ffmpeg` for raw RGBA frames on a pipe; frames arrive on the source clock, and each published frame wakes the window directly. Install `ffmpeg` if a video wallpaper is used. When it is missing, dawn falls back to the gradient, logs a warning, and the login form is unaffected.

The window paints the gradient placeholder immediately and crossfades to the first frame. A missing, oversized, or corrupt file falls back to the gradient and shows a warning that names the file. `kind = "image"` on a video shows the first frame and treats it as a still.

Per-output overrides use the compositor output name:

```toml
[background.monitors.DP-1]
kind = "image"
image = "wallpapers/left.png"
fit = "contain"
color = "#0b1020"
```

The override table accepts `kind`, `color`, `image`, and `fit`. Unknown output names keep the base values. In this release the greeter creates one window, so the override that applies is the one for the output containing that window.

When it goes wrong: an unknown `kind` reports `unknown kind "vidoe"; use auto, solid, gradient, image, animated, or video` and keeps `auto`. An unknown `fit` reports `unknown fit "stretch"; use cover, contain, or fill` and keeps `cover`. A missing image reports `cannot read /path: No such file or directory (fix: correct background.image or make the file readable)` and the background falls back to the gradient. A still image above 64 MiB reports the MiB limit; a video above 8 GiB reports the video limit. `fps = 500` reports `must be between 0 and 240 (got 500)` and keeps `0`. See [troubleshooting.md](troubleshooting.md) for a black or frozen video.

## `[login]`

This section controls the login card, its position, its surface, and the text and behaviour of the form.

### Card login in six lines

This is the smallest useful login card. The dotted keys create the two field tables without separate headers.

```toml
[login]
width = 380               # card width, 260 to 2400
greeting = "Welcome back" # empty hides the greeting
username.placeholder = "Username"
password.placeholder = "Password"
```

### Glass card without a colour surprise

`login.panel` does not control a glass card. A glass surface samples the blurred wallpaper and then applies `[theme].tint`, so the alpha channel in `tint` is what decides how much wallpaper shows through. A fully opaque tint makes the blur invisible. The field fill is a separate value and falls back to `theme.tint` at alpha 110, so it stays lighter than the card by default.

```toml
[theme]
tint = "#101726cc"        # last two digits are alpha; cc is about 80 percent opaque
border = "#ffffff40"      # one-pixel edge highlight

[login]
surface = "glass"         # frosted panel; "flat" and "none" are the other surfaces
show_shadow = true
show_border = true

[login.field]
background = "#10172655"  # translucent field fill over the card
```

### Keyboard-only navigation text

The button is off by default, so the form is already keyboard-only. This recipe keeps that state and replaces the three built-in hint lines with shorter copy.

```toml
[login]
show_hints = true
show_picker = true
show_session = true

[login.button]
show = false              # Enter still submits; no pointer target is drawn

[login.hints]
username = "Enter  Continue    Tab  Password    Esc  Clear"
password = "Enter  Sign in     Tab  Session    Esc  Clear"
picker = "Up/Down  Choose    Enter  Select    Esc  Cancel"
```

### Layout modes

This subsection controls how the card frame is sized and where the form sits inside it.

`layout = "card"` is the compact, content-sized glass panel. Its height comes from the greeting, the fields, the status line, the session row, and the hint line.

`layout = "hero"` is a fixed-size panel. The form starts at `form_offset` from the card's top-left and uses `form_width` when it is positive. The panel can fade into the wallpaper on the left with `panel_fade`, and the whole frame scales down uniformly when it does not fit the screen.

`layout = "poster"` shares the hero geometry but says what the author wants: a full-bleed frame with no implied surface. It is normally paired with `surface = "none"`. The form is still positioned with `form_offset`.

```toml
# Compact card
[login]
greeting = "Welcome back"
width = 416
draggable = true

[login.username]
placeholder = "Username"
label = "Account"
```

```toml
# Wide hero panel
[login]
layout = "hero"
width = 1160
height = 663
panel = "#ff4655e6"
panel_fade = [0.38, 0.62]
tab = "Login"
form_offset = [610, 246]
form_width = 495
```

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `layout` | enum: `card`, `hero`, `poster` | `"card"` | Card is content-sized; hero and poster are fixed frames positioned by `form_offset`. |
| `anchor` | enum: `top-left`, `top-center`, `top-right`, `center`, `bottom-left`, `bottom-center`, `bottom-right` | `"center"` | Which point of the screen the card is placed against. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Logical pixel offset from the anchor. Positive x moves right, positive y moves down. |
| `monitor` | string, at most 64 characters | unset | Restrict the card to one output name. Unset shows it on every output. |
| `width` | number, 260 to 2400 | `380` (or `ui.card_width` when that legacy key is set) | Card width in logical pixels. The card is clamped so it always fits the screen. |
| `height` | number, 0 to 2000 | `0` | Fixed height. Zero derives the height from the content, which is what the card layout uses. Hero and poster use a positive value. |
| `padding` | number, 0 to 80 | `26` | Space between the card edge and the form. Hero and poster position the form with `form_offset` instead when needed. |
| `gap` | number, 0 to 48 | `14` | Vertical gap between the greeting, fields, status, session row, and hints. |
| `radius` | number, 0 to 64 | theme `corner_radius` (`18` in glass-dark and glass-light, `10` in minimal) | Card corner radius in logical pixels. |
| `panel` | colour | theme `tint` | Fill for `surface = "flat"` and for hero or poster panels, including a hero glass surface. A card-layout glass surface ignores it and uses `[theme].tint` through the blur. |
| `panel_fade` | `[start, end]`, each 0 to 1, start not after end | `[1.0, 1.0]` | Hero and poster horizontal fade as fractions of the card width. Before `start` the wallpaper shows through; after `end` the panel is fully opaque. A start of 1 or a start at or after the end draws the panel at full strength. |
| `border` | colour | theme `border` | Hero and poster outline. It is not used by the glass card, which draws its own edge. |
| `surface` | enum: `glass`, `flat` (alias `solid`), `none` (alias `transparent`) | `"glass"` | Glass is the blurred panel. Flat is a solid fill. None hides the panel, border, and shadow, so the fields float directly on the wallpaper. |
| `show_shadow` | bool | `true` | Draw the card shadow. Ignored by `surface = "none"`. |
| `show_border` | bool | `true` | Draw the card outline. Ignored by `surface = "none"`. |
| `rotation` | number, -360 to 360 | `0` | Rotate the whole card, including its form and decor, around its centre. |
| `draggable` | bool | `false` | Let the pointer move the whole card. A click on a field still focuses it. Off by default so a stray click while typing cannot move the form. |
| `greeting` | string, at most 128 characters | `""` | Line above the fields. Empty hides it. Falls back to `ui.greeting` when this key is absent. |
| `show_session` | bool | `true` | Convenience alias for `login.session.show`. The more specific `[login.session].show` wins when both are present. |
| `show_hints` | bool | `true` | Draw the keyboard hint line at the bottom of the form. |
| `show_picker` | bool | `true` | Allow the session picker to open. Disable it for a single-session kiosk. |
| `picker_title` | string, at most 128 characters | `"Choose a session"` | Picker heading. Empty hides it. |
| `busy_text` | string, at most 128 characters | `"Signing in…"` | Status text while authentication runs when `login.hints.busy` is empty. |
| `dry_run_badge` | string, at most 64 characters | `"DRY RUN"` | Badge text in `--dry-run`. |
| `show_dry_run_badge` | bool | `true` | Draw the dry-run badge. Set to `false` for a clean preview. |
| `no_sessions` | string, at most 160 characters | `"No desktop sessions found. Install one or set [sessions].dirs."` | Status line when session discovery returns nothing. |
| `tab` | string, at most 64 characters | `""` | Small top-left tab label, used by hero and poster layouts. Empty hides it. |
| `form_offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Where the form starts inside a hero or poster frame, relative to the card's top-left. |
| `form_width` | number, 0 to 2400 | `0` | Width of the credentials and button column in hero and poster layouts. Zero uses the card padding. |

The card layout clamps its width and position so it always fits the current screen. A hero or poster frame keeps its authored proportions and scales down as one unit when the display is smaller than `width` by `height`. Saved drag offsets are clamped separately, so a position saved on a large panel cannot push the form out of reach on a laptop.

When it goes wrong: `width = 3000` reports `login.width: must be between 260 and 2400 (got 3000)` and keeps `380`. `height = 2500` reports the range `0 to 2000`. An unknown layout reports `unknown layout "wide"; use card, hero, or poster` and keeps `card`. An unknown surface reports `unknown surface "blur"; use glass, flat, or none` and keeps `glass`. `panel_fade = [0.8, 0.2]` reports `fade start must not be after fade end` and keeps `[1.0, 1.0]`. A monitor name that does not match any output leaves the card on every output.

### `[login.field]`, `[login.username]`, `[login.password]`

This subsection controls the shared field style and the two credential fields.

`[login.field]` is the shared style. `[login.username]` and `[login.password]` inherit it and can override every key. A zero in `font_size`, `height`, `radius`, `padding`, or `caret_width` means "derive it from `ui.font_size`" or the field text size, which is what keeps a complete-looking config short.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `label` | string, at most 128 characters | `""` | Optional label above the field. Empty hides it. |
| `label_color` | colour | theme `muted` | Label colour. Useful when the label should carry an accent while the placeholder stays quiet. |
| `label_size` | number, 0 to 64 | `0` (derive `field_size * 0.8`) | Absolute label size in logical pixels. Zero keeps the historical 80 percent of the field size. |
| `label_tracking` | number, 0 to 0.5 | `0` | Label letter spacing in em. |
| `placeholder` | string, at most 128 characters | `"Username"` or `"Password"` | Text shown while the field is empty. |
| `font_size` | number, 0 to 96 | `0` (inherit `ui.font_size`) | Typed text and placeholder size in logical pixels. |
| `height` | number, 0 to 160 | `0` (derive `field_size * 2.5`) | Field height including padding. |
| `radius` | number, 0 to 64 | `0` (derive `field_size * 0.55`) | Field corner radius. |
| `padding` | number, 0 to 64 | `0` (derive `field_size * 0.9`) | Horizontal text inset. |
| `background` | colour | theme `tint` at alpha 110 | Field fill. |
| `background_end` | colour | unset | Second fill colour. When set, the fill becomes a horizontal gradient from `background` to this colour. |
| `border` | colour | theme `border` at alpha 28 | Unfocused border. Ignored when `underline` is set. |
| `underline` | colour | unset | Draw a rule at the bottom of the field instead of a full box. Focus thickens the rule and adds a soft underglow. |
| `underline_size` | number, 0 to 12 | `0` (derive 1 pixel) | Rule thickness in logical pixels. Focus multiplies the thickness by up to 2.2. |
| `bracket` | colour | unset (theme shadow at alpha 220 when `bracket_gap` is positive) | Broken targeting bracket around the field. `None` hides it. |
| `bracket_gap` | number, 0 to 64 | `0` | How far the bracket sits outside the field. Zero skips the bracket even when `bracket` is set. |
| `text` | colour | theme primary text | Typed text colour. |
| `placeholder_color` | colour | theme `muted` | Placeholder colour. |
| `focus` | colour | theme `focus_ring` | Focus border, rule, and glow colour. |
| `caret` | colour | theme `accent` | Caret colour. |
| `caret_width` | number, 0 to 16 | `0` (derive `bar = field_size * 0.12`, `block = field_size * 0.55`) | Caret width in logical pixels. |
| `caret_style` | enum: `bar`, `block` | `"bar"` | Bar is the thin text caret; block is a filled character cell. |
| `icon` | enum: `user`, `lock`, `none` (empty also means none) | `""` | Leading glyph inside the field. |
| `icon_size` | number, 0 to 64 | `0` (derive `field_size * 1.1`) | Glyph size. |
| `icon_gap` | number, 0 to 64 | `0` (derive `field_size * 0.55`) | Space between the glyph and the text. |
| `icon_color` | colour | theme primary text | Glyph colour. |
| `rotation` | number, -360 to 360 | `0` | Rotate the field, its label, and its bracket around the field centre. |
| `mask` | string, at most 128 characters | `""` for username, `"•"` for password | Password glyph. Only the password field uses it. The first character is repeated once per typed character. An empty string falls back to `•`; the username field ignores the key. |
| `key_pop_ms` | number, 0 to 2000 | `0` | One-shot keystroke pop for the newest character, in milliseconds. Zero is off and keeps the field pixel-identical. |
| `key_pop_scale` | number, 1.0 to 2.5 | `1.18` | Peak scale of the popping character. It eases back to 1.0 with an expo-out curve. |
| `key_pop_color` | colour | theme `accent` | Colour of the popping character. It blends back to the field text colour as the pop settles. |

The key pop splits the painted run at the last character boundary with `char_indices`, so a multibyte glyph is never cut in half. It works on the masked password run as well, which means the pop lands on the newest mask glyph rather than exposing the real character. A zero `key_pop_ms` skips all of it. The pop starts when the field is focused and a character has just landed; it never fires on the placeholder.

When it goes wrong: `font_size = 120` reports `login.username.font_size: must be between 0 and 96 (got 120)` and keeps `0`, which derives the size from `ui.font_size`. `caret_style = "underline"` reports `unknown caret style "underline"; use bar or block` and keeps `bar`. `icon = "mail"` reports `expected user, lock, or none` and keeps the previous icon. A bracket colour without a positive `bracket_gap` draws nothing; this is valid, not an error.

### `[login.button]`

This subsection controls the optional pointer-clickable submit button. The button sends the same Enter key as the keyboard. It is off by default, so a classic card stays keyboard-only.

```toml
[login.button]
show = true
text = "GET STARTED"
gap = 42
height = 48
background = "#ff4655"
background_end = "#c91f35"
gradient_angle = 90
text_color = "#0f1923"
bracket = "#0f1923"
bracket_gap = 12
bold = true
tracking = 0.08

# Hover and game-style effects
hover_background = "#ff6b77"
hover_background_end = "#ff4655"
hover_direction = "left"
hover_ms = 200
hover_scale = 1.03
hover_lift = 2
glow = "#ff4655"
glow_size = 16
effect = "sweep"
effect_color = "#ffffff"
effect_ms = 1600
effect_angle = 115
effect_strength = 0.22
```

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `show` | bool | `false` | Draw the button. |
| `text` | string, at most 64 characters | `"Sign in"` | Button copy. |
| `gap` | number, 0 to 160 | `16` | Space above the button. |
| `height` | number, 20 to 160 | `44` | Button height in logical pixels. |
| `radius` | number, 0 to 64 | `8` | Corner radius. |
| `font_size` | number, 0 to 96 | `0` (inherit `ui.font_size`) | Button text size. |
| `background` | colour | theme `accent` | Button fill. |
| `background_end` | colour | unset | Second gradient stop. When set, the fill becomes a gradient. |
| `gradient_angle` | number, -360 to 360 | `0` | Gradient direction in degrees. 0 is left to right, 90 top to bottom. |
| `text_color` | colour | theme primary text | Button text. |
| `bracket` | colour | unset | Broken bracket around the button. It only draws when `bracket_gap` is positive. |
| `bracket_gap` | number, 0 to 64 | `0` | Bracket offset outside the button. |
| `bold` | bool | `false` | Bold button text. |
| `tracking` | number, 0 to 0.5 | `0` | Letter spacing in em. |
| `hover_background` | colour | a lighter version of `background` | Fill after the pointer enters. |
| `hover_background_end` | colour | a lighter version of `background_end` | Second stop while hovered. |
| `hover_text_color` | colour | `text_color` | Text colour while hovered. |
| `hover_direction` | enum: `fade`, `left`, `right`, `up`, `down`, `center` | `"fade"` | Directional modes paint the hover fill in from that edge. |
| `hover_ms` | number, 0 to 5000 | `180` | Hover colour and geometry transition in milliseconds. |
| `hover_scale` | number, 0.5 to 2.0 | `1.0` | Uniform scale while hovered. |
| `hover_lift` | number, 0 to 64 | `0` | Upward travel while hovered, in logical pixels. |
| `glow` | colour | unset | Outer glow colour while hovered. |
| `glow_size` | number, 0 to 64 | `0` | Glow spread in logical pixels. Zero disables it even when `glow` is set. |
| `effect` | enum: `none`, `sweep`, `sheen` (alias `shine`), `pulse`, `glow`, `corners` | `"none"` | Decoration drawn over or around the button while hovered. |
| `effect_color` | colour | white | Effect colour. |
| `effect_ms` | number, 100 to 20000 | `1400` | One full effect cycle in milliseconds. |
| `effect_angle` | number, -360 to 360 | `120` | Sweep and sheen band direction in degrees. |
| `effect_strength` | number, 0 to 1 | `0.25` | Effect alpha. |
| `rotation` | number, -360 to 360 | `0` | Rotate the button around its centre. |

`sweep` moves a wide highlight across the face, `sheen` is the narrow collectible-card highlight, `pulse` breathes the fill, `glow` breathes the outer glow, and `corners` draws four expanding target marks. Effects run only while the pointer is over the button. `[motion].reduced` and `[power].low_power` skip them. The bracket is decoration outside the hit area, so a click in the gap does not submit unexpectedly.

When it goes wrong: `effect = "sparkle"` reports `unknown effect "sparkle"; use none, sweep, sheen, pulse, glow, or corners` and keeps `none`. `hover_direction = "diagonal"` reports `unknown direction "diagonal"; use fade, left, right, up, down, or center` and keeps `fade`. `height = 10` reports `must be between 20 and 160 (got 10)` and keeps `44`. An out-of-range `effect_ms` keeps `1400`.

### `[login.transition]`

This subsection controls the optional cut sequence that plays around a login attempt. It is decorative only: the auth request is already on its way before the first cut paints, and the greeter never waits for the animation to accept or reject a password. It is off by default.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Play the sequence. |
| `trigger` | enum: `enter`, `success`, `both`, `boot`, `failure` | `"enter"` | `enter` starts when an auth request is sent. `success` waits for greetd to accept the start. `both` keeps the historical enter plus success pair and does not mean boot or failure. `boot` plays once when the window first appears. `failure` plays on a rejected attempt. |
| `cuts` | array of colours, at most 8 | `["#ffffff", "#d41830", "#0e0b10"]` | Full-screen flash colours, one per cut. An array with no usable colour falls back to the default. |
| `cut_ms` | number, 0 to 1000 | `55` | Time per cut in milliseconds. Zero-length cuts are skipped. |
| `shake` | number, 0 to 64 | `14` | Peak shake in logical pixels. It decays to zero across the slam. |
| `zoom` | number, 0 to 0.5 | `0.06` | Extra scale punch at the start of the slam. |
| `slam` | string, at most 96 characters | `"WELCOME"` | Big centred line shown after the cuts. Empty keeps the colour field without text. |
| `slam_ms` | number, 0 to 10000 | `900` | Total slam time including its fade, in milliseconds. Zero collapses the whole sequence to nothing. |
| `slam_color` | colour | `#f3e7d2` | Slam text colour. |
| `slam_size` | number, 8 to 240 | `92` | Slam font size in logical pixels. |

The sequence has three parts. The cuts flash one colour at a time, the last colour becomes the field behind the slam line, and then the field and the line fade back to the normal login screen. `enter` starts the moment Enter sends a request, so the form's busy state and the animation run together. `success` is the only mode that briefly holds the window open, and that hold is capped at two seconds. `boot` fires once on the first presented frame and never touches the auth path. `failure` fires after the app has already returned the form to its typing state.

```toml
[login.transition]
enabled = true
trigger = "enter"
cuts = ["#ffffff", "#d41830", "#0e0b10"]
cut_ms = 55
shake = 14
zoom = 0.06
slam = "WELCOME TO THE PARLOR"
slam_ms = 900
slam_color = "#f3e7d2"
slam_size = 92
```

A transparent last cut is useful for a success montage: the field lets the particle burst and the live poster read through while the slam line sits on top. That is how the `hutao-montagem` example gets butterflies and type in the same frame.

When it goes wrong: an unknown trigger reports `unknown value "fail"; use enter, success, both, boot, or failure` and keeps `enter`. `cuts = []` is reported as `no usable colours; using the defaults` and the three default colours are used. `slam_size = 4` reports `must be between 8 and 240 (got 4)` and keeps `92`. A zero `slam_ms` and zero `cut_ms` make the sequence total zero time, which returns to the normal screen immediately.

### `[login.session]` and `[login.hints]`

This subsection controls the session row and the keyboard hint line. Both are text only.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `session.label` | string, at most 64 characters | `"Session"` | Session row label. Empty hides the label but keeps the row. |
| `session.change_hint` | string, at most 64 characters | `"Tab to change"` | Shown when more than one session exists. |
| `session.empty` | string, at most 128 characters | `"No sessions found"` | Shown when discovery returned nothing. |
| `session.show` | bool | `true` | Draw the session row at all. Overrides `login.show_session`. |
| `hints.username` | string, at most 160 characters | `"Enter  Continue      Tab  Password      Esc  Clear"` | Hint line while the username is focused. |
| `hints.password` | string, at most 160 characters | `"Enter  Sign in      Tab  Session      Esc  Clear"` | Hint line while the password is focused. |
| `hints.picker` | string, at most 160 characters | `"Up/Down  Choose      Enter  Select      Esc  Cancel"` | Hint line while the picker is open. |
| `hints.busy` | string, at most 160 characters | `""` | Hint while signing in. Empty falls back to `login.busy_text`. |

When it goes wrong: a string above the character limit reports `must be at most N characters` and keeps the default. A control character in a hint reports `must not contain control characters`. `session.show = "yes"` reports `expected true or false` and keeps `true`.

### `[[login.decor]]`

This subsection controls decorative items inside the card. Decor places a logo, social button, footer line, panel, or other cosmetic item inside the card. The full key reference lives in [Decor item reference](#decor-item-reference). The short version: items paint after the card surface and before the form, in list order, so a later item covers an earlier one and no item can cover an input or the submit button. The built-in widgets paint behind decor by default; set `layer = "front"` on `[clock]` or `[media]` to paint after the decor but still before the form.

```toml
[[login.decor]]
kind = "image"
image = "brand/wordmark.png"
width = 174
anchor = "top-left"
offset = [430, 42]

[[login.decor]]
kind = "text"
text = "© 2026 Example Studio"
size = 12
color = "#ffffff"
anchor = "bottom-right"
offset = [24, 20]
```

## `[clock]`

This section controls the first-party clock widget: its face, format, placement, plate, motion, and state visibility.

### Clock and battery in the corners

This pairs a quiet clock at the top right with a pill battery at the bottom right. Both widgets take the same anchor vocabulary and both can be dragged, so the corners stay honest on different screen sizes.

```toml
[clock]
enabled = true
anchor = "top-right"
offset = [32, 32]
time_format = "%H:%M"
date_format = "%a %d %b"
size = 44
date_size = 13
ghost = false

[battery]
enabled = true
anchor = "bottom-right"
offset = [32, 32]
style = "pill"
width = 220
low_at = 20
show_percent = true
```

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Draw the clock. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` paints in screen space before decor. `front` paints after decor and before the form and switches the widget into card space, so both its offset and size scale with the card. |
| `style` | enum: `digital`, `analog`, `flip`, `segment` | `"digital"` | Digital text, an analog dial, split-flap cards, or seven-segment digits. For analog, `size` is the dial diameter. Flip and segment use the same formatted time string as digital. |
| `time_format` | strftime string, at most 64 characters | `"%H:%M"` | Time pattern. Validated at load time with jiff. Invalid patterns fall back to the default. |
| `date_format` | strftime string, at most 64 characters | `"%A, %B %d"` | Date pattern. Empty hides the date line. |
| `hour12` | bool | `false` | Rewrite `%H` to `%I` and append `%p`. An explicit `%I`, `%l`, or `%r` wins. |
| `seconds` | bool | `false` | Insert seconds before an AM/PM marker, or append them when there is none, if the format has no `%S`. An explicit `%S` wins. |
| `timezone` | IANA zone name, at most 64 characters | unset (system zone) | For example `"Europe/Berlin"`. An unknown zone is reported and the system zone is used. |
| `posix_locale` | bool | `false` | Format `%c`, `%x`, `%X`, and day and month names with the POSIX/C locale. |
| `anchor` | enum: the seven anchors listed under `[login]` | `"top-center"` | Placement anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 48]` | Logical pixel margin from the anchor. |
| `monitor` | string, at most 64 characters | unset | Output name restriction. Unset shows the widget on every output, and a missing output name in screenshot mode shows every widget. |
| `rotation` | number, -360 to 360 | `0` | Rotate the clock around its centre. |
| `size` | number, 12 to 180 | `54` | Digital and text font size, or analog dial diameter. |
| `date_size` | number, 8 to 48 | `18` | Date font size. |
| `color` | colour | theme primary text | Clock text colour override. |
| `date_color` | colour | theme `muted` | Date colour override. |
| `label` | string, at most 64 characters | `""` | Small label above the time. Empty hides it. |
| `label_color` | colour | the clock `color` | Label colour override. |
| `label_size` | number, 0 to 48 | `0` (derive 22 percent of `size`) | Label size. |
| `label_tracking` | number, 0 to 0.5 | `0` | Label letter spacing in em. |
| `ghost` | bool | `false` | Segment face only: draw the unused segments at low alpha, like an unlit LCD. The other faces ignore it. |
| `glow` | colour | unset | Segment face only: halo colour for the active segments. |
| `glow_size` | number, 0 to 32 | `0` | Halo spread in logical pixels. Zero disables the glow even when `glow` is set. |
| `colon_blink_ms` | number, 0 to 5000 | `0` | Full on and off period for the colon in milliseconds. Zero keeps it steady. Applies to the flip and segment faces. |
| `second_hand` | enum: `tick`, `sweep` | `"tick"` | Analog only. `tick` jumps once per second; `sweep` follows the subsecond clock and asks for more frames. |
| `draggable` | bool | `true` | Let the pointer move the clock. The position can be saved by `[layout]`. |
| `plate` | table | see [Widget plates](#widget-plates) | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget, plate included. |
| `show_on` | array of state names, at most 8 | empty | States that may show the clock. Empty means every state. |
| `hide_on` | array of state names, at most 8 | empty | States that hide the clock. It wins over `show_on`. |

Formats use jiff's strftime implementation, which covers the usual `%H`, `%M`, `%S`, `%A`, `%B`, `%d`, and `%Z` specifiers. Day and month names are English by default; `posix_locale = true` selects the POSIX/C forms. There is no full per-locale translation layer in this release.

The clock asks for a redraw only when its visible strings change. A minute clock does not wake the renderer sixty times a minute.

The flip face paints one split-flap card per character, using the widget plate as the card material:

```toml
[clock]
style = "flip"
time_format = "%H:%M"
seconds = false
size = 42
color = "#F5F3FF"
colon_blink_ms = 1000
```

The segment face draws real seven-segment digits. `ghost` shows the unlit segments, and `glow` adds a halo behind the active ones:

```toml
[clock]
style = "segment"
size = 48
color = "#22D3EE"
ghost = true
glow = "#22D3EE"
glow_size = 6
```

When it goes wrong: an unknown style reports `unknown style "digitial"; use digital, analog, flip, or segment` and keeps `digital`. An invalid format reports `not a valid strftime format: ...` and keeps the default. An empty `time_format` reports `must not be empty`; an empty `date_format` is valid and hides the date. An unknown timezone reports `cannot load time zone "Mars/Base": ...` and uses the system zone. `glow_size = 40` reports `must be between 0 and 32 (got 40)` and keeps `0`.

## `[media]`

This section controls the first-party media widget. It polls MPRIS on the greeter's session bus on a dedicated thread, so a slow or broken player cannot block the UI or the greetd socket.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Show the media widget. |
| `layer` | enum: `behind`, `front` | `"behind"` | Same paint order and card-space rule as `[clock].layer`. |
| `anchor` | enum: the seven anchors | `"bottom-left"` | Placement anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[32, 32]` | Logical pixel margin. |
| `monitor` | string, at most 64 characters | unset | Output name restriction. |
| `rotation` | number, -360 to 360 | `0` | Rotate the widget around its centre. A rotated widget keeps its controls visible but ignores clicks. |
| `size` | number, 8 to 48 | `16` | Text size in logical pixels. |
| `show_art` | bool | `true` | Show local `file://` album art. Network URLs are never fetched. |
| `layout` | enum: `card`, `compact`, `vinyl` | `"card"` | Card is art left and text right. Compact is one tight row. Vinyl is a circle with centred text. |
| `art` | enum: `cover`, `disc`, `none` | `"cover"` | Cover keeps the square crop. Disc masks it into a circle once at load time. None hides it and lets the text use the full width. |
| `progress` | enum: `line`, `segments`, `wave` | `"line"` | Line is a thin bar. Segments is twelve blocks. Wave is a decorative sine that encodes the playback fraction, not audio data. |
| `controls` | bool | `false` | Draw previous, play/pause, and next buttons. They are clickable once the widget has real metadata and the widget is not rotated. |
| `tint_from_art` | bool | `false` | Derive the progress colour and plate border and glow from the dominant album-art colour, falling back to the theme accent. |
| `color` | colour | theme primary text | Title colour override. |
| `show_before_login` | bool | `true` | Draw the last visible track before authentication. Set to `false` on shared machines where even the track name is private. |
| `keep_last` | bool | `true` | Keep the last track when the player pauses, stops, or disappears. The widget labels it `Last played`. |
| `draggable` | bool | `true` | Let the pointer move the widget. The position can be saved by `[layout]`. |
| `plate` | table | see [Widget plates](#widget-plates) | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array of state names, at most 8 | empty | States that may show the media widget. |
| `hide_on` | array of state names, at most 8 | empty | States that hide the media widget. It wins over `show_on`. |

The greeter runs before the user's session exists. In practice the user's player is usually not on the bus dawn can reach, so the widget shows a calm `No media` state. If a player is visible and answers, dawn keeps the last useful snapshot instead of wiping the widget when the player goes away. On a shared machine, set `show_before_login = false` and the renderer keeps metadata off the login pixels until greetd accepts authentication.

The default card layout is the original composition. A compact corner widget looks like this:

```toml
[media]
layout = "compact"
art = "cover"
progress = "line"
size = 15
```

The vinyl layout masks the cover into a disc once at load time and draws the line progress as a ring around it. `tint_from_art` takes the dominant cover colour for the ring and the plate border:

```toml
[media]
layout = "vinyl"
art = "disc"
progress = "line"
tint_from_art = true
size = 18

[media.plate]
border = "#FFFFFF22"
glow = "#FFFFFF"
glow_size = 8
```

`controls = true` adds a transport row under the progress. Once the widget is showing real metadata, the three glyphs are real buttons: clicking one asks the MPRIS worker to send Previous, PlayPause, or Next to the player it is already polling. A rotated media widget still draws the row but ignores clicks so the hit regions stay honest. The buttons do nothing in the `No media` state, because there is no player to talk to.

When it goes wrong: an unknown `layout` reports `unknown value "coverflow"; use card, compact, or vinyl` and keeps `card`. An unknown `art` or `progress` keeps `cover` or `line`. `size = 60` reports `must be between 8 and 48 (got 60)` and keeps `16`. A `controls = "on"` value reports `expected true or false` and keeps `false`.

### Widget plates

This section controls the backplate drawn behind a built-in widget or the ticker.

`[clock.plate]`, `[media.plate]`, `[calendar.plate]`, `[battery.plate]`, `[system.plate]`, and `[ticker.plate]` replace the fixed scrim with a themed backplate. The defaults reproduce the old pill exactly: a tinted fill, a 14 px radius, 12 px padding, full opacity, and no glow or shadow. The plate wraps the entire widget, including album art and the progress bar, and it scales with the output just like the widget content.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `style` | enum: `none`, `tint` (alias `transparent` for none), `solid`, `gradient`, `glass` (alias `blur`) | `"tint"` | `none` draws no plate, border, glow, or shadow. `tint` is the historical scrim. `solid` is one flat fill. `gradient` blends two colours. `glass` samples the shared blurred backdrop first and then applies `color` when it is set. |
| `color` | colour | unset | Fill for `solid` (falls back to theme `tint`) and the first stop for `gradient`. For `glass` it is an optional tint over the blur, so a translucent value such as `#10172655` shows the wallpaper. Unset means pure blur. |
| `color_end` | colour | transparent | Second gradient stop. Unset fades to nothing. |
| `gradient_angle` | number, -360 to 360 | `0` | Gradient direction in degrees. Zero goes left to right. |
| `radius` | number, 0 to 64 | `14` | Corner radius in logical pixels. The renderer clamps it to half the plate height. |
| `border` | colour | unset | One-pixel stroke. `tint` already has a faint theme border; setting this overrides it. |
| `padding` | number, 0 to 64 | `12` | Space between the content and the plate edge. |
| `glow` | colour | unset | Outer halo colour. |
| `glow_size` | number, 0 to 48 | `0` | Halo spread in logical pixels. Zero disables it. |
| `shadow` | bool | `false` | Draw a soft drop shadow under the plate. |
| `opacity` | number, 0 to 1 | `1` | Overall opacity for the widget and its plate. Zero hides both. |

```toml
[clock.plate]
style = "gradient"
color = "#17191CE6"
color_end = "#17191C00"
gradient_angle = 90
radius = 10
padding = 14
border = "#22D3EE66"
glow = "#22D3EE"
glow_size = 10
shadow = true
opacity = 0.9
```

A glass plate uses the same cached backdrop as the login card:

```toml
[clock.plate]
style = "glass"
color = "#10172655"
radius = 18
padding = 16
border = "#ffffff22"
```

The backdrop is a blur of the wallpaper, not a blur of the plate's own colour. Leaving `color` unset gives pure blur; add a translucent colour like the one above when a tint is also wanted. `glass` and `blur` mean the same thing on every plate table.

`[clock.motion]` and `[media.motion]` reuse the animated-decor table verbatim: `spin`, `sway`, `float`, `pulse`, `fade`, `phase`, and `blink_ms`. The transform applies to the whole widget, plate included. `typer_ms` and the ticker keys only make sense for a text run and are ignored here. `[motion].reduced` and `[power].low_power` pin the sample at its neutral pose, so a configured widget never disappears when animation is turned off.

`show_on` and `hide_on` accept up to eight state names. Omitting `show_on` means "show in every state"; `hide_on` wins whenever both lists match. Exactly one state is current per frame, chosen in this priority order:

| State | When |
| --- | --- |
| `busy` | An authentication request is in flight. |
| `error` | The status line is showing an error. |
| `authenticated` | greetd accepted the credentials. |
| `typing` | The username or password field has content. |
| `idle` | None of the above. |

```toml
[clock]
show_on = ["idle", "typing", "error"]
hide_on = ["busy", "authenticated"]
```

Unknown state names are reported once each and skipped, so one typo does not hide the whole widget. Duplicate names are removed at load time. An empty list is valid: for `show_on` it means every state, and for `hide_on` it means never hide.

When a plate goes wrong: an unknown style reports `unknown plate style "frost"; use none, tint, solid, gradient, or glass` and keeps `tint`. `opacity = 2` reports `must be between 0 and 1 (got 2)` and keeps `1`. A `color` with no `#` reports `colour must start with '#'` and the plate uses its fallback. `glow` without a positive `glow_size` draws no glow; this is valid.

## `[calendar]`

This section controls the first-party month grid. It is built from the local date, has no event source, and makes no network request. It is disabled by default so an existing config renders exactly as before.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the calendar. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` paints in screen space before decor. `front` paints after decor and before the form and uses card space. |
| `anchor` | enum: the seven anchors | `"top-right"` | Placement anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[40, 40]` | Logical pixel margin from the anchor. |
| `monitor` | string, at most 64 characters | unset | Output name restriction. Unset shows the widget on every output. |
| `rotation` | number, -360 to 360 | `0` | Rotate the widget. |
| `size` | number, 8 to 48 | `13` | Day number font size. The cell height is twice this value. |
| `width` | number, 120 to 800 | `260` | Total content width. The seven columns divide it evenly. |
| `week_start` | enum: `monday`, `sunday` | `"monday"` | Which day starts the week row. |
| `show_header` | bool | `true` | Draw the month name and year above the weekday row. |
| `color` | colour | `#F7F3E9` | Ordinary day numbers. |
| `muted_color` | colour | `#8A8D80` | Weekday labels, empty cells, and other secondary text. |
| `today_color` | colour | `#E2734F` | Fill behind the current day. |
| `header_color` | colour | `#E7DA19` | Month and year label. |
| `draggable` | bool | `true` | Let the pointer move the calendar. The position can be saved by `[layout]`. |
| `plate` | table | see [Widget plates](#widget-plates) | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array of state names, at most 8 | empty | States that may show the calendar. |
| `hide_on` | array of state names, at most 8 | empty | States that hide the calendar. It wins over `show_on`. |

```toml
[calendar]
enabled = true
anchor = "top-right"
offset = [40, 40]
size = 13
width = 260
week_start = "monday"
show_header = true
color = "#F7F3E9"
muted_color = "#8A8D80"
today_color = "#E2734F"
header_color = "#E7DA19"
draggable = true
```

When it goes wrong: `week_start = "saturday"` reports `unknown value "saturday"; use monday or sunday` and keeps `monday`. `width = 900` reports `must be between 120 and 800 (got 900)` and keeps `260`. `size = 2` reports `must be between 8 and 48 (got 2)` and keeps `13`. `[calendar.plate]`, `[calendar.motion]`, `show_on`, and `hide_on` accept exactly the same values as the clock equivalents.

## `[battery]`

This section controls the local battery indicator. It reads `/sys/class/power_supply` once every two seconds and never writes a file. It is disabled by default, and on a desktop with no battery it draws a muted `NO BATTERY` label instead of a fake percentage.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the battery widget. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` paints in screen space before decor. `front` paints after decor and before the form and uses card space. |
| `anchor` | enum: the seven anchors | `"bottom-right"` | Placement anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[40, 40]` | Logical pixel margin. |
| `monitor` | string, at most 64 characters | unset | Output name restriction. |
| `rotation` | number, -360 to 360 | `0` | Rotate the widget. |
| `size` | number, 8 to 48 | `14` | Text and geometry scale. |
| `width` | number, 120 to 800 | `220` | Total content width. |
| `style` | enum: `ring`, `bar`, `pill` | `"ring"` | Ring is sixty radial segments with the percentage in the middle. Bar is horizontal with the percentage to the right. Pill is a battery body with a terminal nub. |
| `color` | colour | `#F7F3E9` | Normal charge colour. |
| `low_color` | colour | `#E2734F` | Colour at or below `low_at` while discharging. |
| `charging_color` | colour | `#C6D62B` | Colour while the battery reports `Charging`. Charging wins over low. |
| `low_at` | integer, 0 to 100 | `20` | Low threshold as a percentage. |
| `show_percent` | bool | `true` | Draw the percentage text. The charging bolt is drawn independently. |
| `draggable` | bool | `true` | Let the pointer move the widget. The position can be saved by `[layout]`. |
| `plate` | table | see [Widget plates](#widget-plates) | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array of state names, at most 8 | empty | States that may show the battery widget. |
| `hide_on` | array of state names, at most 8 | empty | States that hide the battery widget. It wins over `show_on`. |

```toml
[battery]
enabled = true
anchor = "bottom-right"
offset = [40, 40]
size = 14
width = 220
style = "ring"
color = "#F7F3E9"
low_color = "#E2734F"
charging_color = "#C6D62B"
low_at = 20
show_percent = true
draggable = true
```

`low_at = 0` means the low colour is only used at 0 percent, which mostly disables the warning colour. `low_at = 100` makes every discharging reading use `low_color`. A battery that reports `Unknown` or `Not charging` stays on `color`; only the literal `Charging` state selects `charging_color`.

When it goes wrong: `style = "circle"` reports `unknown value "circle"; use ring, bar, or pill` and keeps `ring`. `low_at = 120` reports `must be between 0 and 100 (got 120)` and keeps `20`. `low_at = 20.5` reports `expected a whole number` and keeps `20`.

## `[system]`

This section controls the local system monitor. It reads `/proc/sys/kernel/hostname`, `/proc/sys/kernel/osrelease`, `/proc/uptime`, `/proc/loadavg`, and `/proc/meminfo`. It is disabled by default and never writes a file or contacts the network.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the system monitor. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` paints in screen space before decor. `front` paints after decor and before the form and uses card space. |
| `anchor` | enum: the seven anchors | `"top-left"` | Placement anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[40, 40]` | Logical pixel margin. |
| `monitor` | string, at most 64 characters | unset | Output name restriction. |
| `rotation` | number, -360 to 360 | `0` | Rotate the widget. |
| `size` | number, 8 to 48 | `12` | Font and row scale. |
| `width` | number, 120 to 800 | `280` | Content width. |
| `style` | enum: `rows`, `sparkline` | `"rows"` | Rows is label and value pairs. Sparkline adds a load history above those rows. |
| `fields` | array of enum names, at most 6 | `["hostname", "kernel", "uptime", "load", "memory"]` | Ordered list of `hostname`, `kernel`, `uptime`, `load`, and `memory`. Unknown names and duplicates are dropped with an issue. An empty list is allowed and hides the text rows. |
| `color` | colour | `#F7F3E9` | Reading values and the sparkline. |
| `label_color` | colour | `#4E5148` | Uppercase labels and the sparkline baseline. |
| `draggable` | bool | `true` | Let the pointer move the widget. The position can be saved by `[layout]`. |
| `plate` | table | see [Widget plates](#widget-plates) | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array of state names, at most 8 | empty | States that may show the monitor. |
| `hide_on` | array of state names, at most 8 | empty | States that hide the monitor. It wins over `show_on`. |

```toml
[system]
enabled = true
anchor = "top-left"
offset = [40, 40]
size = 12
width = 280
style = "rows"
fields = ["hostname", "kernel", "uptime", "load", "memory"]
color = "#F7F3E9"
label_color = "#4E5148"
draggable = true
```

The sparkline keeps the last 32 one-minute load samples, collected on the same two-second refresh. Uptime is shown as `3d 04h`, `02h 14m`, or `14m`, and memory as `6.2 / 15.5 GiB`.

When it goes wrong: `style = "chart"` reports `unknown value "chart"; use rows or sparkline` and keeps `rows`. `fields = ["hostname", "hostname", "disk"]` reports one error for the duplicate and one for the unknown field, then keeps the usable ordered list. More than six entries reports `accepts at most 6 fields` and the first six are used. An entry that is not a string reports `every field must be a string`.

All built-in widgets share the clock's plate and motion vocabulary, so a HUD theme can give them the same border and glow:

```toml
[calendar.plate]
style = "gradient"
color = "#17191CE6"
color_end = "#17191C00"
gradient_angle = 90
border = "#E7DA1966"
padding = 14
radius = 10
```

## `[ticker]`

This section controls the kinetic marquee that scrolls one joined text run across the top or bottom edge. It is disabled by default. The run is measured with the exact weight and tracking it is painted with, then repeated at that width, so the loop has no visible jump at any font size.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Draw the marquee. |
| `items` | array of strings, at most 8 entries, each at most 128 characters | empty | Short lines joined with `separator`. Empty entries are dropped. An enabled ticker with no usable items is reported and paints nothing. |
| `separator` | string, at most 32 characters | `"  •  "` | Inserted between items and repeated at the seam. Leading and trailing spaces are kept, because the spacing is part of the design. |
| `anchor` | enum: `top`, `bottom` | `"bottom"` | Edge the strip hugs. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 24]` | `x` is the side margin; `y` is the distance from the chosen edge. |
| `size` | number, 8 to 96 | `20` | Text size in logical pixels. |
| `speed` | number, 0 to 400 | `60` | Scroll speed in logical pixels per second. Zero keeps the run still. |
| `color` | colour | `#F7F3E9` | Marquee text. |
| `bold` | bool | `true` | Bold run. |
| `tracking` | number, 0 to 0.5 | `0.2` | Letter spacing in em. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` paints under the login form. `front` paints above the form and the warning line, and is the second-topmost layer after front particles. |
| `session_first` | bool | `false` | Put the selected session name at the head of the run, uppercased, and flash the run in the accent colour for 80 ms when the session changes. The launch command is not touched. |
| `plate` | table | theme `tint` scrim | Same table as [Widget plates](#widget-plates). `style = "glass"` gives the strip the shared blurred backdrop. |

```toml
[ticker]
enabled = true
items = ["WANGSHENG AFTER DARK", "77TH DIRECTOR", "LIVE AT THE PARLOR"]
separator = "  •  "
anchor = "bottom"
offset = [24, 24]
size = 20
speed = 60
color = "#d41830"
bold = true
tracking = 0.2
layer = "behind"

[ticker.plate]
style = "glass"
color = "#0e0b1066"
radius = 0
padding = 14
border = "#d8a84b55"
```

The marquee freezes when `[motion].reduced` or `[power].low_power` is set, and it only asks for ambient animation frames while it is enabled and moving. `session_first` can make the run scroll even when `items` is empty, because the selected session supplies the text.

When it goes wrong: `items = ["A", "B", "C", "D", "E", "F", "G", "H", "I"]` reports `accepts at most 8 items` and the first eight are used. A non-string item reports `every item must be a string` and is skipped. A separator with a control character reports `must not contain control characters` and keeps the default. `speed = 500` reports `must be between 0 and 400 (got 500)` and keeps `60`.

## `[particles]`

This section controls the procedurally drawn ambient emitters and the optional success burst. There are no image files: a flame is a radial ember, a butterfly is two mirrored wing triangles, and a dot is a plain circle. The table is disabled by default and is capped at 96 ambient particles.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Run the ambient emitter. |
| `count` | integer, 0 to 96 | `48` | Live particles. Values outside the range are reported and fall back. |
| `sprite` | enum: `flame` (alias `ember`), `butterfly` (alias `moth`), `dot` (alias `sphere`), `streak` (alias `ray`) | `"flame"` | Sprite shape. A streak is a thin vertical light ray. |
| `color` | colour | `#FF7A3C` | Flame core, dot colour, streak colour, and the first butterfly wing. |
| `color2` | colour | `#D41830` | Second butterfly wing. Ignored by the other sprites. |
| `drift` | enum: `up`, `down`, `float` | `"up"` | Travel direction. `float` wanders on a sine path without leaving the field. |
| `speed` | number, 0 to 400 | `24` | Base travel in logical pixels per second. Zero keeps the field in place. |
| `size` | number, 1 to 48 | `6` | Base sprite size in logical pixels. |
| `opacity` | number, 0 to 1 | `0.55` | Overall opacity. Zero hides the field. |
| `layer` | enum: `behind`, `front` | `"behind"` | `behind` draws over the wallpaper and under the form. `front` is the topmost layer, above the form and warnings. |
| `burst_on` | enum: `none` (alias `off`), `success` | `"none"` | Fire one burst when greetd accepts a login. The burst never changes the auth result. |
| `burst_sprite` | enum: same sprite names as `sprite` | `"butterfly"` | Sprite used by the burst, independent of the ambient `sprite`. |
| `burst_count` | integer, 0 to 64 | `24` | Number of sprites in the burst. |

```toml
[particles]
enabled = true
count = 48
sprite = "flame"
color = "#ff7a3c"
color2 = "#d41830"
drift = "up"
speed = 24
size = 6
opacity = 0.55
layer = "behind"
```

Positions come from a stable hash of the particle index plus the session clock, so every run is deterministic and a screenshot is reproducible. A particle that leaves the edge wraps back in on the opposite side. Flames flicker in size and opacity, and butterflies flap by shrinking their wing span. The emitter freezes with `[motion].reduced` and `[power].low_power`.

The burst is a second, one-shot field. Every sprite is a closed-form function of `(index, age)`, so the same age always gives the same frame and a dropped frame cannot spawn a second copy. Sprites start at the centre of the stage, travel outward with exponential drag, clamp inside the screen, and fade over 1.8 seconds. Once the age passes that deadline the window stops scheduling frames for them.

When it goes wrong: `count = 200` reports `must be between 0 and 96 (got 200)` and keeps `48`. `burst_count = 100` reports `must be between 0 and 64 (got 100)` and keeps `24`. An unknown sprite reports `unknown value "spark"; use flame, butterfly, dot, or streak` and keeps `flame`. `size = 0` reports `must be between 1 and 48 (got 0)` and keeps `6`.

## `[motion]`

This section controls the boot, focus, error, and drag motion, plus the ambient decor frame cap and global pointer parallax. It does not control the wallpaper; `[power]` and `reduced` do.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Master switch for boot, focus, and error motion. |
| `reduced` | bool | `false` | Skip motion and freeze animated wallpapers on one frame. Static scanlines stay. |
| `boot_ms` | integer, 0 to 2000 | `450` | Card slide and fade duration in milliseconds. |
| `focus_ms` | integer, 0 to 1000 | `160` | Focus-ring settle duration. |
| `shake_ms` | integer, 0 to 2000 | `320` | Error shake duration. |
| `drag_ms` | integer, 0 to 1000 | `120` | Time constant for pointer-drag easing. `0` snaps to the pointer. |
| `decor_fps` | integer, 10 to 60 | `30` | Refresh rate for ambient decor only. Focus, typing, boot, and button effects still use the full motion rate. |
| `parallax` | number, 0 to 2 | `0` | Global cursor parallax multiplier. Each decor item or built-in widget also needs a positive `parallax_depth`. Zero disables it everywhere. |

Animated decor is rasterised inside the cached UI overlay, so a moving ornament would otherwise force a full overlay repaint on every 60 fps wallpaper frame. `decor_fps` caps only that ambient path. Input, focus, the boot animation, and button hover effects stay at the full rate. Parallax only runs in a live window with a known pointer position, so screenshots stay at the neutral offset.

When it goes wrong: `decor_fps = 5` reports `must be between 10 and 60 (got 5)` and keeps `30`. `parallax = 4` reports `must be between 0 and 2 (got 4)` and keeps `0`. A negative duration reports the range and keeps the default. `boot_ms = 0` is valid and removes the boot animation.

## `[theme]`

This section controls the resolved design tokens: text, accents, the glass tint, blur, corner radius, spacing, grain, contrast, and CRT scanlines. Presets supply defaults; every token can still be overridden.

```toml
[theme]
preset = "glass-dark"   # glass-dark, glass-light, or minimal
```

Set `package` to use an installed theme bundle instead of this file:

```toml
[theme]
package = "midnight"
```

`dawn use midnight` writes this key for you and leaves the rest of the file
alone.

A pointer config is not merged with the bundle. The bundle's `config.toml`
becomes the whole config, and its relative asset paths resolve against the
bundle directory. Bundle lookup checks `/usr/share/dawn/themes` first, then
every `DAWN_THEME_DIRS` entry, and only then the deprecated
`$XDG_DATA_HOME/dawn/themes` store. A theme that exists only in the user store
still loads but prints a warning asking for a system reinstall. See `dawn list`,
`dawn use`, `dawn add`, and the theme section in [installation.md](installation.md).
`package` is not a token, so the rest of the `[theme]` table is ignored when it
is present.

### A theme from zero

A theme bundle is an immutable directory with a DSB-2 manifest, a complete `config.toml`, a `preview.webp`, and an `assets/` root. The config below is the palette core of a bundle. The manifest rules, directory layout, and store resolution are in [the bundle specification](bundle-spec.md).

```toml
[background]
kind = "solid"
color = "#101114"

[theme]
preset = "minimal"        # start from the neutral token set
text = "#f5f5f5"          # primary text
muted = "#a0a0a0"         # placeholders, hints, dates
accent = "#9ecbff"        # focus rings, caret, selection
error = "#ff948c"         # login errors
tint = "#17181cf2"        # glass fill; "f2" is nearly opaque
border = "#ffffff22"      # one-pixel edge
shadow = "#00000055"      # soft shadow
focus_ring = "#9ecbff"
corner_radius = 10
spacing = 0.95
noise = false
min_contrast = 4.5

[theme.blur]
enabled = false           # minimal has no backdrop blur by default
```

| Key | Type and range | Default (glass-dark) | What it does |
| --- | --- | --- | --- |
| `preset` | enum: `glass-dark` (alias `dark`), `glass-light` (alias `light`), `minimal` | `"glass-dark"` | Base token set. The light preset also supplies a light background colour when `[background].color` is unset. |
| `text` | colour | `#f2f6ff` | Primary text and input text. |
| `muted` | colour | `#a9b4c8` | Placeholders, hints, prompts, dates. |
| `accent` | colour | `#7db1ff` | Focus rings, caret, selection, dry-run badge. |
| `error` | colour | `#ff8a80` | Login errors and asset warnings. |
| `tint` | colour | `#101726cc` | Glass panel and widget scrim tint. The last two digits are alpha, so `00` makes a glass card invisible and `ff` makes it opaque. |
| `border` | colour | `#ffffff40` | One-pixel panel highlight. |
| `shadow` | colour | `#00000066` | Soft shadow layers behind panels. |
| `focus_ring` | colour | `#7db1ff` | Focus-ring token. |
| `corner_radius` | number, 0 to 64 | `18` | Default panel corner radius, used when `login.radius` is unset. |
| `spacing` | number, 0.75 to 2.0 | `1.0` | Padding and gap multiplier. |
| `noise` | bool | `true` | Subtle grain over glass panels. |
| `min_contrast` | number, 1.0 to 21.0 | `4.5` | Minimum contrast ratio for text against the composited glass tint. The renderer adjusts text that would miss it. |
| `scanlines` | number, 0 to 1 | `0.0` | CRT scanline strength. Zero turns the overlay off. |
| `scanline_gap` | integer, 2 to 12 | `3` | Physical pixel spacing between scanlines. Smaller is denser. |
| `scanline_ms` | number, 1000 to 20000 | `6000` | Time for the bright sweep band to cross the screen once, in milliseconds. |

The preset defaults differ in more than colour. `glass-light` uses text `#101318`, muted `#4d5665`, accent `#1f6feb`, error `#b42318`, tint `#f7f9fcd2`, border `#ffffffcc`, and shadow `#00000033`. `minimal` uses text `#f5f5f5`, muted `#a0a0a0`, accent `#9ecbff`, error `#ff948c`, tint `#17181cf2`, border `#ffffff22`, shadow `#00000055`, corner radius `10`, spacing `0.95`, noise off, and blur off by default. The background fallback follows the preset: `#0b1020` for glass-dark, `#dfe8f5` for glass-light, and `#101114` for minimal.

A subtle CRT backdrop looks like this:

```toml
[theme]
scanlines = 0.35
scanline_gap = 3
scanline_ms = 6000
```

The static lines paint directly onto the wallpaper, before the glass and the login form, so the effect textures the backdrop instead of the text. Reduced motion and low-power mode keep the lines but drop the moving sweep band.

`card` is accepted as a legacy alias for `tint`; `tint` wins when both are present. `blur_radius`, `blur_refresh_ms`, and `blur_downsample` are accepted for older configs. `blur_radius` maps to `[theme.blur].size` only when that key is absent, and `blur_refresh_ms` maps to `[theme.blur].refresh_ms` only when that key is absent. `blur_downsample` is validated as a number from 1 to 16 and then ignored, because the dual Kawase pyramid does not use a single downsample factor.

`min_contrast` is not a filter on the wallpaper. The renderer composites the theme `tint` over the configured background colour to estimate the panel colour, then moves text, muted text, error text, accent, and focus rings toward black or white only when the configured colour would miss the target. Setting it to `1.0` disables the adjustment. Setting it above 21 is outside the WCAG range and is rejected. Very high values can push every text token to pure black or pure white, which is allowed but changes the palette noticeably.

When it goes wrong: an unknown preset reports `unknown preset "midnight"; use glass-dark, glass-light, or minimal` and keeps `glass-dark`. `corner_radius = 100` reports `must be between 0 and 64 (got 100)` and keeps `18`. `spacing = 0.5` reports `must be between 0.75 and 2 (got 0.5)` and keeps `1`. `min_contrast = 30` reports `must be between 1 and 21 (got 30)` and keeps `4.5`. A colour without `#` reports `colour must start with '#'` and keeps the preset value.

### `[theme.blur]`

This section controls the backdrop blur and the colour pipeline that runs with it.

Backdrop blur follows Hyprland's `decoration:blur` block. The live window runs a full-resolution separable Gaussian on the GPU and does the whole colour pipeline in the same shader: the contrast and brightness prepare plus vibrancy in the horizontal pass, then grain and the final brightness in the vertical pass. The finished pixels are read back and fed to the glass cache. If the GPU path is unavailable, `--screenshot` and the fallback use a CPU dual Kawase pyramid with the same colour math.

```toml
[theme.blur]
enabled = true
size = 8
passes = 3
noise = 0.0117
contrast = 0.8916
brightness = 0.8172
vibrancy = 0.1696
vibrancy_darkness = 0.0
refresh_ms = 300
```

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` (glass-dark and glass-light), `false` (minimal) | Turn backdrop blur on or off. |
| `size` | number, 0 to 64 | `8` | Blur radius in logical pixels. Zero leaves the backdrop unblurred but still runs the colour pipeline. |
| `passes` | integer, 1 to 8 | `3` | Dual Kawase passes. More passes are wider and softer. |
| `noise` | number, 0 to 1 | `0.0117` | Per-pixel grain added after the blur. Zero disables the grain. |
| `contrast` | number, 0 to 2 | `0.8916` | Contrast gain applied before blurring. |
| `brightness` | number, 0 to 2 | `0.8172` | Final brightness multiplier. |
| `vibrancy` | number, 0 to 1 | `0.1696` | Saturation boost during the downsample. |
| `vibrancy_darkness` | number, 0 to 1 | `0.0` | Protects dark colours from the vibrancy boost. |
| `refresh_ms` | integer, 50 to 5000 | `250` for glass-dark and glass-light, `500` for minimal | How often an animated wallpaper refreshes the cached blur, in milliseconds. The bare `BlurConfig` fallback is `300`, but the shipped presets override it, so an unconfigured file resolves to the preset value. |

`ignore_opacity`, `new_optimizations`, `xray`, `special`, `popups`, and `popup_ignore_alpha` are accepted so a Hyprland blur block can be pasted directly, but they do not change a single full-screen opaque greeter. The blur is computed on a reduced copy of the background and cached; an animated wallpaper refreshes on the configured interval rather than once per frame. Turning blur off removes the expensive path and leaves the glass fill and grain.

When it goes wrong: `passes = 0` reports `must be between 1 and 8 (got 0)` and keeps `3`. `refresh_ms = 10` reports `must be between 50 and 5000 (got 10)` and keeps the preset value. `size = 100` reports `must be between 0 and 64 (got 100)` and keeps `8`. `contrast = 3` reports `must be between 0 and 2 (got 3)` and keeps `0.8916`.

## Decor item reference

This section controls every `[[login.decor]]` item, including its kind, geometry, colours, stroke, and motion. Items paint after the card surface and before the form, in list order, so a later item covers an earlier one and no item can cover an input or the submit button. The built-in clock and media widgets paint behind decor by default; set `layer = "front"` on the widget to paint it after the decor but still before the form. A front widget joins the card coordinate system: its anchor is the card, and its offset and size scale with the card just like decor. Images are decoded once at startup; a missing image is logged and skipped. Anchors are relative to the card. At most 32 items are supported.

### Shared keys

This section lists the keys shared by every decor kind.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `kind` | enum: `text`, `image`, `panel` | required | Selects the item. An unknown kind reports an issue and skips that item. |
| `anchor` | enum: the seven anchors | `"top-left"` | Anchor point inside the card. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Logical pixel offset from the anchor. |
| `rotation` | number, -360 to 360 | `0` | Rotate the item around its own centre. |
| `motion` | table | static | Per-item motion. See [Animated decor](#animated-decor). |

`opacity` is accepted by images and panels; text items ignore it and use only the motion fade.

### `kind = "image"`

This section controls image decor items.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `image` | path, at most 1024 characters | required | PNG, JPEG, WebP, or GIF. Relative paths resolve against the config file directory. A missing, non-regular, or oversized file reports an issue and skips the item. |
| `width` | number, 4 to 2000 | `64` | Display width in logical pixels. Height follows the image aspect ratio. |
| `anchor` | enum: the seven anchors | `"top-left"` | Card-relative anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Logical pixel offset. |
| `opacity` | number, 0 to 1 | `1` | Image opacity. |
| `rotation` | number, -360 to 360 | `0` | Rotation around the image centre. |

### `kind = "text"`

This section controls text decor items.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `text` | string, 1 to 256 characters | required | The run to draw. An empty string skips the item without an error. |
| `size` | number, 6 to 200 | `14` | Font size in logical pixels. |
| `color` | colour | theme primary text | Fill colour. |
| `bold` | bool | `false` | Bold run. |
| `tracking` | number, 0 to 0.5 | `0` | Letter spacing in em. |
| `stroke` | colour | unset | Optional outline colour. The outline draws the same run eight times around a small radius before the fill pass. |
| `stroke_width` | integer, 0 to 12 | `0` (resolved to `2` when `stroke` is set) | Outline radius in logical pixels. A stroke colour with width zero is deliberately promoted to `2` so the option never silently does nothing. |
| `anchor` | enum: the seven anchors | `"top-left"` | Card-relative anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Logical pixel offset. |
| `rotation` | number, -360 to 360 | `0` | Rotation around the run centre. |

### `kind = "panel"`

This section controls panel decor items. A panel is an anchored plate with no asset. It is useful for a HUD bar, a label backplate, or a radar blip. Panels paint in list order like every other decor item.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `width` | number, 4 to 4000 | `320` when missing (with an issue) or out of range (range error) | Panel width in logical pixels. |
| `height` | number, 4 to 4000 | `120` when missing (with an issue) or out of range (range error) | Panel height in logical pixels. |
| `color` | colour | theme `tint` | Fill colour. A translucent value is what makes a glass panel show the backdrop. |
| `color_end` | colour | unset | Second colour. When set, the plate becomes a linear gradient from `color` to this value. |
| `gradient_angle` | number, -360 to 360 | `0` | Gradient direction. 0 is left to right. |
| `radius` | number, 0 to 64 | `0` | Corner radius. |
| `border` | colour | unset | One-pixel stroke colour. |
| `glass` | bool | `false` | Paint the shared blurred backdrop under the fill. This is a backdrop blur, not a blur of the panel's own colour. |
| `opacity` | number, 0 to 1 | `1` | Plate opacity. |
| `anchor` | enum: the seven anchors | `"top-left"` | Card-relative anchor. |
| `offset` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Logical pixel offset. |
| `rotation` | number, -360 to 360 | `0` | Rotation around the panel centre. |

A panel ignores `image` and `text`; if either is present the panel is still drawn as a bare plate and one issue is reported. `blink_ms` works on a panel. `typer_ms`, `scramble_ms`, `wave`, `glitch`, and the ticker pair are text-only and do nothing to a panel.

With `glass = true` the panel paints the shared blurred backdrop under its fill. `color` still falls back to the theme tint, which is fairly opaque, so a glass panel normally sets an alpha colour:

```toml
[[login.decor]]
kind = "panel"
width = 420
height = 64
glass = true
color = "#10172655"
radius = 16
border = "#ffffff22"
anchor = "bottom-left"
```

### Animated decor

This section controls the optional motion subtable attached to one decor item.

Any decor item can grow a `[login.decor.motion]` subtable. The motions are additive, so one ornament can drift, sway, breathe, and fade at the same time. An item with no motion table stays still, and the whole system freezes when `[motion].reduced` or `[power].low_power` is set.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `spin` | number, -720 to 720 | `0` | Continuous rotation in degrees per second. |
| `sway` | number, 0 to 45 | `0` | Sine rotation amplitude in degrees. |
| `sway_ms` | number, 200 to 20000 | `6000` | Sway period. |
| `float` | `[x, y]`, each component -2048 to 4096 | `[0, 0]` | Elliptical drift amplitude in logical pixels. X uses sine, Y uses cosine. |
| `float_ms` | number, 200 to 30000 | `7000` | Drift period. |
| `pulse` | number, 0 to 0.5 | `0` | Scale amplitude as a fraction of the base size. |
| `pulse_ms` | number, 200 to 20000 | `2400` | Pulse period. |
| `fade` | number, 0 to 1 | `0` | Opacity depth. The item cycles between full and `1 - fade`. |
| `fade_ms` | number, 200 to 20000 | `3000` | Fade period. |
| `phase` | number, 0 to 1 | `0` | Phase offset in turns. Every oscillation subtracts its phase-only value, so a staggered row still starts at the neutral pose at `elapsed = 0`. |
| `typer_ms` | number, 0 to 20000 | `0` | Text only. One-shot character reveal over this many milliseconds. Zero is off. The text is empty at `elapsed = 0` and complete at `typer_ms`. |
| `blink_ms` | number, 0 to 20000 | `0` | Text and image. Square-wave blink period. Visible for the first half of each period, hidden for the second. Zero is off. |
| `ticker_ms` | number, 0 to 120000 | `0` | Text only. Time for the marquee strip to travel one tile. Zero is off. |
| `ticker_width` | integer, 0 to 4000 | `0` | Text only. Width of the marquee strip. Required when `ticker_ms` is positive. |
| `ticker_gap` | integer, 0 to 2000 | `64` | Text only. Space between repeated copies in the ticker. |
| `parallax_depth` | number, 0 to 1 | `0` | Pointer parallax strength for this item. `[motion].parallax` must also be positive, and the effect needs a live window with a pointer. |
| `enter` | enum: `none`, `fade`, `slide`, `scale` | `"none"` | One-shot entrance. Starts hidden at `elapsed = 0` and settles after `enter_delay_ms + enter_ms`. |
| `enter_ms` | number, 0 to 20000 | `600` | Entrance duration. |
| `enter_delay_ms` | number, 0 to 20000 | `0` | Delay before the entrance starts. |
| `enter_from` | enum: `left`, `right`, `top`, `bottom` | `"left"` | Start edge for `enter = "slide"`. Ignored by the other styles. |
| `wave` | number, 0 to 40 | `0` | Text only. Vertical per-character wave amplitude in logical pixels. Zero is off. A ticker keeps scrolling plain. |
| `wave_ms` | number, 200 to 20000 | `2400` | Wave period. |
| `wave_step` | number, 0 to 1 | `0.12` | Phase step between adjacent characters in turns. |
| `scramble_ms` | number, 0 to 20000 | `0` | Text only. Scramble-to-real reveal. Overrides `typer_ms` on the same item, and a ticker overrides both. At `elapsed = 0` the final text is shown. |
| `glitch` | number, 0 to 1 | `0` | Text only. Glitch strength. Two coloured copies appear for the first 8 percent of each period. Images ignore it. |
| `glitch_ms` | number, 200 to 20000 | `2000` | Glitch period. |

A typewriter starts empty, so a screenshot at `elapsed = 0` shows no text for that item. The `enter` styles are also one-shot and start hidden at `elapsed = 0`, so a screenshot captures the first frame of the entrance rather than the settled item. A ticker overrides the typewriter and the scramble on the same item, because the marquee already shows the full run. A ticker without a positive `ticker_width` is reported as a config issue.

The scramble reveal deliberately breaks the start-at-zero rule: at `elapsed = 0` it shows the final text, so a captured frame stays readable instead of showing noise. Live, it resolves from the head over `scramble_ms`; a positive `scramble_ms` overrides `typer_ms` on that item. The per-character wave ignores `stroke` because eight outline passes per character is a lot of pixels for an effect that already moves, and it does not apply to a ticker. A tickered item also ignores `stroke` and keeps the plain fill. Glitch is text-only: it draws deterministic red and cyan copies at opposite horizontal offsets for 8 percent of each `glitch_ms` period. If wave and glitch are both on, the coloured copies follow the same wave path.

`blink_ms` is the one clock-like field that works on both kinds: it hides a text run or an image during the second half of each period, which is how a strobing cursor sprite is made.

Parallax follows the pointer: the item shifts a few pixels toward the opposite side of the cursor. It needs a live window with a pointer position, so screenshots stay at the neutral offset. `[motion].parallax` is the global strength and every item still needs its own `parallax_depth`.

Motion runs inside the cached UI overlay. A moving ornament asks the renderer for a fresh overlay frame, so animated artwork should stay small and a handful of accents works better than a full-screen moving layer. Screenshots always capture the neutral pose at `elapsed = 0`, which makes visual regression checks stable.

When decor goes wrong: a missing `kind` reports `expected text, image, or panel` and skips the item. A text run above 256 characters reports `must be at most 256 characters` and skips the item. A panel without `width` or `height` reports `panel needs a whole number from 4 to 4000` and uses `320` by `120`. `ticker_ms = 3000` without `ticker_width` reports `ticker_ms needs ticker_width greater than 0` and the item does not scroll. A motion amplitude outside its range keeps that field's default while the rest of the motion table still applies.

## `[layout]`

This section controls dragging and where dragged positions are remembered.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `save` | bool | `false` | Remember dragged widget positions between runs. Off by default, because a greeter should not write files unless the administrator asked for it. |
| `file` | path, at most 1024 characters | state directory | Explicit layout file. A relative path resolves against the config directory. |

When `save = true` and `file` is unset, dawn tries `$XDG_STATE_HOME/dawn/layout.toml`, then `~/.local/state/dawn/layout.toml`, then `/var/lib/dawn/layout.toml`. The file is small, versioned, and written atomically with mode `0600`. A missing or corrupt file is ignored; dragging is decoration, never a login dependency.

The saved keys are stable widget ids: `login`, `clock`, `media`, and `<plugin-id>:<index>` for plugin widgets. Built-in ids do not shift when one widget is disabled.

When it goes wrong: an empty `file` reports `path must not be empty` and falls back to the state directory. A path with a control character reports `path must not contain control characters`. If the process cannot create the state directory, the save fails with a warning and the form itself keeps working.

## `[sessions]`

This section controls which directories are scanned for `.desktop` sessions and which session is selected first.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `dirs` | array of paths, at most 8 entries, at least 1 | `["/usr/share/wayland-sessions", "/usr/share/xsessions"]` | Directories scanned for `.desktop` sessions. An empty list is rejected. Relative paths resolve against the config directory. |
| `default` | string, at most 128 characters | unset | Desktop file id to preselect, for example `"hyprland"`. An unknown id falls back to the first session. |

Sessions are sorted by display name. `TryExec` is honoured, `NoDisplay` and `Hidden` entries are skipped, and duplicate ids keep the first directory's version. The greeter does not invent `XDG_SESSION_TYPE`; the session launcher knows whether it is Wayland or X11.

When it goes wrong: an empty `dirs` list reports `must contain at least one directory` and keeps the defaults. More than eight entries reports `at most 8 directories are supported` and keeps the defaults. A non-string entry reports `every entry must be a string`. A `default` id that matches nothing is not an error; the first sorted session is used.

## `[plugins]`

This section controls plugin discovery.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Set to `false` to skip plugin discovery entirely. |
| `dirs` | array of paths, at most 8 entries | `["/usr/share/dawn/plugins", "$HOME/.config/dawn/plugins"]` | Directories containing one subdirectory per plugin. Relative paths resolve against the config directory. An empty list is allowed and means no external plugins. |

Plugin directories may be missing. A broken plugin is logged and skipped without affecting the login form or other plugins. See [plugins.md](plugins.md) for the widget API, layers, and manifest schema.

When it goes wrong: more than eight entries reports `at most 8 directories are supported` and keeps the defaults. A non-string entry reports `every entry must be a string`. A `plugin.toml` that fails validation appears under `plugin errors:` in `--check-config` and does not stop any other plugin.

## `[auth]`

This section controls the greetd connection. A normal greetd launch sets `GREETD_SOCK` and needs no config here.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `socket` | path, at most 1024 characters | unset (use `GREETD_SOCK`) | Override the greetd socket. Intended for a fake greetd during development. |
| `timeout_secs` | integer, 5 to 600 | `120` | Read and write timeout for greetd requests, in seconds. |

When it goes wrong: `timeout_secs = 2` reports `must be between 5 and 600 (got 2)` and keeps `120`. An empty socket reports `path must not be empty` and falls back to `GREETD_SOCK`. A socket that does not exist is a runtime error, not a config error; the login still shows and the failure is logged.

## `[power]`

This section controls the low-power switch.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `low_power` | bool | `false` | Freeze wallpaper animation and disable motion. Intended for battery-powered or remote greeter hardware. It never affects login responsiveness. |

`low_power = true` also freezes decor motion, widget motion, the ticker, particles, and the moving scanline band. Static scanlines stay.

## `[gpu]`

This section controls the live window's GPU compositor. `--screenshot` and the CPU compositor always run on tiny-skia, so a GPU setting can never change the pixels in a screenshot.

| Key | Type and range | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Set to `false` to use softbuffer without touching a driver. `DAWN_GPU=0` is the environment equivalent. |
| `backend` | enum: `auto`, `vulkan` (alias `vk`), `gl` (aliases `opengl`, `gles`), `software` (alias `sw`) | `"auto"` | `auto` enables Vulkan and GL. `vulkan` or `gl` picks one. `software` keeps the GPU pipeline but restricts it to CPU adapters such as lavapipe or llvmpipe. |
| `adapter` | string, at most 64 characters: `auto`, `index:N`, or a case-insensitive substring of the adapter name or driver | `"auto"` | Adapter choice. `index:N` numbers the adapters that match the backend and device filter, in the same order `dawn --list-gpus` prints them, before any power ranking. |
| `device` | enum: `auto`, `discrete`, `integrated`, `virtual`, `cpu` (alias `software`) | `"auto"` | Adapter class filter. A non-auto value is a hard filter, not a hint. `backend = "software"` forces a CPU adapter. |
| `power` | enum: `low`, `high`, `default` | `"low"` | Ranking for `adapter = "auto"`. `low` prefers integrated, then other, virtual, discrete, and finally CPU. `high` prefers discrete first and can raise heat and battery drain. `default` keeps the backend enumeration order. This is a preference, not a filter. |
| `present` | enum: `fifo` (alias `vsync`), `mailbox`, `immediate` (alias `tearing`) | `"fifo"` | Swapchain presentation mode. `fifo` queues frames with vsync-style pacing: tear-free, but it can add up to one frame of latency. `mailbox` replaces the queued frame when offered: tear-free and lower latency. `immediate` presents as soon as possible and may tear. A mode the surface does not offer is ignored with a warning. |
| `fallback` | enum: `chain`, `software`, `cpu`, `none` | `"chain"` | What to try when the selected adapter cannot produce a device or surface. `chain` tries the configured adapter, then a software adapter, then softbuffer. `software` stops after the software adapter. `cpu` skips the GPU. `none` sends a failure to the terminal fallback. |

`index:N` is checked for syntax at load time. An index that does not exist at runtime produces a readable error and follows `fallback`; it is not a config error. A name that matches nothing at runtime also falls through the same way, because the adapter may only exist on the machine that actually runs the greeter. Run `dawn --list-gpus` to see the indices, names, backends, device classes, and which one the config selects.

### Host-only rule

`[gpu]` always comes from the host config, even when `--theme ID` replaces the rest of the file. A theme bundle cannot change which adapter the greeter uses. When `--config PATH` is passed without a theme, that file is the host config and its `[gpu]` table applies. `DAWN_GPU` is applied after the host table is chosen: `0` or `cpu` disables the GPU, `vulkan`, `gl`, or `software` selects a backend, `auto` resets `adapter` to `"auto"`, and any other value is treated as an adapter name.

When it goes wrong: `backend = "metal"` reports `unknown value "metal"; use auto, vulkan, gl, or software` and keeps `auto`. `device = "apu"` reports `unknown value "apu"; use auto, discrete, integrated, virtual, or cpu` and keeps `auto`. `power = "max"` reports `unknown value "max"; use low, high, or default` and keeps `low`. `present = "fast"` reports `unknown value "fast"; use fifo, mailbox, or immediate` and keeps `fifo`. `fallback = "retry"` reports `unknown value "retry"; use chain, software, cpu, or none` and keeps `chain`. See [troubleshooting.md](troubleshooting.md) for a black screen with the GPU compositor.

## Full examples

- `dawn init` writes the built-in default config, commented, to `/etc/dawn/config.toml`.
- [../examples/showcase-aurora.toml](../examples/showcase-aurora.toml): a cool-toned showcase with a custom wallpaper.
- [../examples/showcase-ember.toml](../examples/showcase-ember.toml): the warm counterpart, same layout with a plum and coral palette.
- [../examples/showcase-gif.toml](../examples/showcase-gif.toml): the same layout with an animated GIF wallpaper.
- [../examples/showcase-video.toml](../examples/showcase-video.toml): the same layout with an MP4 live wallpaper. Requires `ffmpeg`.
- [../examples/themes/hutao-chibi](../examples/themes/hutao-chibi): a cardless terminal layout with line fields, ambient decor, and a video wallpaper.
- [../examples/themes/fischl](../examples/themes/fischl): a banner-led royal layout with a crest, crescent moon, and staggered stars.
- [../examples/themes/genshin](../examples/themes/genshin): a card-free hero layout with a hydro sigil and an open gold frame.
- [../examples/themes/furina](../examples/themes/furina): an opera playbill layout with a curtain, spotlight, droplets, and an applause meter.
- [../examples/themes/kinich](../examples/themes/kinich): an 8-bit tactical HUD with pixel brackets, drifting leaves, and a status bar.
- [../examples/themes/hutao-montagem](../examples/themes/hutao-montagem): the AFTER DARK stage-call poster: no card, an inline poster headline, hairline credential rules, a support-act marquee, and a butterfly success burst.
- [../examples/wallpapers](../examples/wallpapers): the showcase wallpapers, with editable SVG sources next to the rendered WebP files.
- [../examples/snippets/glass-light.toml](../examples/snippets/glass-light.toml): a light theme block to merge into a config.
- [../examples/plugins/hello-world/plugin.toml](../examples/plugins/hello-world/plugin.toml): a minimal widget plugin.
