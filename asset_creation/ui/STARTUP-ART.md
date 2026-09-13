# FistWorld startup art

The launcher, name-entry modal and preparing-world view now implement the
approved 2026-09-12 concepts using native Bevy UI and reusable artwork. The
current game name is **FistWorld**, including both generated wordmarks and the
window and bundle display names. The earlier **FirstWorld** concept spelling is
retained only in historical prompts and review evidence. Real captures, the
size/state matrix and connected join-flow checks have passed and been inspected.
Generated concepts remain design
references, distinct from screenshots of the implementation.

## Escape and settings reuse

The 2026-09-13 Escape, Graphics, Audio and Controls menus reuse these same handles
and the existing ledger wood. They add **zero runtime image files or compressed
image bytes**. Navigation icons, heading symbols, arrows and keycap labels are
native UI; the live menu backdrop filters the existing scene target with the
shared startup material, without another world camera or full-size image asset.
Generated concepts and actual review captures stay in ignored `logs/`; the
maintained acceptance recipe is `capture/scenarios/pause-menu-tour.ron`.

## Runtime artwork and budget

`client/src/ui/startup/artwork.rs` owns one shared set of handles for all startup
states. The eight files below total **1,232,750 compressed bytes (1.18 MiB)** and
**10,007,248 base RGBA bytes (9.54 MiB)**. Paths are relative to the repository.

| Asset | Runtime path | Dimensions | Compressed bytes | Base RGBA bytes |
|---|---|---:|---:|---:|
| Village backdrop | `client/assets/ui/startup/launcher-village.jpg` | 1672 × 941 | 542,466 | 6,293,408 |
| Full FistWorld wordmark | `client/assets/ui/startup/wordmark.png` | 900 × 375 | 79,822 | 1,350,000 |
| Compact FistWorld wordmark | `client/assets/ui/startup/wordmark-compact.png` | 600 × 170 | 34,764 | 408,000 |
| Loading ring | `client/assets/ui/startup/loading-ring.png` | 256 × 256 | 7,641 | 262,144 |
| Loading star | `client/assets/ui/startup/loading-star.png` | 256 × 256 | 11,702 | 262,144 |
| Panel parchment | `client/assets/ui/startup/panel-paper.png` | 512 × 512 | 441,305 | 1,048,576 |
| Ivory input field | `client/assets/ui/startup/input-field.png` | 512 × 112 | 101,190 | 229,376 |
| Dark secondary button | `client/assets/ui/startup/dark-button.png` | 384 × 100 | 13,860 | 153,600 |

Three existing creator assets are reused through the same asset handles; they
are not copied into the startup directory:

| Reused asset | Runtime path | Dimensions | Compressed bytes | Base RGBA bytes |
|---|---|---:|---:|---:|
| Brass frame and corner plates | `client/assets/ui/creator/frame.png` | 512 × 342 | 23,296 | 700,416 |
| Gold primary button | `client/assets/ui/creator/journey.png` | 480 × 100 | 15,892 | 192,000 |
| Brass selector face | `client/assets/ui/creator/brass.png` | 96 × 96 | 4,677 | 36,864 |

The complete loaded set is **11 images, 1,276,615 compressed bytes (1.22 MiB)**
and **10,936,528 base RGBA bytes (10.43 MiB)**. This is an image budget, not a
measurement of total GPU memory: mipmaps, driver allocations, fonts, render
targets and the rest of the game are excluded. The backdrop shader samples the
same village texture for dimming and softness; it does not load a second large
blurred backdrop.

All new PNG sprites have genuine alpha outside their silhouettes. Compact metal
and wordmark sprites use indexed PNG. Paper and input interiors keep full-color
PNG because indexing visibly posterized their subtle fibers. The backdrop uses
JPEG quality 91, subsampling 0, optimized progressive encoding, without rescaling
or retouching. The compressed runtime assets are intentionally maintained.

The old, unchanged `ui/fistforce.png` logo was removed after confirming zero
production usages. The unused full compass composite was moved to ignored
`logs/startup-review/ornament-review/loading-compass-delivery.png`; production
uses its separate ring and star layers.

## Native presentation and behavior

The shared startup module owns artwork, backdrop, reusable widgets and motion.
The launcher and name-entry modules retain their own actions and network state.
Labels, text editing, focus, buttons and loading diamonds remain native controls;
no full-screen concept image is used as a runtime form.

- The launcher preserves hostname/IP input and saved presets in one compact
  field with a brass dropdown. Manual host/port edits are validated before
  connecting; keyboard editing supports selection and platform paste shortcuts.
- Connecting remains visible and recoverable while DNS runs off the frame
  thread. The name form freezes the submitted identity while joining or preparing
  and retains authoritative server validation and distinct error feedback.
- Preparing world has its own neutral parchment panel, independently rotating
  brass ring and three small gold diamonds that pulse in sequence. The world
  task exposes no measured fraction, so the UI never invents a percentage.
- Back, cancel and lost-connection recovery return to the launcher. The connected
  startup test exercises these paths with real production controls and networking.
- Protruding screw plates and narrow brass rails frame browned parchment edges.
  Text inputs use a recessed ivory/brass face; secondary actions have the same
  clipped bevel as the gold primary button. The compact wordmark omits the
  launcher's compass crest without clipping letter tops.

Keep the warm low-poly village, imperfect sandy roads, modest fenced gardens,
wildflowers, irregular fields, light chimney smoke and open meadows. Preserve
quiet space around the menu. Avoid slogans, modern imagery and decorative text
that competes with the functional controls.

## Sources, prompts and delivery

- [Original concept prompts](startup-art-prompts.json) retain the exact generated
  background/launcher/name/preparing requests and now record implementation
  status, loaded asset paths and measured image budgets.
- [Ornament prompts](startup-ornament-prompts.json) retain the wordmark and
  compass layer generation requests.
- [Refined skin prompts](startup-skin-prompts.json) retain the paper, input,
  secondary button and compact wordmark requests, including alpha corrections.
- [Final branding correction prompts](startup-branding-correction-prompts.json)
  record both **FistWorld** edits, exact spelling, approved alpha sources, rejected
  checkerboard drafts and the current delivery bytes. Original prompts above are
  preserved verbatim.
- [Delivery importer](import_startup_ornaments.py) only trims empty delivery
  margins, sizes and encodes approved sprites. Its optional full-compass output
  goes to ignored review storage rather than the runtime asset directory.

Image creation and background extraction used the built-in image generator.
Originals and discarded variants remain in ignored
`logs/startup-review/ornament-sources/`; alpha composites and byte/hash audits
are in `logs/startup-review/ornament-review/`. The original text-free backdrop
and full-screen concepts remain under `logs/startup-review/concepts/`.

## Review evidence

Actual original screens and their `.capture.json` files are under
`logs/startup-review/before/{menu,name,preparing}/`. The first implemented
captures are under `logs/startup-review/iteration-1/`:

- `menu/launcher.png`
- `name/join-game.png`
- `preparing/preparing-world.png`

Those historical PNGs show the first implementation before the refined skins.
The latest inspected **FistWorld** captures and paired `.capture.json` and
`.startup.json` files are under `logs/startup-review/fistworld-final/`. Normal
and rejection forms pass at 1600×900, 1280×720 and 1024×768. Their wood backing
and 5–8 pixel paper/header overlap close the seam even when feedback grows the
modal. The launcher tour checks hover, visible saved-server choices and their
actual stacking above Connect. The earlier full size/state matrix remains in
`matrix-2/` and `matrix-final/` under the same review root.

`fistworld-final/loading-preview.mp4` assembles 91 real renderer probes at 30 fps
from a continuous 60 Hz UI run. Its JSON verifies that all three diamonds pulse
in order, with stable geometry; the compass ring rotates independently. There is
no frame interpolation or generated animation in this review video.

`logs/startup-review/connected-final/report.json` records a passing ordinary
seed-12345 server/client run: keyboard navigation and cancellation, preset port
reset, invalid address/DNS recovery, long-name fitting, server name rejection,
real world preparation, hero creation and reconnect to the same PersonId, wallet
and cargo. Eight PNGs and their paired metadata were inspected. The live game
Window title was **FistWorld**. All owned test processes were stopped.

Final verification: workspace/all-target build and check, 1,387 tests passed
(21 intentionally ignored), and `git diff --check`. Capture scenarios and runner
are maintained; screenshots, video and test-world output stay ignored.

Offline name/preparing scenarios are explicit presentation fixtures; they do not
prove network timing or world-generation progress. The actual native
**FirstWorld** title, before the user’s final branding correction, was separately observed using CoreGraphics filtered to the
owned test process, recorded in `logs/startup-review/native-window-title.json`.
The capture harness uses its own diagnostic title.

Both final **FistWorld** wordmark sprites were inspected at their runtime size,
on dark wood to verify transparency, and in the final composed captures above.
