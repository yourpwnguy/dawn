# Audio

dawn can play theme audio: a looping bed under the login screen and short
sounds for typing, focus, submit, success, failure, and the optional rituals
layer. Every shipped example enables it, and the default device is `auto`, so a
tester does not have to edit a theme for their own sound card.

Sound is still a machine policy. The host config can mute it, cap the volume,
and choose a device. A theme supplies the files and the sound design, not the
machine's mute switch.

## Turn it on

The built-in default is on and system independent:

```toml
# /etc/dawn/config.toml
[audio]
enabled = true
volume = 60     # 0 to 100
device = "auto" # "auto", "default", or an ALSA device name
```

`auto` asks ALSA for playback hints and ranks them: analog
`sysdefault`/`front` first, then `plughw`/`hw`, then `default`. Loopback cards
and helper PCMs are skipped, and HDMI is demoted below analog. That is what
makes the same bundle work on a laptop, a desktop with a USB DAC, and a machine
with several cards.

`DAWN_AUDIO=0` mutes for one process. `DAWN_AUDIO=1` enables for one process,
which is what the theme studio uses. `enabled = false` in the machine config is
the permanent switch.

A theme that ships no audio content is silent even when `enabled = true`.
`should_start` requires an ambient bed or at least one event.

## Theme sound design

```toml
[audio.ambient]
file = "assets/audio/parlor-loop.ogg"
volume_db = -14
fade_in_ms = 800
duck_db = -10
duck_ms = 250

[audio.events.key]
file = "assets/audio/key-01.ogg"
volume_db = -18
retrigger = "restart"
cooldown_ms = 55
pitch_jitter = 1.5

[audio.events.success]
file = "assets/audio/stage-call-success.ogg"
volume_db = -6
```

When a bundle is selected, the machine's `enabled`, `volume`, `device`,
`max_voices`, and `stop_on_login` are applied over the bundle values. The
bundle still owns the ambient and event tables.

### `[audio.ambient]`

| Key | Default | Meaning |
| --- | --- | --- |
| `file` | unset | Loop bed. Absent means no ambient. |
| `volume_db` | `-12` | Gain relative to the master volume. |
| `fade_in_ms` | `600` | Fade at the start of the greeter. |
| `duck_db` | `-10` | Gain while a stinger plays. `0` disables the duck. |
| `duck_ms` | `250` | How long each stinger holds the duck. |

### `[audio.events.<name>]`

| Key | Default | Meaning |
| --- | --- | --- |
| `file` | required | `.ogg` or `.wav` under `assets/`. |
| `volume_db` | `-10` | Gain relative to the master volume. |
| `retrigger` | `restart` | `restart`, `overlap`, or `ignore`. |
| `cooldown_ms` | `0` | Minimum gap between two plays of this event. |
| `pitch_jitter` | `0` | Random pitch variation in semitones, for key clicks. |
| `duck_db` | `-10` | Ambient ducking while this sound plays. `0` disables it. |
| `max_ms` | `0` | Hard cutoff. `0` means the decoded length. |

Unknown event names are config errors, so a typo cannot silently produce a
silent theme.

## Wallpaper audio

A video wallpaper can supply the ambient bed instead of a separate audio file:

```toml
[background]
kind = "video"
image = "assets/stage.mp4"
audio = true
```

The track is extracted with ffmpeg when the theme loads. `[audio.ambient]` still
controls `volume_db`, `fade_in_ms`, `duck_db`, and `duck_ms`; its `file` is
ignored while the wallpaper track is playing. If the video has no audio track,
dawn falls back to `ambient.file` when one is set, otherwise it stays silent.
The host `[audio].enabled` switch still gates everything.

The same 3 minute cap applies. A longer track is refused and the fallback
happens silently; use a shorter loop or a separate ambient file.

## Events

| Event | When it fires |
| --- | --- |
| `boot` | The first frame of the greeter. |
| `key` | A printable character is added to a field. |
| `backspace` | A character is deleted. |
| `field_focus` | Focus moves between username, password, and the session picker. |
| `submit` | Enter starts an authentication attempt. |
| `success` | Authentication is accepted. |
| `failure` | Authentication is rejected. |
| `cancel` | Escape or cancel aborts the attempt. |
| `session_change` | The session picker selection changes. |
| `rarity_common` | A common boot drop. |
| `rarity_rare` | A rare boot drop. |
| `rarity_saga` | A saga boot drop. |
| `milestone` | A streak milestone was reached. |
| `achievement` | An achievement unlocked. |
| `error_config` | The startup check found a config problem. |

Key clicks are capped at 12 per second no matter what the config asks for. On
success the ambient fades out, the success sound is cut at the handoff, and the
device is released so the user session can open it. Sound never delays the
login.

## Formats and limits

| Item | Limit |
| --- | --- |
| Formats | OGG Vorbis (`.ogg`), WAV PCM 16/24 or float 32 (`.wav`) |
| Event file | 2 MiB, 5 seconds |
| Ambient file | 16 MiB, 3 minutes, 80 MiB decoded |
| Sample rate | 8 kHz to 192 kHz |
| Channels | mono or stereo |
| Voices | 8 by default, 1 to 16 in config |

Audio is decoded by pure Rust decoders. `ffmpeg` is only used for video
wallpapers, not for event sounds.

## Where the sound goes

At boot there is no PipeWire or PulseAudio session. dawn talks to ALSA
directly. `device = "auto"` asks ALSA for playback hints, prefers analog
`sysdefault`/`front`, then `plughw`/`hw`, and keeps `default` last. Loopback
cards and helper PCMs are skipped, and HDMI is demoted below analog. If nothing
opens it stays silent. A configured device that cannot be opened is not
retried: dawn logs one warning and the login continues without sound.

On a real machine, ask ALSA what it can see and test the exact name:

```sh
dawn audio devices
dawn audio test --device "hw:0,0"
sudo dawn audio test --device "sysdefault:CARD=Generic"
```

Use the name that prints `opened ... at ... Hz` only when the `auto` result is
wrong on your machine. The shipped themes already use `auto`, so there is
nothing to edit for a normal install. `default` is often a PipeWire or
PulseAudio plugin that works in your desktop session but not for the `greeter`
user at boot. An explicit hardware or `sysdefault:` name avoids that mismatch.
Run the test as the greeter user when you can, because that is who opens the
device at the login screen.

The mixer always decodes to 48 kHz stereo, but the device is allowed to pick
the nearest rate it supports. dawn asks ALSA for the accepted rate and
resamples on the fly, so a 44.1 kHz-only DAC still plays at the right pitch.
The period is 10 ms and the buffer is 40 ms, which keeps key clicks responsive
without turning a scheduler hiccup into an underrun. A partial `writei` is
completed in a loop, and an underrun is recovered at most twice before the
audio thread stops instead of spinning.

Wallpaper audio is decoded once through `ffmpeg` at load. That is the only
place the audio path shells out; event sounds and ambient files use the
in-process OGG/WAV decoders.

## Theme development

The studio respects the same policy. A bundle can enable its own sound, but the
machine switch still wins. Use the environment switch while authoring:

```sh
DAWN_AUDIO=1 dawn dev examples/themes/hutao-montagem
```

## Rituals

Rituals are opt-in streaks, milestones, rarity drops, and achievements. They
are per machine, never per user, and store counts and dates only.

```toml
[rituals]
enabled = true
achievements = true      # off by default
state_path = "/var/lib/dawn/rituals.toml"
seed = "date"            # or "boot"
```

With no `state_path`, dawn uses `$XDG_STATE_HOME/dawn/rituals.toml` or
`$HOME/.local/state/dawn/rituals.toml`. If neither is usable, rituals stay off
for the run with one warning. The file is written after a successful login and
at boot, atomically, with mode 0600.

Boot drops use the configured weights, `common = 70`, `rare = 25`, `saga = 5`
by default. `seed = "date"` gives the same drop for every boot on a local date;
`seed = "boot"` rolls every boot.

Achievements: `first_login`, `first_week`, `night_owl`, `early_bird`,
`weekend`, `comeback`.

```sh
dawn rituals status   # streak, best streak, last login, achievements
dawn rituals reset    # remove the state file
```

The next boot plays any pending milestone or achievement sound, then clears it.
Rituals never change authentication and never delay the handoff.

## Design rules

These are hard requirements, not preferences:

1. Audio never blocks authentication. Playback requests are non-blocking sends
   to a worker. A full queue drops sounds.
2. If audio cannot start for any reason (no device, missing crate support,
   corrupt file, permission denied), the greeter runs silently and logs one
   warning.
3. The login path stays bounded. Device open is attempted once at startup. No
   retries while a login is in progress.
4. The device is released when the greeter exits, so the session can open it.
5. Muting is available from the host config and the environment. A theme
   cannot override the machine's mute.
6. Assets are local files under `assets/`. Paths follow the same rules as
   wallpapers: relative, no `..`, no symlinks.
7. Loud surprises are a bug. Shipped assets should peak at or below -3 dBFS;
   `dawn check` warns above that. The default master volume is 60.
8. Rituals are opt-in, local, resettable, and cosmetic. They never change what
   authentication accepts, how long it takes, or what the user sees about other
   accounts.

Non-goals: network audio, user-generated code, playback after the handoff,
per-user tracking, changes to PAM or greetd ordering, and use of the session
audio stack.

## Failure modes

| Failure | Behaviour |
| --- | --- |
| No ALSA device | One warning, silent run, login unaffected. |
| Device busy | One warning at open, silent run. No retry. |
| Missing audio file | Config issue at load; the event is silent; other events still play. |
| Corrupt file | One warning naming the path; that voice is silent. |
| Queue full | Sound is dropped; a counter is logged at exit when verbose. |
| Ambient decode error mid-loop | Ambient stops, one warning; events continue. |
| State file corrupt | Warning, fresh state, rituals continue. |
| State path not writable | Warning, rituals disabled for the run. |
| `stop_on_login` join timeout | Detach the thread and exit; the session is not delayed. |

## Security and privacy

- Decoders are pure Rust and bounded by the file limits above. A malformed
  file is a warning and a silent voice, never a crash.
- Asset paths follow the bundle rules: relative, inside `assets/`, no
  symlinks, no absolute paths.
- The audio path does not shell out, with one deliberate exception: a video
  wallpaper's own track is extracted through `ffmpeg` at load. Event sounds and
  ambient files never use a subprocess. Device names come from config, but they
  are passed as ALSA strings, never through a shell.
- The mixer thread never reads credentials, config, or the greetd socket.
- Ritual state contains counts and dates only. It is not a login history.
- Muting is always available: host `[audio].enabled = false`, `DAWN_AUDIO=0`,
  or a machine policy file managed by the package.
- Audio assets are treated as untrusted input from the theme.

## Tooling

- `dawn check PATH` validates audio references, formats, sizes, headers, and
  reports duration and peak warnings.
- `dawn add` refuses a bundle that fails those checks, as it does for images.
- `dawn audio devices` lists the ALSA playback PCMs the greeter can try.
- `dawn audio test --device NAME` opens one device and plays a short tone.
- `dawn dev` plays audio while editing. A HUD line shows the device and the
  last played event. `F6` replays the last event; `F7` toggles mute.
- `--check-config` reports the audio policy: enabled, device, event count,
  ambient file, and a warning when a referenced file fails to decode.

## Troubleshooting

- Nothing audible: check `dawn --check-config` and look at the `audio:` line.
  `enabled=false` means the host config has turned it off.
- `no usable ALSA device`: run `dawn audio devices` as the greeter user, and
  check that the account is in the `audio` group on systems that require it.
- `Device or resource busy`: PipeWire or PulseAudio currently owns the card.
  The greeter starts before your session, so test from a TTY or stop the user
  sound server first.
- One sound missing but others work: the decoder warning names the file. Check
  the format and the size limits above.
- Sound continues after login: that is a bug. `stop_on_login` defaults to true
  and the device is released on exit; report it.
