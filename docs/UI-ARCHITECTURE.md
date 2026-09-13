# Client UI architecture

Last reconciled with Bevy 0.19 on 2026-09-13. This is the contract for new UI and for
touching an existing screen. The goal is a coherent medieval ledger interface without
screen-specific hover logic, accidental world input, or full-tree churn at simulation speed.

The [encyclopedia verification record](ENCYCLOPEDIA-VERIFICATION.md) documents the
inspected layouts, connected behavior, compressed art budget and portrait timings.

## Ownership

| Module | Owns |
|---|---|
| `client/src/ui/styles.rs` | Wood/brass/parchment palette, ink, rules and shadows |
| `client/src/ui/foundation.rs` | Semantic layers, type scale, standard button states, disabled/focus behavior, contract audit and live-panel refresh safety |
| `client/src/ui/typography.rs` | Bundled Cinzel headings, MedievalSharp HUD text and Libre Baskerville reading text; shared font handles |
| `client/src/ui/motion.rs` | Analytic springs and retained panel/page reveals |
| `client/src/ui/button_motion.rs` | Shared button hover/press motion and paint easing |
| `client/src/ui/frame.rs` | Non-interactive brass corner ornaments |
| `client/src/ui/encyclopedia/shell.rs` | Bound-book frame, header, tabs and footer; separate from page layouts |
| `client/src/ui/modal.rs` | One backdrop, canonical modal root/backdrop/panel structure, click-through protection and central modal-open state |
| `client/src/ui/scroll.rs` | Wheel bubbling and nested scroll behavior |
| Screen modules | Data model, layout and actions specific to that screen |
| `client/src/app_wiring/window.rs` | Resolution-aware UI scale; 3D render scale never makes UI blurry |
| `client/src/ui/pause_menu/layout.rs`, `skin.rs` and `widgets/` | Retained Escape/settings frame, shared launcher/ledger artwork and grouped native controls |
| `client/src/ui/pause_menu/input.rs` | Visible-page keyboard focus, hidden-control exclusion and graphics-key activation |
| `client/src/ui/pause_menu/backdrop.rs` | Bounded live-world filter using the existing scene render target; resize/release ownership |
| `client/src/ui/pause_menu/display.rs` | Explicit display-mode choices, supported output sizes, actual scene-pixel labels and retained confirmation/resize state |
| `client/src/ui/hud/shell.rs` | Place/purse, owned-hero/Home selection, map/encyclopedia navigation and compass |
| `client/src/ui/hud/selection_card.rs` | Selected-person card and full-record expansion |
| `client/src/ui/hud/portrait.rs` | HUD selection consumer of the shared portrait service |
| `client/src/ui/portraits/` | Actual observed outfits, shared canonical geometry, one raster worker and a 32 MiB LRU texture cache |
| `client/src/ui/ledger/` | Shared paper/wood/hardware, framed portraits, building illustrations and textured buttons |
| `client/src/ui/startup/` | Shared FistWorld background, artwork handles, framed controls, responsive startup scale and three-diamond loading motion |
| `client/src/ui/main_menu/` | Server-address draft, presets, launcher layout and Connect/Exit actions |
| `client/src/ui/name_entry/` | Account-name editing, submission lock, authoritative rejection and asynchronous world preparation |
| `client/src/render/systems/connection.rs` | Asynchronous DNS, connection deadline, cancellation and disconnect recovery |
| `client/src/ui/hero_creator/layout.rs` | Retained wardrobe book, manifest-indexed selectors and inline submission status |
| `client/src/ui/hero_creator/artwork.rs` | Creator material handles, readiness and slicing recipes |
| `client/src/ui/hero_creator/actions.rs` | Guarded mouse/keyboard actions, changed-only value bindings and authoritative creation intent |
| `client/src/ui/hero_creator/preview.rs` | Main-view idle character diorama, framing from the actual preview pane and fixed facing |
| `client/src/ui/hud/chrome.rs` | Small authored wood/brass frames and icon handles, with native text and input |
| `client/src/ui/hud/journey.rs` | Shared exploration/combat bell and bounded recent notice drawer |
| `client/src/battalion_bar.rs` and `battalion_bar/navigation.rs` | Retained battalion cards, bounded paging and selection reveal |
| `client/src/combat_mode.rs` | Compact combat status and optional Orders help |
| `client/src/siege/controls.rs` | Selected siege controls and placement above mixed-army cards |
| `client/src/ui/market/model.rs` | Pure market presentation: eligible purchase quote, cargo/listed ownership and disabled reasons |

Do not put a new palette alias, hover state machine, modal scrim or scroll algorithm in a
screen module. Screen-specific data colours (for example chart series or a health grade) are
allowed; reusable chrome colours belong in `styles.rs`.

The persistent HUD leaves the centre of the world clear. Its top-left crest returns to the
owned hero (also Home); the place label follows the viewed area and centres the nearest
town when clicked, preserving the selected hero or army for the next order. The purse always belongs to the local hero, even when another person
is selected. Top-right contains the replicated clock and notice bell. Bottom-left shows
the selected person or group; bottom-right opens the map and encyclopedia. Modal screens
and the opening cinematic hide the HUD and its input rectangles.

`selection_card.rs` displays actual health and one expansion control for the selected
person's durable encyclopedia record. Detailed character and inventory information stays
inside that record; the persistent card has no trade or inventory action row. Group selection uses a crest and selection count, never an arbitrary person's
portrait. `ui/portraits/` rasterizes canonical GLB geometry, outfit, skin and textures with
one shared background worker. It remembers observed appearances by durable PersonId,
including outfits arriving after identity; a never-observed person gets a neutral crest.
The demand set contains only visible, unclipped widgets. Larger selected portraits are
prioritized, identical outfits share images, and a 32 MiB LRU pins visible outputs while
evicting offscreen entries. Source mesh/material data is shared across jobs. Disconnect
clears the memory and rejects an old session's pending output. This adds no PBR camera;
`PortraitStatus`, `PortraitMetrics` and the HUD's `PortraitReadiness` expose semantic gates.

`journey/view.rs` owns the optional drawer; `journey/notices.rs` owns three-message history,
repeat coalescing and unread state. A sequence on `GodNotice` distinguishes a new result
from its countdown. New messages never force expansion and become read only after the
expanded drawer has visible layout. The same bell remains available in combat without a
local hero. The combat dock reserves 438 design pixels on the left and 180 on the right;
its clipped card viewport and previous/next controls accommodate large armies. Its cards
are 96 design pixels tall, 18 pixels above the bottom edge. Selection changes reveal an
offscreen selected card without undoing deliberate paging. A compact combat badge expands
Orders help only on request.

Siege controls share the dock's left edge. Catapult-only selection hides the battalion dock
and places siege controls 18 design pixels above the bottom edge. Mixed soldiers and siege
retain the dock and raise the controls to 130 pixels, leaving a gap above the cards. Modal
input blocking immediately hides the siege panel and rejects its actions, independent of
the panel's slower data-refresh interval. Group command counts include owned siege units.

The frame artwork is generated by `asset_creation/ui/build_hud.py`. Keep the small PNGs
and generator in Git, never bake labels, values or whole-screen mockups into a HUD asset.
Panels use nine-slicing; native text uses the bundled medieval fonts. Artwork uses shared
button state and motion rather than introducing another hover animation system.

The exploration compass has its own recessed charcoal/brass case and transparent
bearing artwork, leaving shared crest/portrait medallions unchanged. The case stays
fixed while bearings follow commander yaw; the native north label follows that bearing
but counter-rotates to stay readable. Its 112px control and separate 48px encyclopedia
button sit inside a 32px edge inset, with a 6px gap at UI scale 1. Both palette-optimized
224px PNGs together use about 11 KB on disk and share the existing artwork handles,
readiness and button lighting. No textures are regenerated as the camera turns.

The market layout and actions remain in `market.rs`; its read-only model lives in
`market/model.rs`. Quote another seller's eligible offer, not the exchange's headline ask
(which may be the player's own offer). The displayed BUY price also becomes the order's
server-enforced price ceiling. Ordered trade replies are associated with the originating
market so changing pages cannot show another town's feedback.

## Visual contract

The startup views use one shared village image and a bounded five-tap UI material
for the modal backdrop. The launcher keeps the illustration clear, with a local
shade behind its controls. Name entry and connection/world preparation reuse the
same artwork handles and worn frames. Text, fields, buttons and loading diamonds
remain native Bevy UI; generated full-screen concepts are review output, never
interactive screen textures. The background node and material are removed on
entering Playing. Small shared art handles remain cached for returning to the menu.
See [STARTUP-ART.md](../asset_creation/ui/STARTUP-ART.md) for source provenance,
compression and the delivery budget.

`NameEntryPhase` separates Editing, Submitting and Preparing. A reliable submission
locks the account name immediately, before deferred work can permit a second click.
Busy text uses neutral ink, three sequentially pulsing diamonds and a rotating
compass ring. It never implies a measured completion percentage. Validation errors
return to the editable form; Back/Cancel drops pending installation and disconnects.
The server remains authoritative for reserved names, account identity and the map
recipe. DNS runs on the I/O pool and the connection attempt has a real-time deadline,
so an invalid address cannot freeze the menu or leave it indefinitely blank.

Server and name fields consume logical keyboard events, preserve UTF-8 boundaries,
and support selection, deletion and clipboard shortcuts. The server field keeps an
incomplete draft separate from the parsed hostname/IP and port. Name guidance follows
the existing server format: letters, numbers, underscore or hyphen, with a 3–16-byte
wire limit. Wide names fit inside the field using the existing font atlas. Shared
button focus and spring feedback apply to startup controls too. The startup modal
tab group removes hidden retained controls from navigation and disables their
actions; focus returns to a visible field or Cancel after a state change. Presets
use their configured port, or the server default, independently of a custom draft.
The name form has continuous wood behind its overlapping torn parchment, so a
validation message can grow the page without opening a gap onto the backdrop.
Startup sizing follows the physical window; Playing restores the graphics-settings
owner. Offline scenarios check presentation and motion; the connected startup lab
checks actual submission, rejection, creation and reconnect behavior.

- In-world panels use warm parchment and dark ink. Wood headers, brass binding and
  parchment lettering connect the encyclopedia and menus to the combat UI. Dark panels
  use the inverse palette; page bodies remain light for dense records.
- `EMBER` means selected/commanded. Debug tools use `SLATE`; destructive actions use the
  danger color. Do not create another saturated accent in a screen module.
- Use `typography::body` / `typography::heading` with `foundation::type_scale`: caption 12,
  body 14, value 15, heading 18, title 26 on the 1600×900 design canvas. `typography::text`
  applies the existing screens’ 17 px heading boundary. `ledger::reading` uses Libre
  Baskerville at weight 450 for dense book records; `reading_strong` / `body_strong`
  use the same variable font at 700 for names, controls and key values. Headings retain Cinzel. Fonts are bundled under OFL
  and installed once; do not load another font per screen or depend on system fonts.
- Book frames use a 3 px brass-brown edge, a projecting bevel and aged corner hardware
  aligned to the outside binding. Decorative borders never intercept input or cover text.
  Compact plates use a one-pixel rule, 2–3 px radius and `plate_shadow()`. Nested content uses
  soft dividers more often than boxes inside boxes.
- Authored HUD frames use nine-sliced `ImageNode`s. They replace chrome,
  never become baked text or screen-sized bitmaps.

The hero creator reuses ledger typography and button behavior, with four dedicated
worn material sprites in `ui/creator/`. `LedgerButtonFace` supplies explicit image
recipes without losing shared hover, focus, disabled or spring feedback. Its larger
46/54 composition leaves breathing room around the idle figure; parchment edges
carry wear while the reading centre stays quiet. Arrows and button diamonds use
native geometry so they cannot turn into missing-font boxes. See
[CREATOR-ART.md](../asset_creation/ui/CREATOR-ART.md) for the compressed asset budget.
Keep it free of decorative slogans: use functional labels and reserve footer status for
useful connection feedback. The visual hierarchy comes from the frame and spacing.
Its layout stays mounted while selections change: only the affected wardrobe/skin label binds again,
along with labels added by a new tree or changed manifest. The local preview follows
the chosen outfit without changing any replicated character. Tab uses the foundation's
modal focus group; Enter, Numpad Enter and Space activate the focused enabled control.
Mouse actions wait for a completed in-modal press/release, including the first macOS
click whose press arrives before a cursor position. The opening click cannot select or
confirm an option. A connection-pending creation request leaves the modal open and
shows its retry message inside the footer, since the ordinary HUD is hidden there.

Character creation is a startup-only flow for an account that needs its first hero.
BEGIN JOURNEY sends the existing reliable `CreateHero` intent and the server owns
arrival. The God panel has no Create Hero control, and the creator has no developer
Place/Cancel mode. Ordinary Escape and backdrop clicks retain the mandatory screen.
The capability-checked developer skip remains available for inspecting a world without
starting a voyage; it cannot reopen the creator. `FISTWORLD_AUTOSPAWN_HERO=1` and the
server-gated `DevCommand::SpawnHero` remain explicit development/lab hooks, not interactive
creation controls. This screen's transparent UI backdrop is an intentional modal
exception: the live idle character and
its backdrop are rendered through the main 3D view, rather than a second PBR camera.
The preview holds a readable fixed facing and derives framing from its laid-out pane.
Its studio lights switch off whenever the pane is closed or unready. Do not put a
filled panel shadow behind the transparent cutout: Bevy's box-shadow quad covers
its interior and darkens the character. The 3D surround supplies the backdrop.
The maintained startup tour in [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md) exercises all
wardrobe selectors, skin, idle framing, Escape/backdrop retention and disconnected
submission. The former God creator tour is retired. The startup-only cleanup was
verified at normal and small sizes, together with the God HUD's remaining controls;
current and historical evidence are recorded separately in the capture guide.
Connected creation is checked separately.

## Escape and settings

The compact GAME MENU opens into a single settings frame with dark navigation and
a light parchment page. `pause_menu` reuses compressed `StartupArtwork` and ledger
paper, wood, button faces and corner hardware; generated full-screen concepts are
review references, not runtime menu textures. Text, arrows, keycaps and slider
interaction remain native UI. Its backdrop samples the existing pre-UI scene target
through the shared bounded startup filter, refreshes its binding after target
replacement/resizing, and releases the material when the menu closes. It adds no
second world camera.

Standalone navigation and the Graphics, Audio and Controls pages remain mounted
while the menu is open. Page changes toggle display and selected navigation state;
shared `UiReveal` springs own motion without rebuilding controls. Hidden retained
controls leave keyboard navigation and cannot accept actions. Focus must stay on a
visible, enabled control; disabled step endpoints retain their setting-specific
meaning independently of page visibility.

The stationary parchment owns its safe edge gutter; inner page viewports clip
and scroll inside it, including when display confirmation expands. Authored
menu buttons opt into `UiArtworkFocus`: keyboard focus lights their existing
face instead of adding a rectangular outline across their irregular edges.
The settings header shares the body's navigation-column width, so the wordmark
and SETTINGS heading remain centered over their own panels. Page headings,
section headings and field labels use a distinct size hierarchy; display values
share the display-mode controls' center rather than the page's far-right edge.

Escape returns from settings to the standalone menu; a second Escape resumes.
Back also returns to the standalone menu, while Resume or the frame's X dismisses
the whole menu. A transparent shared outside-click target also dismisses it; the
filtered scene material supplies the only visual scrim. A held click during menu
creation or a drag beginning inside the panel must not trigger outside dismissal.
Another modal, including mandatory character creation, retains its
existing input guard. GAME MENU does not pause the authoritative world.

Graphics separates Display, Quality & Distance and Lighting, with explicit ON/OFF
choices and the existing Keep/Revert display safety flow. Audio retains immediate
Master/Music/Effects levels, independent Music/Effects switches and keyboard/drag
input. Controls combines mouse sensitivity with a grouped shortcut reference; its
ornamental keycaps are not rebinding controls. The requested Find your hero and
Close menu shortcut rows are omitted without removing the underlying shortcuts.

The maintained [pause-menu tour](../capture/scenarios/pause-menu-tour.ron) checks
production navigation and local setting changes at normal and small resolutions.
See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md#escape-and-settings-menu-tour) for readiness,
artifact evidence and the separate native-display/audio acceptance boundaries.

## Encyclopedia materials and portraits

The five spreads and nested pages use the same `ledger` catalogue. `paper()`,
`directory_paper()` and `wood()` deliberately contain no Node: pages own layout,
input and scrolling. The directory surface tints the same shared paper texture
slightly darker; `directory_gutter()` adds a non-interactive paper lip and fading
edge shadow without changing column or scrollbar geometry. The detail page stays
lighter for reading. Framed
portraits place a `PersonPortrait` on their inner image, keeping its mutable identity
separate from the retained frame. Illustration components bind shared `UiMaterial`
handles by artwork and finish. Printed vignettes contain the whole illustration
with transparent, irregular paper fades; medallions fill their circular frame.
Fixed shader grain stays still, and the source image is never copied per widget.

`ledger::LedgerButtonScope` opts a retained panel outside the encyclopedia into the
shared worn button faces and ledger scrollbars. It changes presentation scope, not
input ownership or modal behavior. `ledger::selector_face()` supplies the reusable
paper inset behind selector values; screen layouts own its dimensions and live text.
`LedgerIllustration::settlement` maps Hamlet, Village, Town and City to separate
paintings in the directory, Overview and page header. Settlement upgrades update
those retained instances without replacing observed building appearances. Ruins
currently use the Hamlet plate until dedicated ruin art is authored.
Use `LedgerIllustration::building` and observed Hall/House appearance for canonical
thumbnails; do not infer a completed upgrade solely from a town's tier. The shared village
vignette is illustrative scenery, not a literal world-map view.

`ledger::heading` and `ledger::body` already contain TextColor and Pickable. Override a
component with a subsequent `.insert(...)`, never a duplicate entry in the same bundle.
Generated material images are sized and compressed for delivery; maintained generators,
recipes, budgets and licenses are documented in `asset_creation/ui/LEDGER-ART.md`.

Major section rules use a small native diamond ornament. Outer triangular brass
caps cover the panel's outside border; inner viewports own clipping. Ledger button
faces retain native rectangular targets, focus and spring feedback while sharing
worn paper/dark-leather/amber artwork. The shared `LedgerButton` skin requires
`UiArtworkFocus`, including startup, character creation and custom face recipes:
keyboard focus brightens the authored face without outlining its rectangular hit
target. Skin binding precedes focus styling; newly styled retained controls also
reconcile their focus without requiring another Tab press. Selected-row borders
are separate, non-picking
children: `Outline` remains owned exclusively by keyboard focus. Troop checkbox
decoration follows the existing selection action instead of embedding brackets in
the person's name.

The close control uses its own small square face from the same button generator:
scaling a wide nine-slice into a square otherwise shrinks the brass rim below a pixel.
`UiButtonLabelTint` supplies enabled engraved lettering colour while foundation
continues to own spring motion, focus and disabled contrast.

`ledger/scrollbars.rs` supplies bronze scroll indicators beside actual book
viewports. They are retained siblings positioned from resolved bounds and
`ScrollPosition`, so they cannot scroll with the content or increase its extent.
They disappear when the content fits; wheel/touchpad ownership remains in
`ui::scroll`. These indicators do not introduce a separate drag-input system.

Personal money and inventory are visible only for the own hero and currently commanded
people. The book clears these cached fields when access is lost. The server applies a
recipient filter to Wallet/GoodsInventory before sending, denying unclassified actors
from initial spawn. Visible cargo communicates its good/prop but no exact private amount.
Public building and market storage retain their existing regional visibility.

## Buttons and accessibility

Every new `Button` gets `button_chrome(UiButtonVariant::...)`. Its primary text child gets
`UiButtonLabel`, which lets the foundation preserve readable contrast for selected, disabled
and inverse states without recolouring secondary metadata in a complex row. Action systems
query their required marker and read `Interaction`; they do **not** write `BackgroundColor`.

```rust
parent.spawn((
    MyAction,
    Button,
    Node { /* screen-owned dimensions */ ..default() },
    button_chrome(UiButtonVariant::Secondary),
)).with_children(|button| {
    button.spawn((Text::new("ACTION"), UiButtonLabel, TextColor(INK)));
});
```

The available variants are `Primary`, `Secondary`, `Ghost`, `Row`, `Tab`, `Inverse`,
`Ribbon` (dark header navigation), `Developer` and `Danger`. A development-time audit fails
immediately if an ordinary Bevy `Button` lacks `UiButtonStyle`. `UiButtonStyleExempt` is reserved for the full-screen
outside-click target created by `modal_backdrop_chrome`; it is not an escape hatch for custom
button styling. Authored book button faces carry `UiTexturedButton`: the foundation
keeps transparent fill/edges while the shared spring and label contrast still apply.
The artwork binding supplies a loaded face immediately and tints it from the same
hover/press/focus state. A textured button is never exempt from input or disabled rules.

Unavailable controls remain visible and add Bevy's `InteractionDisabled`; do not leave an
active button in the tree and merely ignore its click. The foundation removes disabled
controls from tab order. Standard controls receive tab navigation, visible keyboard focus
feedback and Bevy's automatic button accessibility role/label. Plain controls use a
focus ring; authored faces use their material tint. Text fields retain their caret.

An interaction query containing optional action markers must also have a required filter:

```rust
Query<(... Option<&A>, Option<&B>),
      (Changed<Interaction>, Or<(With<A>, With<B>)>)>
```

Without that filter it matches every interactive colored entity, including a full-screen
backdrop. This exact bug once recolored the world white from the property screen.

## Modals and navigation

Use `spawn_modal`. Custom layouts use `modal_root_chrome` and
`modal_backdrop_chrome`; do not reconstruct those bundles by hand. The standard helper creates:

1. a transparent, non-pickable root at `foundation::layer::MODAL`;
2. exactly one warm 46% backdrop that owns outside-click dismissal;
3. a blocking panel above it; and
4. a modal `TabGroup`, so keyboard focus cannot escape behind the window.

The default panel is the parchment ledger shell. A deliberately dark front-of-house screen must
override it explicitly; developer and debug screens do not receive a separate prototype theme.

Never color both the root and backdrop: their alpha compounds. `InputState::modal_open` is
derived from `ModalRoot`; do not borrow an unrelated flag such as inventory state. The hero
creator uses a transparent backdrop because its 3D diorama is intentionally seen through
the panel. The Game Menu also uses a transparent input backdrop, with its sole visual
scrim supplied by the filtered scene material. Both retain the shared modal root,
outside target, layer and tab group.

Use `X` to dismiss a top-level window and `BACK TO …` when returning to an owning record.
Escape and clicking the backdrop dismiss the same top-level target. A Back action must restore
the prior selection instead of opening an unrelated copy of the screen. The Game Menu's
owned settings pages explicitly use Escape as Back; from its standalone menu, Escape
dismisses it.

Button state is computed after screen state in `PostUpdate`, so plugin registration order cannot
introduce a one-frame selected/hover mismatch. Layer assignments are centralized: presentation
−1000, HUD 0, floating panel 100, modal 1000, tooltip 1100 and toast 1200. Screen-local child
ordering may use local `ZIndex`.

## Motion

`UiReveal::panel()` gives a stable shell a 24 px arrival; `UiReveal::page()` gives retained
pages an 8 px arrival when their own `Node.display` becomes visible. `TabBody` requires the
page reveal. Do not attach it to a repeatedly rebuilt value/row, a whole input backdrop,
or an entity whose `UiTransform` is already owned by another animation. Close remains
immediate so invisible modal hit areas never linger. Native transforms do not reflow rows.

`UiButtonStyle` requires shared button motion. Hover/press moves the primary label by a small
spring translation; the button hit rectangle stays fixed, preventing edge-hover flicker.
Rows and flat tabs stay still. Selection/disabled/variant changes snap their palette to keep
labels readable, while hover paint eases. The analytic underdamped solver is shared with the
combat banner and battalion bar, stays stable across long frames, and stops transform writes
when settled. Do not copy Euler spring integration into a screen.

## Live simulation panels

Economy values can change every server tick, especially at 10×–100×. Do not rebuild a panel
at that cadence while the user is interacting with it.

Compatibility panels which still rebuild structurally (notably history ledgers and the permit
tray) follow the shared rule:

- compare a screen signature and do nothing when it is unchanged;
- cap structural refresh to about eight per real second with `UiRefreshStamp`;
- defer a structural refresh while a real control in that subtree is hovered or pressed;
- mark the full-screen backdrop `UiRefreshExempt`, so parking the pointer outside the panel
  does not freeze its values; and
- preserve scroll position when rebuilding the same record.

Prefer stable entities plus diff-gated text/state updates for a new complex screen. A bounded
rebuild is the compatibility path for the existing data-heavy ledgers, not permission to
despawn a button under the cursor.

## Build once, bind in place

Bevy UI is retained: the rule for every live panel is **rebuild only on a structural change,
bind values in place for everything else.** Structure is what exists — which settlement, which
tab, which cards, whether an acting company exists. A value is a price, a count, a label, an
enabled state. The property board (`property_market.rs`) is the reference implementation:

- One system, `sync_property_panel`, computes the page model every frame. A short *structure
  key* (no `Debug` dumps of whole structs) decides whether to respawn the tree; otherwise the
  system writes values into marked widgets (`PermitPriceText`, `PermitActionLabel`,
  `ListingFactText`, `ActingCompanyText`, `PropertyTabCount`) and flips button state
  (`UiButtonStyle.variant`, `InteractionDisabled`, the `PurchasePermitButton` order) in place.
- The values come from one pure function (`permit_card_values`) used both at spawn and at bind,
  so the two can never disagree, and it is what the unit tests exercise.
- Because the widgets under the pointer survive, there is no hover-deferred refresh, no
  `UiRefreshStamp`, and no scroll-position retention hack on this panel. The founding form
  follows the same pattern for its steppers.

Three panels show the *model-driven* variant, which is the shape to copy for anything with many
rows or optional sections:

- Company controls (`business_management.rs`): the replicated policies fold into a pure
  `ControlsModel` — sections, rows, controls and meters, each with a stable id (`wage`,
  `cover.Wheat.3`, `input.Wheat`). `structure_key()` is the id sequence; only a change in it
  respawns the tree. `bind_panel` then writes every value by id into `BoundText` nodes,
  `BoundButton`s (label, `UiButtonStyle.selected`, and the `Action` order the button will send)
  and `MeterFill` lanes. Selected choices are filled buttons, not a "SELECTED:" prefix.
  The unit tests pin the contract: a wage or position change keeps the key; ids are unique.
- The compact settlement card (`settlement_panel.rs`): `CompactModel` is a title, a handful of
  key tiles, a few vital rows and the action buttons; `CompactBound` slots (`Tile(i)`, `Row(i)`)
  bind values, and the card only respawns when the selection or the tile/row *labels* change.
  Detail the card dropped (labour market, policy lines, arrears) lives on the place page behind
  EXPAND.
- The settlement market (`market.rs`): one stable row exists for every `Good`. Stock, last sale,
  best offer, today's demand, hero cargo, prices and disabled reasons bind in place. Its only
  structure key is the settlement entity, so even a 100x economy tick never replaces the BUY,
  POST or HISTORY controls under the pointer.

Why: the signature-rebuild pattern (format the whole input into a string, despawn and respawn
on any difference, defer while hovered) produced every UI-feel bug we hit — a +1 press that
showed two seconds later, rows that could not be clicked, scroll positions jumping. Rebuilding on
a float drift also costs real frame time at scale. Panels that still rebuild on change
(history ledgers) do so because their inputs genuinely change only on an event.

Measure it: `FISTFORCE_CLIENT_PERF=1` logs `ClientPerfUi <system>=calls/rebuilds/ms`; a bound
panel shows `1r` per open, never `Nr` while you hover or while the world ticks.

## Type scale and ledgers

The UI scale is derived from the 1600x900 launcher frame, so small captions land at roughly
6 pt on a Retina laptop. Panels declare a type scale at the top of their module (title 22,
heading 17, value 15, button 14, body 13.5, label 11-12) and nothing a player must read to make a
decision sits below body size. Place pages lead with key-figure tiles, then grouped sections
(`group_rows` in `encyclopedia/places.rs`); history charts draw real lines
(`spawn_chart_segment` rotates a thin node with `UiTransform`). Use ` / ` between compact facts and ASCII `<`, `>`, `v`, `|` for steppers, dropdowns and
carets. The bundled faces do not contain arrow/triangle/box-drawing glyphs; do not rely on
system font fallback for controls.

## The encyclopedia is one window with pages

Markets (`market.rs`), ledgers (`history.rs`) and company controls (`business_management.rs`) are not modals: they
render inside `EncyclopediaPageHost`, full size, under one BACK bar that names its destination
("BACK TO ALDRIC GRAIN & BREAD"). Their target resources (`HistoryPanelTarget`,
`BusinessManagementTarget`) are the page state; setting one from anywhere — a company record,
the compact settlement card, the market page — opens the encyclopedia on the matching tab and
hosts the page next frame. A market history page covers its owning market without clearing it,
so BACK reveals the same retained market page and scroll state. ESC pops a page before it closes the window; the X closes everything
and `close_pages_with_encyclopedia` clears the page targets so nothing reopens itself. New
pages follow the same shape: spawn into the host, bind in place, no own close.
The company-founding form (`company_founding.rs`) is the model for an input form: the name and
capital are a client-side draft, `sync_founding_texts` writes them into the form in place so a
press shows instantly even while the pointer rests on the button (refresh-gated panels defer
their rebuild until the pointer leaves), and only FOUND talks to the server. It is both an
encyclopedia page (COMPANIES › NEW COMPANY; `FISTFORCE_CAPTURE_ENCYCLOPEDIA=founding`) and the
inline ACTING AS strip on the permit board when you have no company. Capture a populated form
with `FISTFORCE_CAPTURE_HERO=default FISTFORCE_CAPTURE_HERO_OFFSET=0,0 FISTFORCE_CAPTURE_SELECT=1`.

## Responsive layout and scrolling

- Large panels use viewport-relative width/height plus pixel maxima. The global UI scale keeps
  the 1600×900 design canvas usable at lower output resolutions.
- Every bounded content column has `min_height: 0`, `Overflow::scroll_y()` and a visible
  scrollbar width. Nested scroll views rely on `UiScrollPlugin` bubbling; do not consume wheel
  events in a screen-specific system.
- Output resolution, display mode and 3D render scale are separate. The world may render below
  native resolution, but UI renders at the window resolution. The graphics panel exposes
  Windowed, Borderless and Exclusive as direct choices. Borderless disables output-size
  stepping with visible guidance; the separate 3D Resolution control always works and
  reports scene pixels plus scale (25–100%). Its labels and disabled/selected states bind
  from current settings and window size, including after a timed revert. Pixel dimensions
  use the same calculation as the production scene target.

## Regression checks

The client unit suite covers query scoping, one-scrim modals, the button-contract audit,
disabled/selected contrast, hover-safe refresh and nested scrolling:

```bash
cargo test --workspace
```

The deterministic visual harness can render the real property board without opening the game
manually. See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md) for checked-in RON scenarios,
semantic assertions, JSON sidecars, offscreen scene capture, recording and visual baselines:

```bash
FISTFORCE_CAPTURE_SETTLEMENT=village \
FISTFORCE_CAPTURE_PROPERTY=1 \
FISTFORCE_CAPTURE_HUD=god \
BEVY_ASSET_ROOT="$PWD/client/assets" \
cargo run --profile playtest -p client --bin capture -- \
  --at 0,0 --name property-ui --zoom 160 \
  --out /tmp/fistworld-ui --warmup 90 --settle 30
```

The equivalent market page fixture is:

```bash
FISTFORCE_CAPTURE_SETTLEMENT=village \
FISTFORCE_CAPTURE_TRADE=1 \
FISTFORCE_CAPTURE_ENCYCLOPEDIA=places \
FISTFORCE_CAPTURE_HERO=default \
FISTFORCE_CAPTURE_SELECT=1 \
BEVY_ASSET_ROOT="$PWD/client/assets" \
cargo run --profile playtest -p client --bin capture -- \
  --at 0,0 --name market-ui --zoom 160 \
  --out /tmp/fistworld-market-ui --warmup 90 --settle 30
```

Inspect the result at `/tmp/fistworld-ui/property-ui.png`. `FISTFORCE_CAPTURE_PROPERTY=sale` opens the
board on its FOR SALE tab instead of PERMITS. The world must remain visible under
one dark scrim; no surface may flash to `BUTTON_NORMAL` merely because the pointer entered it.

The compact card and the controls page have fixtures too. Selection fixtures live in the hero
spawner, so they need a stand-in hero: `FISTFORCE_CAPTURE_HERO=default
FISTFORCE_CAPTURE_HERO_OFFSET=0,0 FISTFORCE_CAPTURE_SETTLEMENT=village
FISTFORCE_CAPTURE_SELECT=hall|building` photographs the card on a hall or on the first operating
business. `FISTFORCE_CAPTURE_ENCYCLOPEDIA=business` (same hero variables) opens the site-controls
page for the staged windmill; add `FISTFORCE_CAPTURE_BUSINESS_SCROLL=900` (pixels) for its lower
sections. With `FISTFORCE_CLIENT_PERF=1 FISTFORCE_CLIENT_PERF_INTERVAL_SECS=1` the log should show
`sync_compact_panel=…/1r` and `ensure_business_panel=…/2r` once, then `0r` every second.

The real J menu and its locked-access state are deterministic visual targets too:

```bash
BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTFORCE_CAPTURE_DEBUG_MENU=god \
cargo run --profile playtest -p client --bin capture -- \
  --at 0,0 --name j-menu-god --zoom 220 \
  --out /tmp/fistworld-ui --warmup 120 --settle 40

BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTFORCE_CAPTURE_DEBUG_MENU=access \
cargo run --profile playtest -p client --bin capture -- \
  --at 0,0 --name j-menu-access --zoom 220 \
  --out /tmp/fistworld-ui --warmup 120 --settle 40
```
