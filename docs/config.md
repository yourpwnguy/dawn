# dawn configuration

dawn reads one TOML file at startup. The search order is:

1. `--config PATH`
2. `$XDG_CONFIG_HOME/dawn/config.toml`
3. `~/.config/dawn/config.toml`
4. `/etc/dawn/config.toml`

If no file exists, the built-in defaults below are used. Relative paths inside
the file are resolved against the directory that contains it, and a leading
`~/` is expanded using `HOME`.

Unknown keys, wrong types, and out-of-range values never stop a login. Each bad
field falls back to its default, the rest of the file still applies, and the
login screen shows a short warning. Run `dawn --check-config` to see the exact
file, line, and column for every problem.

## Shape of a config

The format is TOML, and there is still only one file. The sections are grouped
by what they affect, not by implementation detail:

- `[ui]`, `[theme]`, and `[motion]` hold global tokens.
- `[login]` holds the card, its fields, and every visible string in the form.
- `[clock]` and `[media]` are the first-party overlay widgets.
- `[background]` selects a solid, gradient, image, GIF/WebP, or video wallpaper.
- `[layout]` controls dragging and where positions are remembered.
- `[sessions]`, `[plugins]`, `[auth]`, `[gpu]`, and `[power]` are runtime concerns.

Every key is optional, and a section only needs the values you want to change.
That is the rule that keeps a complete-looking config short: write
`[login.username] placeholder = "User"` and every size, colour, and caret
detail inherits its default. The renderer never requires a full table.

## `[ui]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `font` | string | `"Inter"` | Font family name. An unknown family quietly falls back to the embedded Inter face. |
| `font_size` | number | `17` | Base size in logical pixels, 10 to 72. |
| `card_width` | number | `380` | Legacy alias for `login.width`. Still accepted. |
| `fullscreen` | bool | `true` | Ask the compositor for a fullscreen window. `--fullscreen` forces this on. |
| `greeting` | string | `""` | Legacy alias for `login.greeting`. Still accepted. |

## `[login]`

This section is the whole form. It controls where the card sits, how big it is,
what every field says, and how each field looks. Values left out inherit from
`[login.field]`, then the theme, then the built-in defaults.

There are two layouts. `card` is the compact, content-sized glass panel.
`hero` is a fixed-size website-style panel: the form sits at `form_offset`, the
panel can fade into the wallpaper on one side, and the submit button and
decorative lockups are available.
`examples/themes/fischl/config.toml` is a complete hero example.

```toml
# Compact card
[login]
greeting = "Welcome back"
width = 416
draggable = true

[login.username]
placeholder = "Username"
label = "Account"

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

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `layout` | string | `"card"` | `card` or `hero`. |
| `anchor` | string | `"center"` | `top-left`, `top-center`, `top-right`, `center`, `bottom-left`, `bottom-center`, or `bottom-right`. |
| `offset` | `[x, y]` | `[0, 0]` | Logical pixel offset from the anchor. |
| `monitor` | string | unset | Output name restriction. |
| `width` | number | `380` | Card width, 260 to 2400, clamped to fit the screen. |
| `height` | number | `0` | Fixed height. Zero derives it from the content. `hero` uses this. |
| `padding` | number | `26` | Card inner padding, 0 to 80. |
| `gap` | number | `14` | Vertical gap, 0 to 48. |
| `radius` | number | theme corner radius | Card corner radius, 0 to 64. |
| `panel` | colour | theme tint | Fill colour for `surface = "flat"` and the hero panel. |
| `panel_fade` | `[start, end]` | `[1.0, 1.0]` | Hero panel fade as fractions of the card width. Before `start` the wallpaper shows through. |
| `border` | colour | theme border | Hero card outline. |
| `surface` | string | `"glass"` | `glass`, `flat`, or `none`. `none` hides the panel, border, and shadow entirely, which is how you float the form directly on the wallpaper. |
| `show_shadow` | bool | `true` | Draw the card shadow. Ignored by `surface = "none"`. |
| `show_border` | bool | `true` | Draw the card outline. Ignored by `surface = "none"`. |
| `rotation` | number | `0` | Rotate the whole card, including its form and decor, in degrees (-360 to 360). |
| `tab` | string | `""` | Small top-left tab label, for example `"Login"`. Empty hides it. |
| `form_offset` | `[x, y]` | `[0, 0]` | Hero form position from the card's top-left. |
| `form_width` | number | `0` | Hero form width. Zero uses the card padding. |
| `draggable` | bool | `false` | Let the pointer move the whole card. Clicking a field still focuses it. |
| `greeting` | string | `""` | Line above the fields. Empty hides it. |
| `show_session` | bool | `true` | Alias for `login.session.show`. The specific key wins if both are set. |
| `show_hints` | bool | `true` | Draw the keyboard hint line. |
| `show_picker` | bool | `true` | Allow the session picker to open. Disable it for a single-session kiosk. |
| `picker_title` | string | `"Choose a session"` | Picker heading. Empty hides it. |
| `busy_text` | string | `"Signing in..."` | Used when `login.hints.busy` is empty. |
| `dry_run_badge` | string | `"DRY RUN"` | Badge text in `--dry-run`. |
| `show_dry_run_badge` | bool | `true` | Hide the badge if you prefer a clean preview. |
| `no_sessions` | string | built-in message | Status line when no `.desktop` session is found. |

### `[login.field]`, `[login.username]`, `[login.password]`

`[login.field]` is the shared style. The username and password tables inherit
it and can override every key. A zero in `font_size`, `height`, `radius`,
`padding`, or `caret_width` means "derive it from `ui.font_size`".

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `label` | string | `""` | Optional label above the field. Empty hides it. |
| `label_color` | colour | theme muted | Label colour. Useful when the label should carry the accent while the placeholder stays quiet. |
| `placeholder` | string | `"Username"` / `"Password"` | Shown when empty. |
| `font_size` | number | `0` (inherit) | Field text size, 0 to 96. |
| `height` | number | `0` (derive) | Field height, 0 to 160. |
| `radius` | number | `0` (derive) | Field corner radius, 0 to 64. |
| `padding` | number | `0` (derive) | Horizontal text inset, 0 to 64. |
| `background` | colour | theme tint | Field fill. |
| `background_end` | colour | unset | Second fill colour. When set, the fill becomes a horizontal gradient. |
| `border` | colour | theme border | Unfocused border. Ignored when `underline` is set. |
| `underline` | colour | unset | Draw a rule at the bottom of the field instead of a full box. Focus thickens the rule and adds a soft underglow. |
| `underline_size` | number | `0` (derive) | Rule thickness in logical pixels, 0 to 12. Zero derives one pixel. |
| `bracket` | colour | unset | Broken targeting bracket around the field. `None` hides it. |
| `bracket_gap` | number | `0` | How far the bracket sits outside the field, 0 to 64. |
| `text` | colour | theme text | Typed text colour. |
| `placeholder_color` | colour | theme muted | Placeholder colour. |
| `focus` | colour | theme focus ring | Focus border and glow. |
| `caret` | colour | theme accent | Caret colour. |
| `caret_width` | number | `0` (derive) | Caret width, 0 to 16. Zero derives a bar or a wider block from the text size. |
| `caret_style` | string | `"bar"` | `bar` for the thin text caret, `block` for a filled character cell. |
| `icon` | string | `""` | Leading glyph: `user`, `lock`, or `none`. |
| `icon_size` | number | `0` (derive) | Icon size, 0 to 64. |
| `icon_gap` | number | `0` (derive) | Gap between the icon and the text, 0 to 64. |
| `icon_color` | colour | theme text | Icon colour. |
| `rotation` | number | `0` | Rotate the field, its label, and its bracket, in degrees (-360 to 360). |
| `mask` | string | `"•"` (password) | Password glyph. The first character is used. |

### `[login.button]`

The submit button is pointer-clickable and sends the same Enter key as the
keyboard. It is off by default, so a classic card stays keyboard-only.

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

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `show` | bool | `false` | Draw the button. |
| `text` | string | `"Sign in"` | Button copy. |
| `gap` | number | `16` | Space above the button, 0 to 160. |
| `height` | number | `44` | Button height, 20 to 160. |
| `radius` | number | `8` | Corner radius, 0 to 64. |
| `font_size` | number | `0` (inherit) | Button text size, 0 to 96. |
| `background` | colour | theme accent | Button fill. |
| `background_end` | colour | unset | Second gradient stop. |
| `gradient_angle` | number | `0` | Gradient direction in degrees. 0 is left to right, 90 top to bottom. |
| `text_color` | colour | theme text | Button text. |
| `bracket` | colour | unset | Broken bracket around the button. |
| `bracket_gap` | number | `0` | Bracket offset, 0 to 64. |
| `bold` | bool | `false` | Bold button text. |
| `tracking` | number | `0` | Letter spacing in em, 0 to 0.5. |
| `hover_background` | colour | lighter fill | Fill after the pointer enters. |
| `hover_background_end` | colour | lighter end stop | Second stop while hovered. |
| `hover_text_color` | colour | `text_color` | Text colour while hovered. |
| `hover_direction` | string | `"fade"` | `fade`, `left`, `right`, `up`, `down`, or `center`. Directional modes paint the hover fill in from that edge. |
| `hover_ms` | number | `180` | Hover colour/geometry transition, 0 to 5000 ms. |
| `hover_scale` | number | `1.0` | Uniform scale while hovered, 0.5 to 2.0. |
| `hover_lift` | number | `0` | Upward travel while hovered, 0 to 64 px. |
| `glow` | colour | unset | Outer glow while hovered. |
| `glow_size` | number | `0` | Glow spread, 0 to 64 px. 0 disables it. |
| `effect` | string | `"none"` | `none`, `sweep`, `sheen`, `pulse`, `glow`, or `corners`. `shine` is an alias for `sheen`. |
| `effect_color` | colour | white | Effect colour. |
| `effect_ms` | number | `1400` | One full effect cycle, 100 to 20000 ms. |
| `effect_angle` | number | `120` | Sweep/sheen band direction in degrees. |
| `effect_strength` | number | `0.25` | Effect alpha, 0 to 1. |
| `rotation` | number | `0` | Rotate the button in degrees (-360 to 360). |

`sweep` moves a wide highlight across the face, `sheen` is the narrow
collectible-card version, `pulse` breathes the fill, `glow` breathes the outer
glow, and `corners` draws four expanding target marks. Effects run only while the pointer is over the button;
`[motion].reduced` and `[power].low_power` skip them.

### `[[login.decor]]`

Decor places a logo, social button, footer line, panel, or other cosmetic item
inside the card. Items paint after the card surface and before the form, in
list order, so a later item covers an earlier one and no item can cover an
input or the submit button. The built-in clock and media widgets paint behind
decor by default; set `layer = "front"` on `[clock]` or `[media]` to paint them
after the decor but still before the form. A plugin widget chooses its own
`layer` in the manifest and follows the same rule. A front widget joins the
card's coordinate system: its anchor is the card, and its offset and size scale
with the card just like decor. Images are decoded once at startup; a missing
image is logged and skipped. Anchors are relative to the card.

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

Image keys: `image` (required, relative to the config file), `width` (4 to
2000), `anchor`, `offset`, `opacity` (0 to 1), and `rotation` (-360 to 360).
Text keys: `text` (1 to 256 characters), `size` (6 to 200), `color`, `bold`,
`tracking`, `stroke` (optional outline colour), `stroke_width` (0 to 12,
default 0 when no stroke is set; a stroke with width 0 is resolved to 2),
`anchor`, `offset`, and `rotation`. The outline draws the same run eight times
around a small radius before the fill pass, and the typewriter reveal applies to
both. A tickered item ignores `stroke` and keeps the plain fill. At most 32
decor items are supported.

#### Panels

`kind = "panel"` draws an anchored plate with no asset. It is useful for a HUD
bar, a label backplate, or a radar blip. Panels paint in list order like every
other decor item, so a panel covers items earlier in the list and is covered by
items later in the list.

```toml
[[login.decor]]
kind = "panel"
width = 420
height = 48
color = "#17191CE6"
color_end = "#17191C00"
gradient_angle = 0
radius = 2
border = "#E7DA1966"
opacity = 1.0
anchor = "bottom-left"
offset = [0, 0]
```

Panel keys: `width` and `height` (required, 4 to 4000), `color` (falls back to
`[theme].tint`), `color_end` (optional second colour; when set the plate becomes
a linear gradient from `color` to `color_end`), `gradient_angle` (-360 to 360,
0 is left to right), `radius` (0 to 64, default 0), `border` (optional one-pixel
stroke colour), `opacity` (0 to 1), plus `anchor`, `offset`, `rotation`, and the
usual motion table. A panel ignores `image` and `text`; if either is present the
panel is still drawn as a bare plate and one issue is reported.

`blink_ms` turns a panel into a radar blip by hiding the whole plate for the
second half of each period. `typer_ms`, `scramble_ms`, `wave`, `glitch`, and the
ticker pair are text-only and do nothing to a panel.

#### Animated decor

Any decor item can grow a `[login.decor.motion]` subtable. The motions are
additive, so one ornament can drift, sway, breathe, and fade at the same time.
An item with no motion table stays still, and the whole system freezes when
`[motion].reduced` or `[power].low_power` is set.

```toml
[[login.decor]]
kind = "image"
image = "spark.png"
width = 48
anchor = "top-left"
offset = [120, 600]

[login.decor.motion]
float = [10, -18]   # elliptical drift in logical pixels
float_ms = 6200
sway = 6.0          # sine rotation in degrees
sway_ms = 4600
spin = 8.0          # continuous rotation in degrees per second
pulse = 0.05        # scale breathing, 0 to 0.5
pulse_ms = 2500
fade = 0.35         # opacity depth, 0 to 1
fade_ms = 4200
phase = 0.25        # cycle offset in turns, 0 to 1
```

| Key | Default | Notes |
| --- | --- | --- |
| `spin` | `0` | Continuous rotation in degrees per second, -720 to 720. |
| `sway` | `0` | Sine rotation amplitude in degrees, 0 to 45. |
| `sway_ms` | `6000` | Sway period, 200 to 20000 ms. |
| `float` | `[0, 0]` | Elliptical drift amplitude in logical pixels. X uses sine, Y uses cosine. |
| `float_ms` | `7000` | Drift period, 200 to 30000 ms. |
| `pulse` | `0` | Scale amplitude as a fraction of the base size, 0 to 0.5. |
| `pulse_ms` | `2400` | Pulse period, 200 to 20000 ms. |
| `fade` | `0` | Opacity depth, 0 to 1. The item cycles between full and `1 - fade`. |
| `fade_ms` | `3000` | Fade period, 200 to 20000 ms. |
| `phase` | `0` | Phase offset in turns. Every oscillation is subtracted at its phase-only value, so `phase` staggers a row without moving the `elapsed = 0` pose. |
| `typer_ms` | `0` | Text only. One-shot character reveal over this many milliseconds, 0 to 20000. Zero is off. The text is empty at `elapsed = 0` and complete at `typer_ms`. |
| `blink_ms` | `0` | Text and image. Square-wave blink period, 0 to 20000. Visible for the first half of each period, hidden for the second. Zero is off. |
| `ticker_ms` | `0` | Text only. Time for the marquee strip to travel one tile, 0 to 120000. Zero is off. |
| `ticker_width` | `0` | Text only. Width of the marquee strip in logical pixels, 0 to 4000. Required when `ticker_ms` is set. |
| `ticker_gap` | `64` | Text only. Space between repeated copies in the ticker, 0 to 2000. |
| `parallax_depth` | `0` | Pointer parallax strength for this item, 0 to 1. `[motion].parallax` must also be positive, and the effect needs a live window with a pointer. |
| `enter` | `"none"` | One-shot entrance: `none`, `fade`, `slide`, or `scale`. Starts hidden at `elapsed = 0` and settles after `enter_delay_ms + enter_ms`. |
| `enter_ms` | `600` | Entrance duration, 0 to 20000 ms. |
| `enter_delay_ms` | `0` | Delay before the entrance starts, 0 to 20000 ms. |
| `enter_from` | `"left"` | Start edge for `enter = "slide"`: `left`, `right`, `top`, or `bottom`. |
| `wave` | `0` | Text only. Vertical per-character wave amplitude in logical pixels, 0 to 40. Zero is off. A ticker keeps scrolling plain. |
| `wave_ms` | `2400` | Wave period, 200 to 20000 ms. |
| `wave_step` | `0.12` | Phase step between adjacent characters in turns, 0 to 1. |
| `scramble_ms` | `0` | Text only. Scramble-to-real reveal, 0 to 20000 ms. Overrides `typer_ms` on the same item, and a ticker overrides both. At `elapsed = 0` the final text is shown. |
| `glitch` | `0` | Text only. Glitch strength, 0 to 1. Two coloured copies appear for the first 8 percent of each period. Images ignore it. |
| `glitch_ms` | `2000` | Glitch period, 200 to 20000 ms. |

A typewriter starts empty, so a screenshot at `elapsed = 0` shows no text for that item. The `enter` styles are also one-shot and start hidden at `elapsed = 0`, so a screenshot captures the first frame of the entrance rather than the settled item. A ticker overrides the typewriter and the scramble on the same item, because the marquee already shows the full run. A ticker without a positive `ticker_width` is reported as a config issue.

The scramble reveal deliberately breaks the "start at zero" rule: at `elapsed = 0` it shows the final text, so a captured frame stays readable instead of showing noise. Live, it resolves from the head over `scramble_ms`; a positive `scramble_ms` overrides `typer_ms` on that item. The per-character wave ignores `stroke` because eight outline passes per character is not worth the cost, and it does not apply to a ticker. Glitch is text-only: it draws deterministic red and cyan copies at opposite horizontal offsets for 8 percent of each `glitch_ms` period. If wave and glitch are both on, the coloured copies follow the same wave path.

`blink_ms` is the one clock-like field that works on both kinds: it hides a text run or an image during the second half of each period, which is how you get a strobing cursor sprite.

```toml
[[login.decor]]
kind = "text"
text = "Welcome back, pilot"
anchor = "top-left"
offset = [24, 18]

[login.decor.motion]
typer_ms = 1600
blink_ms = 900

[[login.decor]]
kind = "text"
text = "SYSTEM ONLINE // SECTOR 7 // ALL SERVICES NOMINAL"
size = 11
anchor = "bottom-left"
offset = [24, 18]

[login.decor.motion]
ticker_ms = 3000
ticker_width = 300
ticker_gap = 80
```

The motion pack combines an entrance, pointer parallax, a per-character wave,
a scramble reveal, and a glitch:

```toml
[motion]
parallax = 1.0

[[login.decor]]
kind = "text"
text = "INCOMING TRANSMISSION"
size = 18
anchor = "top-left"
offset = [32, 40]

[login.decor.motion]
parallax_depth = 0.6
enter = "slide"
enter_from = "left"
enter_ms = 700
enter_delay_ms = 120
wave = 6
wave_ms = 2400
wave_step = 0.12
scramble_ms = 1800
glitch = 0.5
glitch_ms = 2200
```

Parallax follows the pointer: the item shifts a few pixels toward the opposite
side of the cursor. It needs a live window with a pointer position, so
screenshots stay at the neutral offset. `[motion].parallax` is the global
strength and every item still needs its own `parallax_depth`.

Motion runs inside the cached UI overlay. A moving ornament therefore asks the
renderer for a fresh overlay frame; keep animated artwork small and prefer a
handful of accents over a full-screen moving layer. Screenshots always capture
the neutral pose at `elapsed = 0`, which makes visual regression checks stable.

### `[login.session]` and `[login.hints]`

| Key | Default | Notes |
| --- | --- | --- |
| `session.label` | `"Session"` | Session row label. Empty hides the label. |
| `session.change_hint` | `"Tab to change"` | Shown when more than one session exists. |
| `session.empty` | `"No sessions found"` | Shown when discovery returned nothing. |
| `session.show` | `true` | Draw the row at all. |
| `hints.username` | built-in | Hint line while the username is focused. |
| `hints.password` | built-in | Hint line while the password is focused. |
| `hints.picker` | built-in | Hint line while the picker is open. |
| `hints.busy` | `""` | Hint while signing in. Empty falls back to `login.busy_text`. |

## `[clock]`

The clock is a first-party widget. It uses the same widget model, placement
rules, and renderer as a plugin clock.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Draw the clock. |
| `layer` | string | `"behind"` | `behind` paints in screen space before decor; `front` paints after decor but before the form and places the widget in card space, so both its offset and size scale with the card. Plugin widgets choose their own layer in the manifest. |
| `style` | string | `"digital"` | `digital`, `analog`, `flip`, or `segment`. For analog, `size` is the dial diameter. `flip` and `segment` use the same time string as `digital`. |
| `time_format` | string | `"%H:%M"` | strftime pattern, validated at load time. Invalid patterns fall back. |
| `date_format` | string | `"%A, %B %d"` | Empty hides the date line. |
| `hour12` | bool | `false` | Rewrites `%H` to `%I` and appends `%p`. An explicit `%I`, `%l`, or `%r` wins. |
| `seconds` | bool | `false` | Adds seconds when the format does not already contain `%S`. |
| `timezone` | string | unset | IANA zone such as `"Europe/Berlin"`. Unset uses the system zone. |
| `posix_locale` | bool | `false` | Use the POSIX/C locale for `%c`, `%x`, `%X`, and day/month names. |
| `anchor` | string | `"top-center"` | One of the anchors listed under `[login]`. |
| `offset` | `[x, y]` | `[0, 48]` | Logical pixel margin from the anchor. |
| `monitor` | string | unset | Output name, for example `"DP-1"`. Unset shows the widget on every output. |
| `rotation` | number | `0` | Rotate the clock in degrees (-360 to 360). |
| `size` | number | `54` | Digital font size or analog diameter, 12 to 180. |
| `date_size` | number | `18` | Date font size, 8 to 48. |
| `color` | colour | theme text | Optional clock colour override. |
| `date_color` | colour | theme muted | Optional date colour override. |
| `ghost` | bool | `false` | `segment` only: draw the unused segments at low alpha, like an unlit LCD. |
| `glow` | colour | unset | `segment` only: halo colour for the active segments. |
| `glow_size` | number | `0` | Halo spread in logical pixels, 0 to 32. Zero disables it even when `glow` is set. |
| `colon_blink_ms` | number | `0` | Full on/off period for the colon, 0 to 5000 ms. Zero is steady. Applies to `flip` and `segment`. |
| `second_hand` | string | `"tick"` | `analog` only: `tick` jumps once per second, `sweep` follows the subsecond clock. |
| `draggable` | bool | `true` | Let the pointer move the clock. Saved by `[layout]`. |
| `plate` | table | see below | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array | empty | States that may show the clock. Empty means every state. |
| `hide_on` | array | empty | States that hide the clock. Wins over `show_on`. |

Formats use [jiff's strftime implementation](https://docs.rs/jiff/latest/jiff/fmt/strtime/index.html),
which covers the usual `%H`, `%M`, `%S`, `%A`, `%B`, `%d`, and `%Z` specifiers.
Day and month names are English by default; `posix_locale = true` selects the
POSIX/C forms. There is no full per-locale translation layer in v0.2.0.

The clock asks for a redraw only when its visible strings change. A minute clock
does not wake the renderer sixty times a minute.

The flip face paints one split-flap card per character, using the widget plate
as the card material. It is a good fit for a compact HUD clock:

```toml
[clock]
style = "flip"
time_format = "%H:%M"
seconds = false
size = 42
color = "#F5F3FF"
colon_blink_ms = 1000
```

The segment face draws real seven-segment digits. `ghost` shows the unlit
segments, and `glow` adds a halo behind the active ones:

```toml
[clock]
style = "segment"
size = 48
color = "#22D3EE"
ghost = true
glow = "#22D3EE"
glow_size = 6
```

`layer = "front"` paints the clock after the login decor and before the form.
Its placement then uses the card's coordinate system, exactly like decor: the
anchor is relative to the card, and both `offset` and `size` scale with the
card. That keeps the digits inside a HUD bar that shrank on a smaller display.
The form still paints on top. Plugin clock widgets choose their own `layer` in
the manifest; when it is `front` they join the same card-space frame.

## `[media]`

The media widget is also a first-party widget. It polls MPRIS on the greeter's
session bus on a dedicated thread, so a slow or broken player cannot block the
UI or the greetd socket.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Show the media widget. |
| `layer` | string | `"behind"` | `behind` paints in screen space before decor; `front` paints after decor but before the form and places the widget in card space, so both its offset and size scale with the card. Plugin widgets choose their own layer in the manifest. |
| `anchor` | string | `"bottom-left"` | Placement anchor. |
| `offset` | `[x, y]` | `[32, 32]` | Logical pixel margin. |
| `monitor` | string | unset | Output name restriction. |
| `rotation` | number | `0` | Rotate the media widget in degrees (-360 to 360). |
| `size` | number | `16` | Text size, 8 to 48. |
| `show_art` | bool | `true` | Show local `file://` album art. Network URLs are never fetched. |
| `layout` | string | `"card"` | `card` (art left, text right), `compact` (one tight row), or `vinyl` (circle with centred text). |
| `art` | string | `"cover"` | `cover` keeps the square crop, `disc` masks it into a circle at load time, `none` hides it. |
| `progress` | string | `"line"` | `line`, `segments` (twelve blocks), or `wave` (a decorative sine). The wave is deterministic and does not use audio data. |
| `controls` | bool | `false` | Draw previous, play/pause, and next buttons. They are clickable once the widget is showing real metadata; a rotated widget keeps the glyphs but ignores clicks. |
| `tint_from_art` | bool | `false` | Derive the progress colour and plate border/glow from the dominant album-art colour, falling back to the theme accent. |
| `color` | colour | theme text | Optional title colour override. |
| `show_before_login` | bool | `true` | Draw the last visible track before authentication. Set `false` on shared machines where even the track name is private. |
| `keep_last` | bool | `true` | Keep the last track when the player pauses, stops, or disappears. The widget labels it `Last played`. |
| `draggable` | bool | `true` | Let the pointer move the media widget. |
| `plate` | table | see below | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array | empty | States that may show the media widget. Empty means every state. |
| `hide_on` | array | empty | States that hide the media widget. Wins over `show_on`. |

The greeter runs before the user's session exists. In practice the user's player
is not on the bus dawn can reach, so the widget shows a calm `No media` state.
If a player is visible and answers, dawn keeps the last useful snapshot instead
of wiping the widget when the player goes away. On a shared machine, set
`show_before_login = false` and the renderer keeps metadata off the login pixels
until greetd accepts authentication.

`layer = "front"` works the same way as it does for the clock: media paints after
decor and before the form, using the card's coordinate system so its offset and
size scale with the card. A plugin media widget uses its manifest `layer`, so it
can also paint in the front layer while the form still stays on top.

The default card layout is the original composition. A compact corner widget
looks like this:

```toml
[media]
layout = "compact"
art = "cover"
progress = "line"
size = 15
```

The vinyl layout masks the cover into a disc once at load time and draws the
line progress as a ring around it. `tint_from_art` takes the dominant cover
colour for the ring and the plate border:

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

`controls = true` adds a transport row under the progress. Once the widget is
showing real metadata, the three glyphs are real buttons: clicking one asks the
MPRIS worker to send Previous, PlayPause, or Next to the player it is already
polling. A rotated media widget still draws the row, but ignores clicks so the
hit regions stay honest. The buttons also do nothing in the `No media` state,
because there is no player to talk to.

#### Widget plates

`[clock.plate]` and `[media.plate]` replace the fixed scrim with a themed
backplate. The defaults reproduce the old pill exactly: a glass tint, a 14 px
radius, 12 px padding, full opacity, and no glow or shadow. The plate wraps the
entire widget, including album art and the progress bar, and it scales with the
output just like the widget content.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `style` | string | `"tint"` | `none`, `tint`, `solid`, or `gradient`. `none` draws no plate, border, glow, or shadow. `tint` is the historical scrim. |
| `color` | colour | theme tint | Fill colour for `solid` and the first stop for `gradient`. |
| `color_end` | colour | transparent | Second gradient stop. Unset fades to nothing. |
| `gradient_angle` | number | `0` | Gradient direction in degrees, -360 to 360. Zero goes left to right. |
| `radius` | number | `14` | Corner radius in logical pixels, 0 to 64. |
| `border` | colour | unset | One-pixel stroke. `tint` already has a faint theme border; setting this overrides it. |
| `padding` | number | `12` | Space between the content and the plate edge, 0 to 64. |
| `glow` | colour | unset | Outer halo colour. |
| `glow_size` | number | `0` | Halo spread in logical pixels, 0 to 48. Zero disables it. |
| `shadow` | bool | `false` | Draw a soft drop shadow under the plate. |
| `opacity` | number | `1` | Overall opacity for the widget and its plate, 0 to 1. |

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

`[clock.motion]` and `[media.motion]` reuse the animated-decor table above
verbatim: `spin`, `sway`, `float`, `pulse`, `fade`, `phase`, and `blink_ms`.
The transform applies to the whole widget, plate included. `typer_ms` and the
ticker keys only make sense for a text run and are ignored here.
`[motion].reduced` and `[power].low_power` pin the sample at its neutral pose,
so a configured widget never disappears when animation is turned off.

`show_on` and `hide_on` accept up to eight state names. Omitting `show_on` means
"show in every state"; `hide_on` wins whenever both lists match. Exactly one
state is current per frame, chosen in this priority order:

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

Plugin widgets do not have a plate or motion table yet. They choose a paint
layer per widget with `layer = "behind" | "front"`, and the form still paints
after both layers. See [the plugin API](plugins.md#layers).

## `[calendar]`

The calendar is a first-party month grid built from the local date. It has no
event source and makes no network request: what you see is the system clock and
a seven-column view of the current month. It is disabled by default so an
existing config renders exactly as before.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the calendar. |
| `layer` | string | `"behind"` | `behind` paints in screen space before decor; `front` paints after decor but before the form and uses card space. |
| `anchor` | string | `"top-right"` | One of the anchors listed under `[login]`. |
| `offset` | `[x, y]` | `[40, 40]` | Logical pixel margin from the anchor. |
| `monitor` | string | unset | Output name restriction. Unset shows the widget on every output. |
| `rotation` | number | `0` | Rotate the widget in degrees (-360 to 360). |
| `size` | number | `13` | Day number font size, 8 to 48. The cell height is twice this value. |
| `width` | number | `260` | Total content width, 120 to 800. The seven columns divide it evenly. |
| `week_start` | string | `"monday"` | `monday` or `sunday`. |
| `show_header` | bool | `true` | Draw the month name and year above the weekday row. |
| `color` | colour | `#F7F3E9` | Ordinary day numbers. |
| `muted_color` | colour | `#8A8D80` | Weekday labels and other secondary text. |
| `today_color` | colour | `#E2734F` | Fill behind the current day. |
| `header_color` | colour | `#E7DA19` | Month and year label. |
| `draggable` | bool | `true` | Let the pointer move the calendar. Saved by `[layout]`. |
| `plate` | table | see below | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array | empty | States that may show the calendar. Empty means every state. |
| `hide_on` | array | empty | States that hide the calendar. Wins over `show_on`. |

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

`[calendar.plate]`, `[calendar.motion]`, `show_on`, and `hide_on` accept exactly
the same values as `[clock.plate]`, `[clock.motion]`, `[clock].show_on`, and
`[clock].hide_on` above.

## `[battery]`

The battery widget reads `/sys/class/power_supply` and nothing else. It is
disabled by default, and on a desktop with no battery it draws a muted
`NO BATTERY` label instead of a fake percentage.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the battery widget. |
| `layer` | string | `"behind"` | `behind` paints in screen space before decor; `front` paints after decor but before the form and uses card space. |
| `anchor` | string | `"bottom-right"` | Placement anchor. |
| `offset` | `[x, y]` | `[40, 40]` | Logical pixel margin. |
| `monitor` | string | unset | Output name restriction. |
| `rotation` | number | `0` | Rotate the widget in degrees (-360 to 360). |
| `size` | number | `14` | Text and geometry scale, 8 to 48. |
| `width` | number | `220` | Total content width, 120 to 800. |
| `style` | string | `"ring"` | `ring` (sixty radial segments), `bar` (horizontal bar), or `pill` (battery body with a terminal nub). |
| `color` | colour | `#F7F3E9` | Normal charge colour. |
| `low_color` | colour | `#E2734F` | Colour at or below `low_at` while discharging. |
| `charging_color` | colour | `#C6D62B` | Colour while the battery reports `Charging`. Charging wins over low. |
| `low_at` | integer | `20` | Low threshold from 0 to 100. |
| `show_percent` | bool | `true` | Draw the percentage text. The bolt is drawn independently. |
| `draggable` | bool | `true` | Let the pointer move the widget. Saved by `[layout]`. |
| `plate` | table | see below | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array | empty | States that may show the battery widget. Empty means every state. |
| `hide_on` | array | empty | States that hide the battery widget. Wins over `show_on`. |

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

The widget refreshes once every two seconds. Reading a handful of sysfs files
on the existing UI tick is cheaper and simpler than keeping a background thread
alive for the whole greeter session.

## `[system]`

The system monitor reads `proc/sys/kernel/hostname`, `proc/sys/kernel/osrelease`,
`proc/uptime`, `proc/loadavg`, and `proc/meminfo`. It is disabled by default and
never writes a file or contacts the network.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Show the system monitor. |
| `layer` | string | `"behind"` | `behind` paints in screen space before decor; `front` paints after decor but before the form and uses card space. |
| `anchor` | string | `"top-left"` | Placement anchor. |
| `offset` | `[x, y]` | `[40, 40]` | Logical pixel margin. |
| `monitor` | string | unset | Output name restriction. |
| `rotation` | number | `0` | Rotate the widget in degrees (-360 to 360). |
| `size` | number | `12` | Font and row scale, 8 to 48. |
| `width` | number | `280` | Content width, 120 to 800. |
| `style` | string | `"rows"` | `rows` for label and value pairs, `sparkline` for a load history above those rows. |
| `fields` | array | all five | Ordered list of `hostname`, `kernel`, `uptime`, `load`, and `memory`. At most six; duplicates and unknown names are dropped with an issue. |
| `color` | colour | `#F7F3E9` | Reading values and the sparkline. |
| `label_color` | colour | `#4E5148` | Uppercase labels and the sparkline baseline. |
| `draggable` | bool | `true` | Let the pointer move the widget. Saved by `[layout]`. |
| `plate` | table | see below | Backplate style, colour, border, glow, shadow, padding, radius, and opacity. |
| `motion` | table | static | Animated decor motion applied to the whole widget. |
| `show_on` | array | empty | States that may show the monitor. Empty means every state. |
| `hide_on` | array | empty | States that hide the monitor. Wins over `show_on`. |

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

The sparkline keeps the last 32 one-minute load samples, collected on the same
two-second refresh. Uptime is shown as `3d 04h`, `02h 14m`, or `14m`, and
memory as `6.2 / 15.5 GiB`.

All three widgets share the clock's plate and motion vocabulary, so a HUD theme
can give them the same border and glow:

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

## `[background]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `kind` | string | `"auto"` | `auto`, `solid`, `gradient`, `image`, `animated`, or `video`. `auto` detects from the file, or uses the gradient when no image is set. `video` is an alias for `animated` that documents intent. |
| `color` | colour | preset | Base colour and the fallback background. Accepts `#rgb`, `#rgba`, `#rrggbb`, and `#rrggbbaa`. |
| `gradient` | bool | `true` | A subtle vertical gradient derived from `color`. Set to `false` for a flat background. |
| `image` | path | unset | PNG, JPEG, WebP, GIF, animated WebP, or an MP4/M4V/MOV/WebM/MKV/AVI/MPG file. |
| `fit` | string | `"cover"` | `cover`, `contain`, or `fill`. |
| `fps` | integer | `0` | Upper bound on animated playback, 0 to 240. `0` follows the source exactly, which is what keeps a 60 fps clip smooth. A positive value caps GIF/WebP delays and adds an ffmpeg `fps` filter to video. |
| `memory_limit_mb` | integer | `128` | Hard per-frame decode budget, 16 to 1024 MiB. Animated GIF/WebP and video stream one frame at a time; a single frame larger than the budget is downscaled. |
| `idle_pause_secs` | integer | `45` | Freeze an animated wallpaper after this long without input. `0` disables the idle pause. |
| `monitors` | table | empty | Per-output overrides, at most 16. |

Still wallpapers are decoded once on a loader thread. Animated GIF and WebP play
through `ui::stream`: only the current frame is kept, so a long 1080p animation
does not fill memory. Video uses `ui::video`, which asks the system `ffmpeg` for
raw RGBA frames on a pipe. ffmpeg runs with `-re` and `-fps_mode passthrough`,
so frames arrive on the source's own clock rather than being decoded in a burst
or resampled to a fixed rate. Each published frame wakes the window directly,
so a 60 fps wallpaper is presented as soon as its frame is ready. Install
`ffmpeg` if you use a video wallpaper. If it is missing, dawn falls back to the
gradient and logs a warning; the login form is unaffected.

The window paints the gradient placeholder immediately and crossfades to the
first frame. A missing, oversized, or corrupt file falls back to the gradient
and shows a warning that names the file. `kind = "image"` on a video shows the
first frame and labels it as a still, instead of silently animating.

Per-output overrides use the output name from the compositor:

```toml
[background.monitors.DP-1]
kind = "image"
image = "wallpapers/left.png"
fit = "contain"
color = "#0b1020"
```

Unknown output names keep the base values. In v0.2.0 the greeter creates one
window, so the override that applies is the one for the output containing that
window. A future multi-window greeter can apply the same table per output.

## `[theme]`

The theme layer resolves design tokens into a palette. Presets supply defaults;
every token can still be overridden.

```toml
[theme]
preset = "glass-dark"   # glass-dark, glass-light, or minimal
```

Set `package` to use an installed theme bundle instead of this file:

```toml
[theme]
package = "midnight"
```

A pointer config is not merged with the bundle. The bundle's `config.toml`
becomes the whole config, and its relative asset paths resolve against the
bundle directory. See `dawn list`, `dawn add`, and the theme section in
[installation.md](installation.md). `package` is not a token, so the rest of the
`[theme]` table is ignored when it is present.

| Key | Default (glass-dark) | Used for |
| --- | --- | --- |
| `preset` | `"glass-dark"` | Base token set. The light preset also supplies a light background colour when `[background].color` is unset. |
| `text` | `"#f2f6ff"` | Primary text and input text. |
| `muted` | `"#a9b4c8"` | Placeholders, hints, prompts, dates. |
| `accent` | `"#7db1ff"` | Focus rings, caret, selection, dry-run badge. |
| `error` | `"#ff8a80"` | Login errors and asset warnings. |
| `tint` | `"#101726cc"` | Glass panel and widget scrim tint. The last two digits are alpha. |
| `border` | `"#ffffff40"` | One pixel panel highlight. |
| `shadow` | `"#00000066"` | Soft shadow layers behind panels. |
| `focus_ring` | `"#7db1ff"` | Reserved focus-ring token. |
| `corner_radius` | `18` | Panel corner radius, 0 to 64. |
| `spacing` | `1.0` | Padding and gap multiplier, 0.75 to 2.0. |
| `noise` | `true` | Subtle grain over glass panels. |
| `min_contrast` | `4.5` | Minimum contrast target used by the theme tests and text guards. |
| `scanlines` | `0.0` | CRT scanline strength, 0 to 1. Zero turns the overlay off. |
| `scanline_gap` | `3` | Physical pixel spacing between scanlines, 2 to 12. Smaller is denser. |
| `scanline_ms` | `6000` | Time for the bright sweep band to cross the screen once, 1000 to 20000 ms. |

A subtle CRT backdrop looks like this:

```toml
[theme]
scanlines = 0.35
scanline_gap = 3
scanline_ms = 6000
```

The static lines paint directly onto the wallpaper, before the glass and the
login form, so the effect textures the backdrop instead of the text. Reduced
motion and low-power mode keep the lines but drop the moving sweep band.

`card` is accepted as a legacy alias for `tint`. `blur_radius`,
`blur_refresh_ms`, and `blur_downsample` are accepted for dawn 0.2 configs;
`blur_radius` and `blur_refresh_ms` map to the corresponding `[theme.blur]`
values, while `blur_downsample` is ignored.

### `[theme.blur]`

Backdrop blur follows Hyprland's `decoration:blur` block. Dawn implements the
same stages on the CPU: a contrast/brightness prepare, a dual Kawase pyramid
(downsample with five taps, upsample with an eight-tap tent), then grain and
brightness. It is not a GPU compositor blur, but the radius, pass count, and
finish controls behave like the compositor settings people already tune. The
GPU presentation path still samples this cached result; only the final
wallpaper composite runs on the GPU.

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

| Key | Default | Notes |
| --- | --- | --- |
| `enabled` | `true` | Turn backdrop blur on or off. |
| `size` | `8` | Blur radius in logical pixels, 0 to 64. |
| `passes` | `3` | Dual Kawase passes, 1 to 8. More passes are wider and softer. |
| `noise` | `0.0117` | Per-pixel grain added after the blur. |
| `contrast` | `0.8916` | Contrast gain applied before blurring. |
| `brightness` | `0.8172` | Final brightness multiplier. |
| `vibrancy` | `0.1696` | Saturation boost during the downsample. |
| `vibrancy_darkness` | `0.0` | Protects dark colours from the vibrancy boost. |
| `refresh_ms` | `300` | How often an animated wallpaper refreshes the cached blur, 50 to 5000 ms. |

`ignore_opacity`, `new_optimizations`, `xray`, `special`, `popups`, and
`popup_ignore_alpha` are accepted so a Hyprland blur block can be pasted
directly, but they do not change a single full-screen opaque greeter. Colour
management is likewise a compositor feature and is not exposed. The blur is
computed on a reduced copy of the background and cached; a 1080p refresh costs
a few milliseconds, and animations refresh on the configured interval rather
than once per frame.

Text colours are checked against the glass tint and adjusted to black or white
when the configured colour would miss the contrast target. The clock and media
widgets sit directly on the wallpaper, so they get a theme-tinted plate behind
their content by default. `[clock.plate]` and `[media.plate]` can recolour,
reshape, or remove it. Focus rings are drawn with the accent colour and are
never the only indicator of focus: the caret and selected row change too.

## `[motion]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Master switch for boot, focus, and error motion. |
| `reduced` | bool | `false` | Skip motion and freeze animated wallpapers on one frame. |
| `boot_ms` | integer | `450` | Card slide and fade duration, 0 to 2000. |
| `focus_ms` | integer | `160` | Focus-ring settle duration, 0 to 1000. |
| `shake_ms` | integer | `320` | Error shake duration, 0 to 2000. |
| `drag_ms` | integer | `120` | Time constant for pointer-drag easing, 0 to 1000. `0` snaps to the pointer. |
| `decor_fps` | integer | `30` | Refresh rate for animated decor only, 10 to 60. Focus, typing, boot, and button effects still use the full motion rate. Lower this on weak hardware; raise it when every ornament should move at the wallpaper's frame rate. |
| `parallax` | number | `0` | Global cursor parallax multiplier, 0 to 2. Each decor item or built-in widget with a positive `parallax_depth` shifts with the pointer. Zero disables it. |

Animated decor is rasterised inside the cached UI overlay, so a moving petal
would otherwise force a full overlay repaint on every 60 fps wallpaper frame.
`decor_fps` caps only that ambient path. Input, focus, the boot animation, and
button hover effects stay at the full rate.

## `[layout]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `save` | bool | `false` | Remember dragged widget positions between runs. |
| `file` | path | state directory | Explicit layout file. Relative paths resolve against the config file. |

When `save = true` and `file` is unset, dawn tries
`$XDG_STATE_HOME/dawn/layout.toml`, then
`~/.local/state/dawn/layout.toml`, then `/var/lib/dawn/layout.toml`. The file is
small, versioned, and written atomically with mode `0600`. A missing or corrupt
file is ignored; dragging is decoration, never a login dependency.

The saved keys are stable widget ids: `login`, `clock`, `media`, and
`<plugin-id>:<index>` for plugin widgets. Built-in ids do not shift when you
disable one of them.

## `[power]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `low_power` | bool | `false` | Freeze wallpaper animation and disable motion. Intended for battery-powered or remote greeter hardware. It never affects login responsiveness. |

## `[gpu]`

This table only controls the live window's GPU compositor. `--screenshot` and
the CPU compositor always run on tiny-skia, so a GPU setting can never change
the pixels in a screenshot.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Set to `false` to use softbuffer without touching a driver. `DAWN_GPU=0` is the environment equivalent. |
| `backend` | string | `"auto"` | `"auto"` enables Vulkan and GL, `"vulkan"` or `"gl"` picks one, and `"software"` keeps the GPU pipeline but forces a CPU adapter. |
| `adapter` | string | `"auto"` | `"auto"`, `"index:N"`, or a case-insensitive substring of the adapter name or driver. |
| `device` | string | `"auto"` | `"auto"`, `"discrete"`, `"integrated"`, `"virtual"`, or `"cpu"`. A non-auto value is a hard filter. |
| `power` | string | `"low"` | Ranking for `adapter = "auto"`: `"low"`, `"high"`, or `"default"`. |
| `present` | string | `"fifo"` | `"fifo"`, `"mailbox"`, or `"immediate"`. A mode the surface does not offer is ignored with a warning. |
| `fallback` | string | `"chain"` | `"chain"` tries the configured adapter, then a software adapter, then softbuffer. `"software"` stops after the software adapter, `"cpu"` skips the GPU, and `"none"` sends a failure to the terminal fallback. |

`index:N` numbers the adapters that match the backend and device filter, in the
same order `dawn --list-gpus` prints them, before any power ranking. An index
that does not exist at runtime produces a readable error and follows
`fallback`; it is not a config error. A name that matches nothing at runtime
also falls through the same way, because the adapter may only exist on the
machine that actually runs the greeter. Run `dawn --list-gpus` to see the
indices, names, backends, device classes, and which one this config selects.

### Host-only rule

`[gpu]` always comes from the host config, even when `--theme ID` replaces the
rest of the file. A theme bundle cannot change which adapter the greeter uses.
When `--config PATH` is passed without a theme, that file is the host config and
its `[gpu]` table applies. `DAWN_GPU` is applied after the host table is chosen:
`0` or `cpu` disables the GPU, `vulkan`, `gl`, or `software` selects a backend,
`auto` resets `adapter` to `"auto"`, and any other value is treated as an
adapter name.

## `[sessions]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `dirs` | list of paths | `["/usr/share/wayland-sessions", "/usr/share/xsessions"]` | Directories scanned for `.desktop` sessions. At most 8. An empty list is rejected. |
| `default` | string | unset | Desktop file id to preselect, for example `"hyprland"`. Unknown ids fall back to the first session. |

Sessions are sorted by display name. `TryExec` is honoured, `NoDisplay` and
`Hidden` entries are skipped, and duplicate ids keep the first directory's
version. The greeter does not invent `XDG_SESSION_TYPE`; the session launcher
knows whether it is Wayland or X11.

## `[plugins]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Set to `false` to skip plugin discovery entirely. |
| `dirs` | list of paths | `["/usr/share/dawn/plugins", "$HOME/.config/dawn/plugins"]` | Directories containing one subdirectory per plugin. At most 8. |

Plugin directories may be missing. A broken plugin is logged and skipped
without affecting the login form or other plugins. See
[plugins.md](plugins.md).

## `[auth]`

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `socket` | path | unset | Override the greetd socket. When unset, `GREETD_SOCK` is used. |
| `timeout_secs` | integer | `120` | Read and write timeout for greetd requests, 5 to 600 seconds. |

The socket override exists for a fake greetd during development. A normal
greetd launch sets `GREETD_SOCK` and needs no config.

## Full examples

- [../examples/config.toml](../examples/config.toml): a complete default config.
- [../examples/showcase-aurora.toml](../examples/showcase-aurora.toml): a
  polished cool-toned showcase with a custom wallpaper.
- [../examples/showcase-ember.toml](../examples/showcase-ember.toml): the warm
  counterpart, same layout with a plum and coral palette.
- [../examples/showcase-gif.toml](../examples/showcase-gif.toml): the same
  layout with an animated GIF wallpaper.
- [../examples/showcase-video.toml](../examples/showcase-video.toml): the same
  layout with an MP4 live wallpaper. Requires `ffmpeg`.
- [../examples/themes/hutao-chibi](../examples/themes/hutao-chibi): a cardless
  terminal layout with line fields, ambient decor, and a video wallpaper.
- [../examples/themes/fischl](../examples/themes/fischl): a banner-led royal
  layout with a crest, crescent moon, and staggered stars.
- [../examples/themes/genshin](../examples/themes/genshin): a card-free hero
  layout with a hydro sigil and an open gold frame.
- [../examples/themes/furina](../examples/themes/furina): an opera playbill
  layout with a curtain, spotlight, droplets, and an applause meter.
- [../examples/themes/kinich](../examples/themes/kinich): an 8-bit tactical HUD
  with pixel brackets, drifting leaves, and a status bar.
- [../examples/themes/hutao-montagem](../examples/themes/hutao-montagem): an
  after-dark concert poster with a marquee ring, notes, and a ticket stub.
- [../examples/wallpapers](../examples/wallpapers): the showcase wallpapers,
  with editable SVG sources next to the rendered WebP files.
- [../examples/snippets/glass-light.toml](../examples/snippets/glass-light.toml):
  a light theme block to merge into a config.
- [../examples/plugins/hello-world/plugin.toml](../examples/plugins/hello-world/plugin.toml):
  a minimal widget plugin.
