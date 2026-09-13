# FistWorld audio workshop

Prompts, original downloads, provenance and the Ogg export tool live here. Game
playback lives in `client/src/audio/`; generation never runs inside the game.

For new effects, start with [SFX_PIPELINE.md](SFX_PIPELINE.md) and the
[runtime sound-effects design](../../docs/AUDIO-DESIGN.md). They define the planned
cue catalogue, formats, zoom behavior and voice budgets. The exporter below is
currently for music; do not apply its normalization/fades unchanged to short SFX.
The [first cart/menu prompt pack](sfx/FIRST-PACK.md) lists the next eight cues and
the ignored `sfx/inbox/` folder where the user can drop ElevenLabs downloads.

## Musical direction

User listening feedback, 2026-09-13:

**Accepted:** the user chose **The Chronicler's Quill** for gameplay. Its six-minute
source and exact short prompt are recorded as `chroniclers_quill` in `tracks.json`.
The runtime Ogg is 4.06 MiB at -24 LUFS, with one-second entry/four-second exit fades.
Playback applies a 0.8 track gain (20% lower), then the Master and Music sliders.
Music defaults to 50%; existing saved levels are retained. This runtime balance
does not re-encode or change the mastered Ogg.
The user prefers to generate in ElevenLabs themselves: provide short prompts in
chat and retrieve a named track only when asked. Do not generate further variants
or take over their ElevenLabs session without a new request.

- Keep the calm, unobtrusive character of the first launcher experiment.
- Its opening took too long to establish itself and felt a little dark. The main
  idea should arrive within the first few seconds, with warmer harmony.
- The first village cue sounded like a children's lullaby. Aim for mature
  atmosphere and restrained grandeur: broad phrases, some depth and a sense of
  a large world. Avoid tinkling harp, nursery tunes and a rocking 6/8 rhythm.
- Gameplay cues should be long; the current candidates request six minutes.
- Violin leads and forceful foreground instruments fatigue the listener. Low,
  warm string support is an option, rather than an orchestral or fiddle lead.

The warmer folk revision also sounded too jolly and childlike, with an irritating
repeating two-note accompaniment. **The user chose a reset:** Valheim-like spacious
atmosphere, softer strings and no repetitive plucked rhythm. A second comparison
uses very soft fantasy-film underscore: music supporting the scene almost unnoticed.
The current prompts are `wilderness.txt` and `cinematic.txt`; the earlier
`launcher.txt` and `meadow.txt` are rejected experiments recorded in `experiments.json`,
not the current art direction. Choose the atmosphere before producing another
launcher revision. The launcher should still establish itself within a few seconds.

Build depth through evolving harmony, sustained warm texture and long irregular
phrases. Avoid the nursery/lullaby character, jaunty folk pulse, repetitive plucks
and big foreground themes. Refer to timbre, rhythm and mood in the prompts; create
original music rather than asking for another game's or film's melody.

**Later feedback, 2026-09-13:** subsequent prompts explored more melodic adventure
music, but the user still wants genuinely quiet RTS background passages with all
instruments occasionally stopping. A constantly sustained backing layer is not
the current goal. Short sparse phrases and real silence take priority over the
earlier sustained-texture direction. Chronicler remains the only newly accepted
gameplay recording; none of these later prompt ideas has replaced it.

Earlier comparisons, generated 2026-09-13 (retained as authoring candidates):

| Cue | Length | Export size | Measured loudness | ElevenLabs project |
|---|---|---|---|---|
| Wilderness atmosphere | 6:00 | 4.15 MiB | -24.03 LUFS | [FistWorld Wilds](https://elevenlabs.io/app/music/project/SgzTyZgAzqAzvX0HBhJn?selectedSongId=4D6o0gt5EhisXApkdjsA) |
| Soft film underscore | 6:00 | 4.49 MiB | -24.16 LUFS | [FistWorld Underscore](https://elevenlabs.io/app/music/project/J5dXL4pFOtccCjIrA1U2?selectedSongId=nrHgqvwL5AJyKptzLtxW) |

Both compressed exports fully decode and retain peak headroom. Their loudness is
matched for comparison. This establishes technical playback quality, not whether
the arrangement achieves the desired mood. The original two MP3s total 16.51 MiB;
there are no duplicate WAV masters or stems. These two experiments are not loaded
by the game; the selected Chronicler cue is the gameplay track.

## Files and storage

- `prompts/`: exact text submitted for each maintained candidate.
- `tracks.json`: track source, generation URL/settings, status and export settings.
- `sources/`: one original download per maintained candidate. Preserve native MP3
  if that is the download; converting it to WAV/FLAC cannot restore lost quality.
  For genuinely lossless future sources, keep FLAC rather than duplicate WAVs.
- `renders/`: ignored, reproducible listening exports and `.audio.json` QA reports.
  Keep rejected experiments here or recover them in ElevenLabs history; do not
  accumulate rejected variants, stems or duplicate masters in Git.
- `client/assets/audio/`: only cues intentionally integrated into the game.

The existing adventure source remains `../sound/gameintrosource.wav`. Its live
export is `client/assets/audio/music/game_intro.ogg`.
`client/src/boat.rs` requests it once during the opening voyage, and
`client/src/audio/music.rs` owns the actual playback. Preserve that cue. Adding a
file to this workshop does not make it play in the client; only the explicitly
integrated Chronicler cue and the existing opening cue ship as music.

## Generate in ElevenLabs

1. Open Music and paste the exact cue prompt. Select Instrumental, one variant,
   an explicit duration, and record the displayed model/version. The current
   candidates use Music v2.5 in the web application.
2. Disable automatic prompt enhancement when offered so the submitted direction
   stays explicit. Generate one candidate, listen, and revise from specific
   feedback before producing a library of similar songs.
3. Download the chosen original. Record its project/song link, duration, date,
   model and prompt in `tracks.json`. Keep the download unmodified in `sources/`.
4. Export and inspect the measured output below, then audition it at game volume.

For a future CLI generator, use the official [music API](https://elevenlabs.io/docs/api-reference/music/compose)
and `ELEVENLABS_API_KEY` from the environment, never a committed credential.
Browser model names and API model IDs can differ. There is no API key configured
for this workshop at present; the initial generations used the signed-in browser.
Do not automatically retry paid generation on ambiguous network failures.

## Compress and verify

From the repository root, with FFmpeg installed:

```sh
python3 asset_creation/audio/build_audio.py wilderness
python3 asset_creation/audio/build_audio.py cinematic
# Rebuild the selected runtime track from its original source:
python3 asset_creation/audio/build_audio.py chroniclers_quill --output client/assets/audio/music/chroniclers_quill.ogg --overwrite
```

Or use the small cached FFmpeg distribution without a system installation:

```sh
uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_audio.py wilderness
uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_audio.py cinematic
```

`FFMPEG=/absolute/path/to/ffmpeg` also works. The tool performs a full decode,
measures loudness, applies a measured normalization pass and gentle cue fades,
then encodes stereo **44.1 kHz Ogg Vorbis quality 3**. This matches the client's
existing Vorbis support. It re-decodes the result, checks duration, loudness and
peak headroom, and writes checksums/settings to the adjacent `.audio.json`.
Exports replace existing files only with `--overwrite`; original sources are
never overwritten. Two-minute and six-minute file sizes are measured, not assumed.

Mastering starting points are -21 LUFS for the launcher and -24 LUFS for gameplay,
with a -2 dBTP normalization ceiling and a post-encoding -1 dBTP acceptance limit.
These are project choices, not universal loudness requirements. Do not make the
background intrusive by raising its playback gain again. Default exports are
ordinary finite cues with faded ends, **not verified seamless loops**.

To ship an accepted cue, explicitly choose its runtime destination:

```sh
python3 asset_creation/audio/build_audio.py wilderness --output client/assets/audio/music/wilderness.ogg
```

Keep the generated report with authoring/review output rather than shipping it.
Always re-encode from the original, never from a previously compressed Ogg.

Exporter regression checks exercise real encoding, duration/headroom, source
preservation, overwrite refusal and rejection of silent input:

```sh
uv run --with imageio-ffmpeg==0.6.0 python -m unittest discover -s asset_creation/audio/tests -v
```

## Listening and playback

Before integrating, audition the first 15 seconds, middle and ending on headphones
and speakers; compare at matched loudness. Check for vocal artifacts, sharp flute
or string peaks, an overly sweet tune, big swells and an ending that feels cut off.
Listen to transitions too: a loop prompt does not prove a seamless loop.

Implemented client behavior:

- `client/src/audio/music.rs` owns one music player outside the spatial-footstep
  budget. Scenery rebuilds, camera movement and menu opening keep the same player.
- **Escape → Audio** provides Master, Music and Effects sliders, plus separate
  Music/Effects switches. Music mute pauses/resumes the existing background decoder.
  Preferences are saved to ignored `client_data/audio.ron` after a short drag debounce.
  Capture runs disable reading and writing that file using the same isolation flag
  as graphics settings. Effects remain audible when music is off.
- The adventure opening takes priority. The background begins after the actual
  opening sink finishes and four seconds of quiet. Turning off music during the
  opening discards that cue so it cannot resume late over ordinary gameplay.
- The Chronicler cue repeats after thirty seconds of quiet. These are finite
  faded cues, not an assumed seamless loop. Audio timing uses real time, independent
  of simulation speed. Disconnecting stops music and releases its retained handle.
- Music loads only when enabled and needed; a muted background retains its
  compressed source and paused decoder so resuming does not restart the song.

Future extensions:

- One primary cue and at most one fading partner; retain only the current and
  next compressed assets. Do not preload a growing soundtrack library.
- If a launcher cue is chosen later, fade it before the existing adventure cue.
- Gameplay tracks with quiet intervals between them, varied ordering and no
  immediate repeat. Do not restart music when zooming or entering another chunk.
- Broader day/night/biome choices can follow later, with enough hysteresis that
  walking along an area boundary does not repeatedly change the music.
- Keep birds, wind, cart wheels, work and UI sounds separate so they follow
  the world and the player's effects setting. Mono for suitable positional SFX;
  stereo for music. Short SFX need their own gain/fade settings, not this music preset.

Runtime integration should test state transitions, reconnects, mute/volume and
opening-cue overlap. A screenshot cannot establish that audio sounds right.
`capture/scenarios/ui-music.ron` rehearses the actual menu handlers and actual Bevy
audio sinks; its `.music.json` companions record decoder position and paused state.
The end-of-track check silently runs the real decoder at 100× until completion
rather than waiting six minutes. Opening priority is an explicit request fixture; it does not
claim a complete connected boat voyage. Unit tests also cover disabled cold starts,
pending loads, opening priority, state cleanup and preference persistence.

Verified 2026-09-13: workspace check and full playtest build passed; workspace tests
passed (1,399 passed, 21 intentionally ignored), as did both real-codec export tests.
All seven 1280×720 captures in ignored `logs/captures/music-review-final/` were
inspected with their capture and music metadata. The retained background decoder
paused/resumed, the opening replaced it without overlap, and the actual six-minute
Vorbis source completed into the quiet interval.
