# First cart and menu sound pack

Requested and supplied 2026-09-13. The user provided seven named MP3 downloads from
`/Users/terminator2/Downloads` and requested compression, fixes and game integration.
The original files are preserved byte-for-byte in `sfx/sources/`. **The user
explicitly skipped `cart_creak`; it is not an asset or playback layer in this pack.**

The six UI effects are mono 44.1 kHz PCM16 WAVs; cart rolling is mono quality-3
Vorbis Ogg. The native Bevy integration reserves up to four UI voices and four
spatial cart loops. UI requests coalesce semantic events; cart sound follows
observed physical movement and fades with stopping, distance and zoom. The
dedicated Audio settings page with Master/Music/Effects sliders and separate
Music/Effects toggles is being completed as part of this integration.

Asset decode/peak checks and exporter tests pass. **Encoded-loop listening,
actual playback and visual checks are recorded in
[the verification record](../../../docs/AUDIO-VERIFICATION-2026-09-13.md); subjective listening remains pending.** Repeated-loop previews
are review material, not approval evidence. Provider generation links, exact
prompts/settings and account-specific rights details were not supplied; the
following prompts are reference briefs rather than confirmed provider inputs.

## Prepared exports

| Cue | Export duration | Trimmed start / end | Static gain |
|---|---:|---:|---:|
| `ui_click` | 0.280 s | 0 / 0.600 s | −1 dB |
| `ui_confirm` | 0.531 s | 0.039 / 0.310 s | −4 dB |
| `ui_reject` | 0.700 s | 0 / 0.180 s | +5 dB |
| `book_open` | 0.560 s | 0 / 0.240 s | +12 dB |
| `book_close` | 0.310 s | 0.290 / 0.200 s | +2 dB |
| `page_turn` | 0.622 s | 0.028 / 0.150 s | +4 dB |
| `cart_roll` | 6.050 s | Source interior 0.55–6.85 s; 250 ms circular overlap | 0 dB |

All exports have at least 3 dB of measured peak headroom, including the 4×
oversampling estimate. `ui_confirm` needed attenuation because its MP3 decoded
slightly above 0 dBFS. `book_open` is unusually quiet; it keeps static +12 dB gain
and needs cue-level balancing in the actual mix. No music normalization, limiter
or compressor was applied.

The runtime pack is 328,164 bytes (0.313 MiB). The cart is 62,832 bytes, about 68%
smaller than its MP3; the short PCM cues make the combined runtime size slightly
larger than the originals. [first-pack.json](first-pack.json) maintains provenance,
hashes, processing settings and measurements. Rebuild from the repository root:

```sh
uv run --with imageio-ffmpeg==0.6.0 python asset_creation/audio/build_first_sfx_pack.py --overwrite
```

Ignored listening previews live in `sfx/renders/first-pack-audition.wav` and
`.mp3`, with cue start/end times in `first-pack-audition.json`. The cart-only
`cart_roll.three_loops.wav` decodes the actual Ogg and repeats it without extra
fades or gaps; joins occur at 6.05 and 12.10 seconds.

## Original reference brief

Use one shared small family of UI sounds across the game, rather than a unique
recording for every button. Begin with one candidate per cue; keep any alternative
takes supplied by ElevenLabs available for comparison instead of generating more.
Loop only the rolling sound. Avoid automatic prompt enhancement for this first
comparison. Approximate durations below are generation settings, not prompt text;
we will trim unwanted leading/trailing silence afterward.

| Suggested filename (keep actual download extension) | Approx. duration | Prompt |
|---|---|---|
| `cart_roll` | 8 seconds, looping | A small wooden handcart rolling steadily over a packed dirt path. Soft wheel rumble and light wooden rattling. Close, dry sound. No footsteps, voices or background ambience. |
| `cart_creak` (skipped) | 1 second | One short, gentle wooden axle creak from a small handcart under load. Natural and understated, with a light rattle. Isolated dry sound. |
| `ui_click` | 0.5 seconds | One soft, crisp wooden click with a tiny muted brass tap. Tactile button feedback for a medieval game. Very short and dry. No music or reverb. |
| `ui_confirm` | 0.5 seconds | One small, satisfying brass latch snapping into a wooden fitting. Warm, restrained confirmation sound for a medieval game. Short and dry, no bell or music. |
| `ui_reject` | 0.5 seconds | One gentle dull wooden knock, like a small latch that will not open. Subtle unavailable-action feedback. Short, dry and unobtrusive. No buzzer or music. |
| `book_open` | 1 second | A small leather-bound book gently opening, with a soft leather creak and brief paper rustle. Close, delicate and dry. No background sounds. |
| `book_close` | 1 second | A small leather-bound book gently closing. A soft padded thump and tiny paper rustle. Close and understated, without a loud slam or reverb. |
| `page_turn` | 1 second | One parchment page turning gently. A brief, delicate dry paper rustle. Quiet menu navigation sound, no background ambience. |

The ordinary click covers buttons, toggles and back/cancel controls; confirmation
and rejection cover their distinct results. Book/page cues cover encyclopedia
transitions. Hover and ordinary scrolling remain silent. The final runtime audio
system prevents a book action also producing a redundant generic click.

## Future download handoff

1. Download the chosen takes, preferably lossless if ElevenLabs offers it; otherwise
   retain its native download without converting it. Use the names above when
   convenient; suffix variants `_01`, `_02` rather than overwriting earlier takes.
2. Put them in the inbox. Preserve the generation link/prompt/settings if available,
   or paste the links in a small text file beside the downloads.
3. Tell the assistant the files are ready. It will inventory and identify each take,
   preserve selected originals, record provenance, trim precisely, export compact
   mono files, check peaks/duration/full decode and prepare a listening comparison.
4. Audition at normal game volume before shipping. A requested loop must be checked
   at its seam. Creaks are separate intermittent one-shots, not a permanent squeak
   baked into every rotation.

The [SFX pipeline](../SFX_PIPELINE.md) governs format, source preservation and QA.
Broader footsteps, horses, armies, battle and ambience effects remain future work;
this seven-cue integration does not establish their implementation or acceptance.
