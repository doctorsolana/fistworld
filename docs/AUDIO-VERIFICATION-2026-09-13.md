# First menu/cart sound pack — 2026-09-13

The seven user-supplied downloads are integrated. This record separates verified
playback/output from subjective listening approval, which remains pending: the
assistant's audio-input tool was unavailable in this session. Do not mark the
encoded cart seam or final timbre/balance as personally auditioned.

## Assets and runtime

Six mono 44.1 kHz PCM16 UI WAVs and one mono Vorbis cart loop total **328,164 bytes**.
Short WAVs trade a few kilobytes for cheap transient decoding; the cart shrank from
193,581 to 62,832 bytes. Original downloads are byte-preserved in the audio workshop.
`first-pack.json` and `build_first_sfx_pack.py` retain the trim/gain/loop recipe.
The 6.05-second cart uses a 250 ms circular overlap; full decoding and numerical
seam/headroom checks pass. This is distinct from listening approval.

The catalogue contains click, confirm, reject, book open/close, page turn and cart
roll. No creak was generated or integrated. UI actions coalesce before allocating
at most four voices; pending or stale requests expire. At most four physical,
moving, observed carts receive spatial loops. Stable character ownership survives
render-rig replacement. Distance/zoom, motion, mute and exit control lifetime.

Escape → Audio exposes Master, Music and Effects levels, separate Music/Effects
switches, drag controls, five-percent steps and keyboard adjustment. Changes reach
existing and newly created sinks; short gain ramps soften level changes. Background
music retains its position on mute. Preferences persist independently of graphics,
with drag-save debouncing and normal-exit flushing.

Rodio 0.22.2's directional factor reversed the installed spatial output at this
scale. The documented listener-ear correction preserves attenuation. A real Rodio
iterator probe checked 1,080 source/elevation/yaw combinations. Recheck this when
upgrading the backend or changing spatial reference distances; this is not a
promise about arbitrary future spatial settings.

## Verification

- `cargo check --workspace --all-targets` and matching playtest build pass.
- `cargo test --workspace --all-targets --profile playtest`: 1,427 passed,
  21 deliberately ignored, no failures.
- Eleven real-codec/preprocessor Python tests pass.
- `capture/scenarios/ui-music.ron`: seven real Bevy PNG/capture/music artifact
  sets. Music mute/resume retains the entity/position; opening priority, discarded
  muted opening and the post-song quiet gap pass. The Audio page was inspected at
  1280×720 with its capture metadata.
- `capture/sfx_review.py`: a real generated world with ordinary porters and
  production UI input. The first run at 1600×1000 completed in 94 seconds, changed
  levels to 60/30/50%, checked native sink gain, preserved levels across mutes,
  exercised book open, two page turns and book close without duplicate clicks,
  observed a cart for about 12 seconds/51 metres, released voices at zoom 1000,
  resumed nearby and muted the running cart through the menu. Zero device-start
  timeouts or unavailable cues. Saved audio preferences remained byte-identical.
- An additional real launcher check entered invalid port zero, received one
  `UiReject`, and recorded its output. No fake server rejection was injected.

Review files are ignored under `logs/sfx-review/`. The first connected captures
revealed a faint unfilled slider track and unsupported arrow glyphs; the final
layout uses a visible dark-brass rail and plain keyboard labels. The final 720p
repeat passed under `logs/sfx-review/run-02/` in 97 seconds. Its final Audio
page and muted state were personally inspected with PNG/capture/session metadata;
the new full rails and keyboard labels are visible. That run recorded another
12.1 seconds of native cart output and about 47 metres of travel.

`capture/record_app_audio.swift` recorded only the owned client application's
stereo output, without the microphone. The first run's UI mix peaked at 0.8575;
cart-only output at 0.1656. Neither contained clipped samples. The final half-second
of the live-mute recording was exactly silent. Inspect the WAVs and their
`.capture-audio.json` companions; screenshots and sink counters alone do not prove
an audible device mix. These sampled cases do not guarantee headroom for every
future sound combination or provide a comparative CPU/frame-time benchmark.

## Reproduce

Build the matching workspace playtest binaries and the recorder as documented in
[VISUAL-CAPTURE.md](VISUAL-CAPTURE.md), then use a fresh ignored output directory:

```sh
python3 capture/sfx_review.py --out logs/sfx-review/new-run --resolution 1280x720
```

The runner refuses an occupied server port and only stops its owned processes.
Runtime sound tuning lives in `client/src/audio/`; maintained source/export records
live under `asset_creation/audio/`. Listening previews and game recordings stay out
of Git. Final perceptual approval should be recorded only after actual audition.

## User listening feedback

The user tested the integration and liked the result except for the carts, which
sound as though the microphone is next to the wheels. Treat cart perceptual
acceptance as unresolved. Before requesting another recording, tune the cart's
base gain, nearby distance falloff and normal-zoom attenuation, then evaluate
distance-dependent high-frequency softening. That first implementation had
spatial/zoom gain but no distance-dependent tonal filtering; its zoom fade started
at 100, leaving ordinary close views at full detail. The user subsequently approved
the tuning and reusable world-effect foundation below. No additional audio was generated.

## Approved cart distance pass

The same recording now uses a lower authored gain (0.28 → 0.20), shorter native
reference distance (12 → 6 m), earlier zoom attenuation and live low-pass filtering.
UI/music gains are unchanged. Reusable `audio::perspective` profiles and native
`audio::filtered` sources supply this treatment for future short world effects;
the broader banks/allocator are still future work. See [AUDIO-DESIGN.md](AUDIO-DESIGN.md).

- Workspace all-target check/build pass. All-target playtest tests: **1,434 passed,
  21 ignored, zero failures**. Seven new tests cover distance/zoom curves, repeated
  loop filtering, source sharing/lifetime, finite smooth coefficient changes,
  real cart decode/corrupt input, and prevention of native outer-loop buffering.
  Stereo regression now directly samples Rodio across 1,080 positions/yaws instead
  of reproducing its formula, including the revised ear gap and reference scale.
- Connected run `logs/sfx-distance-review/run-01/` passed in 139 seconds with the
  normal seeded world (246 residents). The first observation recorded 12.18 seconds
  of cart output, 48.1 m of real movement and 7.1 contiguous seconds of estimated
  audible native playback. Audibility now accounts for native distance gain.
- The next camera-only trial retained voice `7980v1` across zoom 24 → 100 → 24.
  Decoder position advanced 2.807 → 12.963 → 17.189 seconds without restarting.
  Pre-spatial gain was 0.1275 → 0.0504 → 0.1275; cutoff was 5,372 → 2,622 →
  5,416 Hz. A 16.58-second actual-app recording contains this transition. The
  focused cart remained near the listener, so zoom—not a forced simulation
  movement—produced the main change.
- Prepared PCM remained **1,067,220 bytes** across all voices. The observed peak
  was three simultaneous cart voices within the four-voice cap. Far zoom and live
  Effects mute released all filtered player assets and sinks; seven starts matched
  seven stops, with no device timeouts. Saved audio preferences stayed byte-identical.
- Captured cart-only peak was 0.0407; the distance recording peaked at 0.0621.
  All four application recordings were finite and below clipping. The final
  half-second of the live-mute recording was exactly zero. These are different
  live mixes, not controlled before/after loudness or CPU benchmarks.
- Personally inspected close-cart, town-zoom and muted Audio-page PNGs with their
  capture metadata. The run began at 1280×720 and later captures were 2342×1356;
  record actual artifact dimensions rather than assuming the requested size.

That mix has recorded output and numeric/runtime verification. Its subjective
distance impression and final timbre still require listening by the user; do not
claim that the assistant auditioned it. Recordings/screenshots remain ignored.

## Closest-view follow-up

The user clarified that even maximum zoom must sound distant from the wheels.
The overhead camera never reaches a close-microphone listening position. Cart gain
is now 0.14 (previously 0.20) and its low-pass range is 700–2,400 Hz (previously
900–7,000 Hz). Distance and zoom soften that already restrained base further.
The recording, loop construction, UI/music mix, voice budget and memory are unchanged.

The existing real-decoder test now also checks the closest cart profile against
an unfiltered tone: 8 kHz detail falls by at least 20 dB while 200 Hz rolling body
retains over 90% before gain attenuation. The connected distance rehearsal now
uses the camera's actual minimum zoom of 12 and asserts the cutoff never exceeds
2,400 Hz, including while the camera moves.

Verification: workspace all-target check/build and all 33 audio tests pass.
`logs/sfx-overhead-review/run-01/` passed the connected rehearsal in 154 seconds.
The same native voice continued through zoom 12 → 100 → 12 with cutoff
2,102 → 1,174 → 2,104 Hz and decoder position 3.830 → 13.944 → 17.953 seconds.
The 17.22-second application recording peaked at 0.0435 without clipping.
Far zoom and mute released the filtered voices/assets; the mute recording's final
half-second was silent, and saved preferences stayed byte-identical. The real
minimum-zoom PNG and capture metadata were inspected at 1280×720. Perceptual
acceptance remains for the user's listening; numerical checks do not establish it.
