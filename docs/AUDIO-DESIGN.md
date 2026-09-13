# FistWorld sound-effects design

Reviewed 2026-09-13 against the repository and the installed `bevy_audio 0.19.0` /
`rodio 0.22.2` source. This is the maintained implementation plan and sound-design
contract. **The first seven-cue menu/cart slice is implemented with native
Bevy audio; its current scope is recorded below. The broader movement, army,
battle and ambience framework remains proposed.** The user supplied the seven
recordings; no additional effects were generated. Real playback, recorded output and visual checks are recorded in
[AUDIO-VERIFICATION-2026-09-13.md](AUDIO-VERIFICATION-2026-09-13.md). Subjective
listening and comparative performance evidence remain pending.

Asset authors follow [the SFX pipeline](../asset_creation/audio/SFX_PIPELINE.md).
Music sources, compression and accepted cues remain in the
[audio workshop](../asset_creation/audio/README.md).

## Decision

Keep native Bevy audio for the first effects implementation. Put one client-owned
cue catalogue, voice allocator and mixer above it. Gameplay/UI systems describe
what happened; the audio domain decides whether and how it is heard. A voice is an
actively allocated playback instance, not an NPC or a file in the catalogue.

The main scaling tool is fewer relevant voices. At close range, hear individual
people and objects. At town/battle range, blend into a few activity groups. At map
range, let detail become quiet. Zooming out must not make every newly visible
person audible. Calm passages and silence are part of the intended experience.

## First-pack implementation scope

| Area | Current scope and boundary |
|---|---|
| Sources and export | Seven byte-preserved user MP3s, provenance/reference prompts, reproducible static-gain exports and source hashes in `asset_creation/audio/sfx/first-pack.json`. Exact provider prompts/settings were not supplied. |
| Runtime assets | `ui_click`, `ui_confirm`, `ui_reject`, `book_open`, `book_close`, `page_turn`: mono PCM16 WAV at 44.1 kHz. `cart_roll`: mono quality-3 Vorbis, 6.05 seconds after a 250 ms circular overlap. Encoded total: 328,164 bytes. |
| UI | Up to four reserved voices, typed cue requests and semantic event coalescing. Specific book/page actions suppress redundant generic clicks. Initial state, hover and ordinary scrolling are silent. |
| Carts | Up to four native spatial rolling loops from observed physical porter-cart movement. Stable character ownership survives rig/LOD swaps; rest, loss of audibility and removal fade/release voices. Ground-focus distance and camera zoom control gain and a live low-pass filter. |
| Creaks | Explicitly skipped by the user. No creak asset, random creak emitter or always-on creaking layer belongs to this pack. |
| Music and controls | Existing music scheduling and quiet gaps are preserved. A dedicated Audio page with Master/Music/Effects sliders and separate Music/Effects toggles is implemented; gain changes reach both active and newly started voices. Effects includes this pack's UI and carts. |
| Future work | Surface footsteps, horses, armies, battle, environments, broad category arbitration and the full-scale audio performance lab below remain planned. Four cart voices are the present world-audio slice, not implementation of the future 32-world-voice allocator. |
| Acceptance | Full asset decode, mono/rate/duration checks, ≥3 dB peak headroom, source preservation and eleven audio exporter/preprocessor tests pass. Real application recordings, connected cart/UI checks and inspected visual captures pass; see the verification record. Encoded seam audition, final perceptual balance and comparative performance acceptance remain pending. |

The original desert walking loop followed bodiless `Player` commander entities;
the first-pack migration removes that legacy producer/manager path instead of
presenting it as embodied NPC footsteps. Fresh movement audio must come from
physical actors. The sections below remain the broader design contract; only the
bounded menu/cart slice above is in the current integration.

## Bevy 0.19 rules

- `AudioPlayer` starts playback when its asset is available. Create short-lived
  effect entities with `PlaybackSettings::DESPAWN`; retained loops require explicit
  ownership, fades and cleanup. `ONCE` alone leaves the entity behind.
- `PlaybackSettings` initializes playback. Set live gain, pause and speed through
  `AudioSink` / `SpatialAudioSink`. Changing `GlobalVolume` does **not** update
  already-playing sounds. Keep native global volume at unity once the project
  mixer owns gain; apply master gain once to both initial settings and live sinks.
- Native spatial audio is stereo positioning, not HRTF or geometry acoustics.
  Rodio downmixes positional material to mono and applies directional and inverse-
  square ear-distance gains. Do not add a second inverse-square attenuation layer.
  Calibrate `SpatialScale`, ear separation and reference distances together.
- `AudioSource` keeps encoded bytes in an `Arc`; each playing voice has a decoder.
  Sharing an asset handle does not mean decoded samples are shared. Loading reads
  the full compressed file into memory; this is not streaming from disk.
- The project's `vorbis` feature uses the normal Vorbis backend. Bevy's 0.19
  migration guide specifically cautions about the alternate `symphonia-vorbis`
  backend for looping/spatial processing. Do not switch codecs/backends casually.
- Seeking is decoder-dependent. The chosen Vorbis music did not support seek in
  our existing capture rehearsal. Do not design loop virtualization around exact
  seek/resume of every sound, or warm voices by silently decoding all of them.

## Shared distance treatment (implemented)

`audio::perspective::WorldSoundProfile` centralizes reference distance, hearing
radius, zoom gain and tonal falloff. The cart profile uses a 6 m reference distance,
38–54 m outer fade and zoom attenuation beginning above 12, becoming silent at
240. Its authored gain is 0.14. This is an artistic RTS perspective, not a physical
acoustics simulation. UI and music bypass this world treatment.

`audio::filtered` extends native Bevy through `Decodable`/`AddAudioSource`, without
a second mixer/backend. Short mono effects decode on the compute pool once and
share immutable PCM across voices. The cache permits eight entries, each up to
2 MiB of PCM; active entries cannot be evicted. The current cart occupies about
1.02 MiB regardless of whether one or four carts play. Failed decoding is reported
once per retained cache entry. This bounded cache is not for long music/ambience;
it persists across world exits and evicts unused entries when new clips need room.

Each admitted voice owns two nonresonant low-pass stages. A lock-free scalar
control is polled every 128 samples and coefficients ramp over 20 ms. There are
no allocations or ECS/asset access in the sample loop. The cart cutoff varies
between 2.4 kHz and 700 Hz based on ground distance plus an artistic camera-height
term. Even the closest view is heard from above: there is no bright, close-mic
cart mix at minimum zoom. Native distance attenuation still happens exactly once. Pitch and tone
change without replacing the playing voice.

**Filtered sources own their loop internally and require `PlaybackSettings::ONCE`.**
Bevy's ordinary `LOOP` wraps the decoded stream in Rodio's buffered repeat, which
would freeze a filtered first pass or buffer an infinite stream. A scheduled guard
corrects this combination before native playback; regression tests cover continued
filter changes after repeated loops. The cart lifecycle explicitly stops and
despawns both pending and active players; `ONCE` alone does not clean up entities.

For another short world cue: export mono, define its profile, obtain shared PCM
from the cache, admit a bounded voice, and publish that voice's cutoff while its
source moves. Keep event/motion detection in its own producer. This supplies the
distance/filter building blocks; footsteps, combat and a combined world allocator
still require their own integration. Encoded assets and source recordings are
unchanged by this tuning pass.

Primary references: [AudioPlayer](https://docs.rs/bevy/latest/bevy/audio/struct.AudioPlayer.html),
[GlobalVolume](https://docs.rs/bevy/latest/bevy/audio/struct.GlobalVolume.html),
[spatial listener](https://docs.rs/bevy/latest/bevy/audio/struct.SpatialListener.html),
[0.19 migration guide](https://bevy.org/learn/migration-guides/0-18-to-0-19/).
The installed 0.19.0 source is authoritative for implementation; `latest` docs can
advance beyond the locked version.

## Listening and zoom

Use one dedicated audio listener near `CommanderCamera.focus`, just above sampled
ground, oriented by camera yaw. Keep it separate from rendering-camera height and
from the hero, so free camera movement sounds like looking around the world.
Reconcile the existing camera listener so two listeners never compete.

Use ground distance for admission and native spatial distance for playback. The
catalogue supplies each cue's spatial scale/reference distance and finite hearing
radius; the latter adds only a smooth outer cutoff. Apply a separate artistic zoom
envelope. Model those same gains in the admission estimate so inaudible sources
do not consume voices. Compare at several camera yaws and asymmetric left/right
source positions, not just directly beneath the camera.

| View | What the player should hear |
|---|---|
| Close to a person or yard | Sparse surface-specific steps, cart wheels, a door latch or nearby work. Selection gets subtle immediate feedback. |
| A street or town | A few moving carts and small activity clusters. Individual steps recede; occasional work, birds or market activity establish place. |
| An army or battle | A few spatial marching/combat groups that reveal where activity is occurring. Nearby selected troops retain some detail. |
| Regional/world map | Local wheels, steps and work fade out. Quiet broad ambience, music and relevant UI alerts remain. |

Derive detail from camera zoom or visible ground width, independent of graphics
render scale/resolution. Tune band boundaries in actual scenes; do not treat world
distance and zoom as interchangeable. Blend neighboring bands, using hysteresis
for source admission. Starting fade targets: roughly 150–400 ms for loop start/stop
and 0.5–1.5 seconds for ambient/zoom transitions. These are listening hypotheses.

Do not play a flourish on every zoom notch or every object hover. Zoom reveals
existing activity. A selected source may receive a modest priority boost while
still obeying distance, visibility/knowledge and gain limits. Hidden/unobserved
enemy activity must not leak through sound.

## Cue requests and ownership

Broader flow (the first slice supplies UI requests and desired cart-loop state):

```text
accepted UI action / local movement / replicated action result
                         |
                 typed cue or loop intent
                         |
            coalescing + expiry + zoom/distance
                         |
              priority and voice admission
                         |
               one gain/lifecycle owner
                         |
                  Bevy audio sinks
```

Expose a small typed `SoundCue` identifier and a `SfxRequest` message with source
identity or position, event key, intensity and real-time creation stamp. IDs, asset
handles and resolved settings are cached; avoid path strings in producers.
Long-lived activity uses desired loop state keyed by stable source/group ID rather
than repeating a start message every frame. The catalogue owns variation, gain,
spatial parameters, cooldown, priority, maximum instances and loop behavior.

Keep additions focused under `client/src/audio/`: `catalog`, `requests`, `listener`,
`voices`, `mixer` and source adapters for UI, locomotion, vehicles and battle as each
lands. Keep `mod.rs` orchestration-only. Replace the old manager/remote loop path
as its replacement lands; do not maintain a second competing sound manager.

UI/world producers run after their domain has accepted the action or updated its
presentation state. Drain requests and update listener/voice transforms in an
explicit PostUpdate set before transform propagation; native Bevy audio consumes
the propagated transforms afterward. Do not rely on unspecified system ordering.
Use `Time<Real>` for cooldowns, fades and request expiry. High game speed must not
turn sound into accelerated noise or cause missed effects to play in a burst.

## Specific producers

**UI:** A restrained wood/brass tap for accepted controls; leather/paper movement
when the encyclopedia actually opens, a softer close, and short page rustles when
navigation changes. Trigger panel sounds from semantic state transitions so N,
Escape, the book button and other entry points agree. Initial resource creation,
rebuilds, a held click, disabled controls and clicks blocked by a modal are silent.
For an action with a specific cue, suppress its generic click. Default hover and
scroll feedback to silent; rapid adjustments get a throttled cue. One box-selection
or formation order produces one acknowledgement, not one per soldier. A local
command acknowledgement means input accepted, not that a server action succeeded;
use actual response messages for success/failure cues.

**People and horses:** Use smoothed local presentation movement with
`CharacterMotion`, not commander transforms or walking intent alone. The rules in
`hero/footprints.rs` already distinguish ground travel, mounted/aboard/swimming and
teleport-sized changes. Apply those rules to the bounded audible set, not another
full-character scan. Near steps should align with the same locomotion phase/contact
timing as the animation where available; a distance accumulator is the fallback.
Drop missed contacts after a hitch. Never emit catch-up steps or hoofbeats for a
stationary actor merely playing a walk animation. Use a small non-repeating sample
bag and restrained pitch/gain variation. Mounted actors get hoof/gear cues rather
than human footstep loops.

**Carts:** `PorterCartState` and the distance-driven wheel logic in `hero/carts.rs`
provide the model. One admitted cart gets a rolling loop with smoothed speed/gain
plus occasional low-rate wood/axle creaks. Stop/fade at rest, inside buildings,
on unload or despawn. A rig/mesh LOD swap must not restart its sound; track the
stable owner and movement state. Surface choice distinguishes dirt, paving and
wood where the game has that information.

**Armies:** Use `MemberOfBattalion` and nearby spatial clusters. A stretched group
may need several audible clusters; multiple nearby battalions must still share the
global budget. Cluster bounds, moving fraction, speed and mounted/foot composition
control march density. Aggregate work at a lower cadence with incremental membership.
Do not raise gain in direct proportion to troop count. Crossfade individual detail
and group textures, avoiding a fully doubled marching layer during transitions.

**Battle:** Derive fresh attacks/releases/impacts from existing replicated action
timestamps/results and local animation timing. Keep per-source event cursors;
initial replication, interest re-entry and duplicate updates must not replay old
battles. Impacts require the corresponding result, not just attack intent. Nearby
bow release, shield/weapon contact, cavalry and catapult impacts get distinct cues;
distant fighting becomes a few group textures driven by actual engagement activity.
No per-footstep/per-frame sound packets. If an essential event is absent from
current contracts, add a compact authoritative gameplay event with normal protocol
versioning, not a network message carrying audio filenames.

**Environment:** Sparse, varied one-shots around limited ambient beds. Wind, birds,
leaves, water and workplaces depend on location and actual activity. Do not make
every tree, house or yard a looping emitter. Keep ambiences and music separately
adjustable, with quiet stretches rather than continuous blanket noise.

## Voice budget, assets and failure behavior

Proposed expanded tuning budget: **32 world voices + 4 reserved UI voices + up to 2
music voices**, including fading tails and pending starts. The first pack has a
four-cart world cap plus four reserved UI voices; current music needs only one.
This is a starting cap, not a benchmark or a claim about a particular Mac.
Within world audio start with maxima of 12 close movement/work voices, 4 vehicle
voices, 10 battle/group voices and 6 environmental voices; tune or allow borrowing
within the fixed total after listening. Keep separate per-cue/per-source caps.

- Reject or coalesce before spawning a player/decoder. Reserve UI/important
  feedback independently so a large battle cannot steal every click or alert.
- Re-score bounded candidates around the listener at about 10 Hz, reading nearby
  streamed/spatial buckets. Update admitted source transforms and fades per frame.
  Reuse a suitable client index if available; otherwise maintain a small local
  activity index incrementally. Do not scan the global population every frame.
- Score estimated audibility, category priority, recency and modest selected-source
  relevance; retain a voice until a new candidate is meaningfully better. Keep a
  short grace period for border crossings rather than rapidly starting/stopping.
- Paused music may retain its decoder; inaudible world loops become cheap logical
  state with no physical sink. Restart a suitable loop with a fade on re-entry.
  Exact seek continuity is reserved for sources/backends where it is supported.
- Use a bounded request backlog, TTLs and start-rate limits. Delayed clicks/hits
  lose meaning: discard stale requests (initial target about 100–250 ms), including
  while a bank/device is unavailable. Never replay an accumulated startup backlog.
- One allocator owns all removal and index reconciliation. Account for naturally
  ended sounds, voice stealing, source despawn, failed loads, no audio device,
  connection loss and menu transitions. Release world banks when no longer needed.
- Preload the small UI bank at launch. Load near-world/battle banks ahead of likely
  use and retain strong handles while needed. Avoid triggering large loads for the
  first impact. Per-cue failure is logged once and does not gate the whole system.

## Mixing and controls

The first-pack Audio page provides the user's requested **Master,
Music and Effects sliders**, plus separate **Music and Effects on/off toggles**.
Persist values in the existing audio settings file with backward-compatible
defaults for its original music switch. Effects controls the six UI cues and cart
rolling in this slice. Toggle state is independent of the saved slider level;
muting and re-enabling must preserve that level. Apply master/category gain once
to active sinks and newly started voices, including retained music.

Separate UI/Ambience categories are a later design option alongside broader
locomotion and battle banks; they are not controls already supplied by this pack.

Exactly one layer computes each voice's final gain from authored gain, user master
and category gain, zoom envelope, fade and any ducking. Native spatial distance gain
is applied separately by the backend, once. Express mix settings in dB and convert
once for Bevy; avoid independent systems overwriting volume on the same sink.

Keep UI clear at low level. A major alert may briefly lower other groups by a few
dB; ordinary clicks must not make music pump. Consider a subtle world reduction
while reading the encyclopedia, while retaining gameplay alerts. Do not stop world
simulation or constantly restart loops as a side effect of opening a panel.
Keep real mixed-output headroom; individually safe files can still clip together.
Loudness sliders use smooth/perceptual changes, with short ramps to avoid clicks.

## Asset workflow and implementation order

Follow [SFX_PIPELINE.md](../asset_creation/audio/SFX_PIPELINE.md). Start with a small
coherent pack. The seven supplied UI/book/cart recordings are the current slice;
the user omitted creaks. Dirt/paving steps, hoofbeats, marching and battle
transients remain future additions. Approve the current mix in the game before
requesting more variants. No generation or provider session use is authorized by
this planning document.

1. Complete the seven-cue bank, bounded UI/cart lifecycle, Audio settings and ground
   listener, replacing the legacy commander walking loop. Finish real UI capture,
   encoded-loop listening and connected cart/mix QA; counters/tests alone do not
   establish acceptance.
2. Add surface-specific physical footsteps as a separate requested slice. Expand
   catalogue/arbitration only when new banks need them.
3. Group marching, mounted movement and zoom transitions in the existing battle lab.
4. Combat, workplaces and sparse environmental detail; broaden the pack only after
   measuring and listening. Tune categories in the full mix rather than in isolation.

## Verification gate for implementation

Run the normal workspace check/tests. Add meaningful checks for live/new-voice
mute, request expiry/coalescing, disabled UI, semantic open/close, budget recovery,
source removal, decoder failure and state transitions. A virtual source must not
own a sink, and priority changes must admit a newly closer mover despite idle peers.

Use the real Bevy capture harness for affected UI/camera/animation views and inspect
PNG plus capture metadata. Extend the existing music rehearsal style with audio
evidence: active/pending voices by group, admitted/dropped requests and reason,
asset bytes, starts/stops, event deduplication, per-system timings and mix gains.
Pictures and log assertions do not prove audio quality: record the actual device
mix/loopback where available and audition it on speakers and headphones. Verify
recordings actually contain application audio rather than assuming video does.

Connected lab cases: single walker and cart start/stop; panning away/back; continuous
zoom through all bands; normal town; roughly 250 troops in five moving battalions;
mixed infantry/archery/cavalry/catapult engagement; repeated UI interaction during
battle; frame hitch; high game speed; LOD/house construction; disconnect/reconnect;
and no device/missing asset. Check quiet gaps, repeated-sample fatigue, loop seams,
clicks, stereo placement, clipping and disappearing/restarting audio.

Compare sound off/on in the same seeded scene and camera path, with warm assets
and concurrent render jobs accounted for. Record frame-time p50/p95/p99, audio
systems' cost, active decoder count, loaded bytes and any audible underruns. Measure
on an older target Mac when available; the M5 alone cannot establish that result.
Only adopt numeric performance acceptance targets after obtaining this baseline.

## When to reconsider the backend

`bevy_kira_audio 0.26` supports Bevy 0.19 and provides useful fade/channel controls.
It does not automatically provide voice virtualization or solve per-NPC work.
Its channels are logical handle groups, not Kira's full DSP track interface.
Keep backend details behind the playback owner; no parallel native/Kira mixers.

Revisit when profiling or the actual design requires audio-thread fades through
long frame stalls, clock-synchronized music, shared reverb/filter routing, true bus
compression/limiting or streaming. Evaluate the integration exposing those features
rather than assuming the wrapper exposes all of Kira. Advanced occlusion/HRTF is
not needed for this first RTS soundscape.

References: [Bevy Kira compatibility](https://github.com/NiklasEi/bevy_kira_audio),
[Kira mixer tracks](https://docs.rs/kira/0.12.0/kira/track/index.html),
[Kira effects](https://docs.rs/kira/0.12.0/kira/effect/index.html).
