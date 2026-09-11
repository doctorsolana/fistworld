# Encyclopedia visual implementation plan

Status: implemented and visually verified, integrated into `main` on
2026-09-11. [Verification results and limitations](ENCYCLOPEDIA-VERIFICATION.md)
record the actual captures, connected tests and delivery budget.
The requirements below retain the approved design
brief. The five generated images are visual references; names, quantities,
portraits, buildings and available controls come from actual game data.

## 1. Visual target and reference package

Keep local review images in the ignored `logs/retained-encyclopedia/concepts/encyclopedia-2026-09-11/`
directory: `people.png`, `places.png`, `retinue.png`, `army.png`, `companies.png`.
These are design references, not runtime textures or regression baselines. The
four-page generation prompts are retained there in `prompts.md`.

Match at a 1600×1000 reference viewport before adapting to 1280×720 and wider
windows. Measure the outer frame, header, tab row, sidebar widths, portrait sizes,
section spacing, line weights and text hierarchy from the approved references.
The current 1240-design-pixel shell is narrower than the concepts; simply changing
its colours will not reproduce their composition.

Required visual details:

| Detail | Implementation target |
|---|---|
| Paper | Warm ivory with fine low-contrast fibre, subtle variation and gently darkened edges; calm surfaces behind text |
| Binding | Dark walnut/leather, fine brass outlines, shallow page-edge shadow and consistent depth |
| Brass corners | Separate shaped corner plates with uneven patina, edge wear, a visible screw/rivet and a small contact shadow |
| Buttons | Slightly imperfect authored edges, restrained bevels and warm selected fill; consistent hover, press, focus and disabled treatments |
| Typography | Cinzel-style headings and a clean, readable book serif for dense records; compare real font rendering to the reference before choosing the final bundled reading face |
| Portraits | Sharp actual low-poly characters, circular brass frames, soft village/workshop/woodland backgrounds |
| Buildings | Recognizable canonical buildings with consistent framing, light and soft background separation |
| Settlement vignettes | A softly feathered village illustration at the record header, small matching directory thumbnails |
| Page structure | Five peer tabs, clear selection, quiet section dividers, readable lists and useful negative space |

Remove `Chronicles of the realm` from the actual footer. It remains in the current
implementation even though it was removed from the four later concept images.
Retain the small close-key hint. Avoid additional slogans and decoration behind copy.

## 2. Shared artwork and typography

Build a maintained UI art kit before reskinning all the pages. Use the approved
images as references for text-free paper, binding, corner plates, medallions,
buttons and small symbols. Image generation may author organic material detail;
prepare the resulting pieces as controlled runtime assets with documented sources.
Do not extract complete screenshots into clickable panels or bake text into art.

- Separate paper fill, border, corner hardware and shadow layers. Corners retain
  their shape and size when the window changes size.
- Use suitable tiling or nine-slicing for surfaces. Preserve positive centre
  dimensions at every supported scale; test against the prior clock seam regression.
- Give irregular buttons stable rectangular hit areas. Small deterministic artwork
  variations can create character without changing layout or flickering each frame.
- Keep existing `UiButtonStyle`, `UiButtonLabel`, `InteractionDisabled`, focus handling
  and spring motion. Define shared textured appearances through the foundation,
  rather than painting a separate interaction system in each page.
- Add an explicit reading-font role in shared typography if comparison shows the
  existing MedievalSharp body is too different from the concepts. Bundle the chosen
  font and licence, load it once, and verify punctuation, numbers and small text.
- Keep text native and crisp at output resolution, independent of 3D render scale.

Suggested ownership: shared UI skin under `client/src/ui/`, authored assets under
`client/assets/ui/`, maintained preparation sources under `asset_creation/ui/`.
Promote reusable HUD artwork deliberately and migrate its callers; do not leave
duplicate HUD and encyclopedia implementations of the same frames or states.

First gate: an in-engine material/typography sheet showing paper, corner hardware,
medallions and every button state at normal and small UI scales. Compare enlarged
crops with the reference before building out the full book.

## 3. Shared portrait and illustration pipeline

The current HUD portrait implementation is reusable, but is tied to current
Selection and caches only sixteen 192px portraits. It also copies source geometry
and textures for uncached outfits. Multiplying that path across a roster would
create avoidable main-thread work.

Extract a shared thumbnail service with the HUD and encyclopedia as consumers:

1. Cache immutable character source geometry/material data once per asset revision.
2. Retain optional last-observed `HeroOutfit` with the durable `PersonId` record.
   `PersonRecord` does not currently store appearance. Handle delayed appearance
   components, outfit changes, demotion out of replication range and session reset.
3. Render actual clothes, hair and skin in a consistent flattering pose/light.
   Keep framing and masks explicit for large portraits and small list medallions.
4. Composite over a small prepared catalogue of softly blurred canonical scenery.
   Choose a stable backdrop from known context; it is decorative, not a live
   indicator of a person's current location. Blur the background, not the face.
5. Use silhouettes or the appropriate crest when appearance is unknown. Never
   invent a specific likeness for a person whose appearance is unavailable.

For business and Moot Hall pictures, render the canonical building variants offline
through the real Bevy capture harness. Maintain intentional runtime thumbnail assets
and their source scenarios. Match the actual physical building variant when known;
settlement tier alone is not proof that an upgraded hall has been constructed.
Regenerate derived art when its source model changes. Company rows reuse these
building pictures; company headers use a restrained emblem.

For settlement directory pictures and the wide header image, begin with a small
approved catalogue of village illustrations by known environment and settlement
type. This matches the concepts without claiming to photograph an exact layout.
Exact settlement panoramas are an optional later refinement: only from legitimately
observed settlement data, captured in one bounded job and retained as a last-observed
image. Never move terrain streaming focus or fetch hidden regional detail for a UI
thumbnail. Unknown places retain a crest/illustrated fallback.

Initial performance limits to verify, rather than assumed performance claims:

- One portrait worker; selected detail first, visible rows next, small scroll overscan.
- Deduplicate requests; no generation for every person in the world or every hidden tab.
- Start with 192×192 list images and up to 384×384 detail portraits; prepared landscape
  images sized for their actual display. Tune resolutions after inspecting captures.
- Start with a 32 MiB runtime thumbnail texture budget, separate from immutable source
  assets. Pin visible images and evict offscreen entries by least recent use and bytes.
- Invalidate on appearance/source revision, not money, activity updates or repeated
  selection. Clear world-specific cached knowledge and pending jobs on session change.
- No continuously rendered camera per portrait and no live full-screen blur requirement.
- Measure queue length, cache bytes, CPU preparation, upload work and cold/warm opening.

## 4. Perfect the shell and People page first

Build the full frame and People view as the first finished implementation slice.
Match proportions, paper, brass hardware, icon tabs, portrait size and text rhythm
to the People image. Verify it in-engine before propagating the design.

Keep the existing All/Known filters, person count, independent directory/detail
scrolling and durable selection. Unknown remains a developer-only filter. Group
existing facts into attributes, daily life, possessions, routine and affiliation/
knowledge. Hero and villager fields remain conditional on what applies to them.

Personal possessions policy for implementation: exact personal money, inventory and
carried contents are visible for the local hero and units the player commands, not
everyone sharing a banner. Use one permission predicate and clear private cached
values when command is lost. Show unavailable information honestly without dummy
values. Audit both roster/inspection responses and replicated components: hiding a
label alone must not be presented as server-enforced privacy. If the wire contract
must change, implement the server/shared boundary and protocol version together.
Public town stock and company records keep their separate existing access rules.

## 5. Port the other four pages

Preserve existing domain models/actions and replace their presentation in bounded
steps. Do not introduce new gameplay simply because a decorative icon suggests it.

| Page | Layout and functionality to retain |
|---|---|
| Places | Expandable settlement/building tree; village and building pictures; six summary figures; grouped people/stability/economy/growth records; contextual market, history and worksite actions |
| Retinue | Simple full-width portrait roster, hero first; current activity/home/occupation and Locate; correct unavailable/empty states |
| Army | Pennant battalion directory, role portrait, capacity, map selection/Locate, Defensive/Hold line, equipment/fire policy, two troop lists, individual and bulk add/remove/transfer, reserves and disband confirmation |
| Companies | Ownership filters, portfolio facts, firm emblem, financial summary/ledger, player position, operating-site pictures, routes and existing ownership/management actions |

Keep controls accessible when detail grows. At narrow sizes allow scrolling or
deliberate reflow, particularly for Army and Companies, rather than making text tiny.
Retain selection, drafts and scroll positions during live data refreshes and when
portrait jobs finish. Refresh scalar values in place; do not rebuild whole lists
because a health value, clock, image handle or company balance changes.

## 6. Complete all nested destinations

Reskin the existing `EncyclopediaPageHost` and Back bar as part of this work. Market,
history, company controls, founding and route editing must feel like pages in the
same book. Keep their real actions, disabled reasons, form state and return target.
Escape returns one level; X closes the entire book. Keep one modal backdrop and
central input ownership, including no world orders leaking through the panel.

## 7. Verification and acceptance

Use checked-in RON scenarios through the real Bevy capture harness. Wait for actual
artwork/portrait readiness and inspect every affected PNG with its `.capture.json`.
Keep generated review images and recordings under ignored logs. Generated concept
art is a visual design reference, not a deterministic pixel-regression baseline.

Visual acceptance:

- All five reference layouts have an inspected counterpart at 1600×1000 and 1280×720.
- Compare side-by-side images and enlarged crops of paper, button edges, patina,
  screws, portraits and text. Correct noticeable differences before calling a page done.
- Match the reference's material quality and hierarchy; actual characters, buildings,
  names and game values remain truthful. Record any intentional responsive differences.
- Test hover, pressed, selected, disabled and keyboard-focus states; paper remains
  quiet, text legible, hardware unstretched and no transparent slicing seams.
- Include empty lists, long names/amounts, many towns, large retinues/battalion lists,
  changed outfits, unknown appearance, upgraded buildings and summary-only places.
- Test repeated opening, tab switching, scroll restoration, nested return paths,
  rapid portrait selection and interaction during live updates.
- Capture a continuous scroll/navigation sequence to expose flicker and frame spikes;
  compare cold/warm page opening and idle UI work, without claiming unrelated FPS gains.

Behavior and code acceptance:

- `cargo check --workspace --all-targets`, full workspace tests and workspace build.
- Relevant authority/privacy, image-cache, retained-row and navigation regressions.
- Use the connected client/server lab for troop transfers/stances, company/market
  actions, ownership changes and privacy. Offline UI fixtures do not prove those actions.
- Preserve ordinary-player and developer visibility rules. Distinguish hidden
  presentation from permission-controlled data.
- Document the shared skin, thumbnail ownership, asset regeneration and capture recipes.

## 8. Work order and checkpoints

Implement in an isolated `codex/` worktree based on the committed HUD work, preserving
the other agent's unfinished building-detail changes. Avoid broad staging or resets.

1. Reference measurements, shared art kit and font comparison.
2. Finished shell + People page + shared portrait service + possessions policy.
3. Retinue, then Places with building/village imagery.
4. Army, then Companies and all nested pages.
5. Small-screen polish, connected behavior checks and measured performance review.

Use small commits at these boundaries, each with appropriate checks and captures.
The first major checkpoint is an actual People screenshot with the reference's paper,
aged hardware, readable type and portrait atmosphere. This establishes the quality bar
before multiplying the same presentation across the remaining screens.
