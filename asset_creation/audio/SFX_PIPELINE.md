# Sound-effect asset pipeline

Plan established 2026-09-13. The runtime design is
[AUDIO-DESIGN.md](../../docs/AUDIO-DESIGN.md). This document defines the authoring
contract. **The first seven-cue menu/cart pack now has preserved originals,
reproducible exports and a bounded native Bevy playback integration. Actual game
playback/output and visual checks pass; subjective listening remains pending. See
[the verification record](../../docs/AUDIO-VERIFICATION-2026-09-13.md).** The larger footsteps,
army, battle and ambience design remains planned. The existing `build_audio.py`
is a music exporter, not a general SFX processor. The
[cart/menu pack](sfx/FIRST-PACK.md) records the supplied files and original brief.

## Direction and first pack

Use tactile medieval materials: paper, leather, wood, restrained metal, boots,
wheels and cloth. Match the calm world and worn UI. Keep attacks intelligible
without making ordinary interaction bombastic. Avoid a sound on every hover,
every scroll tick or every visible character. Music and ambience should leave
quiet space; the user's latest music feedback explicitly rejects a constant
backing layer that never stops.

| Family | First assets | Listening requirement |
|---|---|---|
| UI | Press/confirm, soft rejection, book open/close, page change | Short and distinct at low gain; page sounds must not mask reading or stack on clicks. |
| Footsteps | Dirt and paving, about 4–6 variations each | Convincing small close transients; no rigid repeated sample or default wind/reverb baked in. |
| Cart | One rolling loop; creaks explicitly omitted from the first pack | Smooth start/stop and a tested seam. Any later creaks need a separate request and intermittent playback. |
| Horse | About 4–6 hoof contacts, light tack/gear | Distinguishable from boots; animation/ground movement controls cadence. |
| Army | A few march textures for boots and cavalry | Coherent mass movement, without shouting or music baked into the loop. |
| Battle | Swing/contact, bow release, arrow impact, catapult release/landing | Separate intent, impact and scale; no musical stingers or excessive low-end on routine hits. |
| Environment (later) | Wind/foliage, water, sparse birds and work | Natural pauses and variation; avoid busy continuous noise everywhere. |

These counts are starting briefs, not an instruction to generate the whole pack
automatically. The user generates ElevenLabs material themselves unless they
explicitly request otherwise. Give short prompts for the next needed cue, retrieve
named approved downloads when asked, and approve each family in the actual mix.
Record provider/project, source, prompt, date and use rights/provenance.

## Catalogue and file ownership

The first pack uses these maintained files:

- `sfx/first-pack.json`: source hashes, provenance, reference prompts, precise trim,
  gain and cart overlap recipes, runtime paths and measured export sizes. Rebuild
  with `build_first_sfx_pack.py`; this authoring record does not assert listening
  approval or duplicate runtime voice/mixer tuning.
- `sfx/cues.ron`: still a proposed broader authoring source of truth for stable cue IDs, variants,
  source and runtime paths, provenance, category, base gain, narrow randomization,
  cooldown, priority, instance limits and spatial/loop parameters. Generate/validate
  runtime definitions from it so Rust tuning and authoring records do not drift.
- `sfx/prompts/` and `sfx/sources/`: prompt/provenance records and one untouched
  original per maintained variant. The first seven downloads have reference briefs;
  their exact provider prompts/settings and generation links were not supplied and
  must not be presented as confirmed. Keep native compressed downloads; WAV
  conversion cannot restore quality. Prefer FLAC for newly recorded lossless masters.
- `sfx/renders/`: ignored listening comparisons, waveforms and export QA reports.
- `client/assets/audio/sfx/{ui,vehicles}/`: the first six UI WAVs and cart Ogg,
  integrated at the user's request with actual playback/capture evidence; subjective
  listening acceptance remains pending.
  Future `foley`, `battle` and `ambience` banks require their own approved material.
  No source masters or duplicated takes belong in runtime banks.

The single-take exporter protects source paths, validates format/gain/trim settings,
and publishes only after full decode/QA succeeds. Its report preserves hashes and
measured size. A future catalogue validator must additionally check IDs,
category/voice limits, distance ranges, loop metadata and missing/unused variants.

## Export a downloaded take

The assistant sorts and identifies inbox files before export. Preserve the chosen
original in `sfx/sources/`; first audition exports belong in ignored `sfx/renders/`.
For example, from the repository root after the named input exists:

```sh
uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_sfx.py \
  --source asset_creation/audio/sfx/sources/ui_click.mp3 \
  --output asset_creation/audio/sfx/renders/ui_click.wav --kind one-shot
```

Use `--kind loop` and an `.ogg` output for cart rolling. Optional `--trim-start`
and `--trim-end` remove explicit seconds; `--fade-in-ms` / `--fade-out-ms` allow
tiny one-shot edge fades only. `--gain-db` applies static gain, without music
normalization. `--overwrite` is required to replace an export or existing report.
`--report` selects the QA destination; the default is ignored `sfx/renders/`.

The exporter checks both sample peaks and a 4× oversampled peak estimate; the
latter is not certified dBTP. It reports headroom and explicitly leaves listening
and loop-seam approval unverified. No downloaded take ships automatically.

Rebuild the supplied first pack with its explicit settings:

```sh
uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_first_sfx_pack.py --overwrite
```

The cart generator selects source seconds 0.55–6.85 and makes a 250 ms circular
equal-power overlap, yielding 6.05 seconds. Its unencoded endpoints are adjacent
source samples; this is not proof that the encoded loop sounds seamless. Audition
the actual Ogg repeatedly, including the runtime speed range. The first pack's
ignored `sfx/renders/first-pack-audition.wav` / `.mp3` and matching timestamp JSON
separate the UI cues with silence, then repeat the decoded cart three times.

Real-codec checks:

```sh
uv run --with imageio-ffmpeg==0.6.0 python -m unittest discover -s asset_creation/audio/tests -v
```

## Format and processing

- Positional effects: **mono**. Spatial playback downmixes stereo anyway. Keep
  stereo for music and deliberate non-positional ambience; test downmixes for
  phase cancellation rather than blindly dropping a channel.
- Very short/frequent effects: begin with 16-bit mono PCM WAV at 44.1 kHz for cheap
  repeated playback. Enable Bevy's `wav` feature for this first-pack integration.
  A 0.25 s mono cue is only about 22 KB before its small header.
- Longer loops/ambience: mono/stereo Ogg Vorbis as appropriate, starting around
  quality 3. Compare against the original at matched gain; raise quality only
  where an audible artifact warrants it. Music keeps its current Ogg path.
- Do not add MP3/AAC/Opus runtime support merely because a provider downloads that
  format. Preserve the original and export to the established runtime formats.
- Trim unwanted leading silence on responsive UI/impact cues, preserve the attack
  and intentional decay, and use tiny edge fades only where needed to avoid clicks.
  Do not apply the music tool's one-second fade or six-minute mastering assumptions
  to a footstep. A silent or corrupt export fails validation.
- Loudness normalization for an entire song does not meaningfully standardize a
  very short click. Set category-specific gain by matched listening, inspect sample
  and true peaks, and preserve transient shape. Start with at least 3 dB of per-file
  peak headroom; this does not guarantee headroom in a 32-voice mixed scene.
- A loop flag or generation prompt does not establish a seamless loop. Inspect
  endpoints and repeatedly audition the actual encoded loop, including at expected
  speed variation. Ordinary random creaks/steps are one-shots, not loops.
- Rate variation changes pitch too with the native sink. Keep ranges small and
  catalogue-controlled; use correct walking/trotting source patterns rather than
  stretching one loop over every gait. Avoid immediate sample repeats.

Start with a provisional 10 MiB runtime budget for the first effects pack, then
report actual bytes and resident encoded/decoded costs before expanding. Small WAV
effects can cost less CPU for little storage; highly compressed bytes are not a
guarantee of efficient simultaneous playback. Do not predecode every long track.

The seven first-pack exports total **328,164 bytes (0.313 MiB)**: 265,332 bytes of
UI WAVs and 62,832 bytes of cart Ogg. The cart download shrinks from 193,581 bytes;
the full runtime pack is slightly larger than the 323,451-byte originals because
the short effects use inexpensive PCM playback. Fully decoded equivalents are
798,474 bytes for mono PCM16 or 1,596,948 bytes for float32. Those are comparisons,
not measured decoder residency or a promise that every cue is predecoded.

## Acceptance

Check full decode, expected duration/channel count/rate, clipping, trim, exported
size and checksums. Compare first/middle/end where applicable. Review individual
effects and the worst-case in-game combination on headphones and speakers at
normal listening volume. Check no repetitive cadence, doubled footsteps, sharp
fatiguing highs, loop seams, delayed clicks, masking or sudden zoom transitions.

Use the runtime verification cases in AUDIO-DESIGN.md. Keep review outputs ignored
and commit only maintained sources, accepted runtime files, catalogue, tools and
intentional capture scenarios. A muted or screenshot-only test cannot approve
how an effect sounds.
