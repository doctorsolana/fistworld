# Client UI architecture

Last reconciled with Bevy 0.19 on 2026-08-19. This is the contract for new UI and for
touching an existing screen. The goal is a coherent medieval ledger interface without
screen-specific hover logic, accidental world input, or full-tree churn at simulation speed.

## Ownership

| Module | Owns |
|---|---|
| `client/src/ui/styles.rs` | Limewash/ink palette, rules, shadows and low-level visual constants |
| `client/src/ui/foundation.rs` | Semantic layers, type scale, standard button states, disabled/focus behavior, contract audit and live-panel refresh safety |
| `client/src/ui/modal.rs` | One backdrop, canonical modal root/backdrop/panel structure, click-through protection and central modal-open state |
| `client/src/ui/scroll.rs` | Wheel bubbling and nested scroll behavior |
| Screen modules | Data model, layout and actions specific to that screen |
| `client/src/app_wiring/window.rs` | Resolution-aware UI scale; 3D render scale never makes UI blurry |

Do not put a new palette alias, hover state machine, modal scrim or scroll algorithm in a
screen module. Screen-specific data colours (for example chart series or a health grade) are
allowed; reusable chrome colours belong in `styles.rs`.

## Visual contract

- In-world panels use limewash surfaces and iron-gall ink. Dark front-of-house screens
  such as pause may use the inverse palette.
- `EMBER` means selected/commanded. Debug tools use `SLATE`; destructive actions use the
  danger color. Do not create another saturated accent in a screen module.
- Use `foundation::type_scale` for new text: caption 10, body 12, value 13, heading 17,
  title 21 on the 1600×900 design canvas.
- Panels use a one-pixel rule, 2–3 px radius and `plate_shadow()`. Nested content uses
  soft dividers more often than boxes inside boxes.
- Future authored frames should be nine-sliced `ImageNode`s. They must replace chrome,
  never become baked text or screen-sized bitmaps.

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
`Developer` and `Danger`. A development-time audit fails immediately if an ordinary Bevy
`Button` lacks `UiButtonStyle`. `UiButtonStyleExempt` is reserved for the full-screen
outside-click target created by `modal_backdrop_chrome`; it is not an escape hatch for custom
button styling.

Unavailable controls remain visible and add Bevy's `InteractionDisabled`; do not leave an
active button in the tree and merely ignore its click. The foundation removes disabled
controls from tab order. Standard controls receive tab navigation, a visible keyboard focus
ring and Bevy's automatic button accessibility role/label.

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

The default panel is the limewash ledger shell. A deliberately dark front-of-house screen must
override it explicitly; developer and debug screens do not receive a separate prototype theme.

Never color both the root and backdrop: their alpha compounds. `InputState::modal_open` is
derived from `ModalRoot`; do not borrow an unrelated flag such as inventory state. The hero
creator is the one transparent-backdrop exception because its 3D diorama is intentionally
seen through the panel, but it still carries the shared modal root, layer and tab group.

Use `X` to dismiss a top-level window and `BACK TO …` when returning to an owning record.
Escape and clicking the backdrop dismiss the same top-level target. A Back action must restore
the prior selection instead of opening an unrelated copy of the screen.

Button state is computed after screen state in `PostUpdate`, so plugin registration order cannot
introduce a one-frame selected/hover mismatch. Layer assignments are centralized: presentation
−1000, HUD 0, floating panel 100, modal 1000, tooltip 1100 and toast 1200. Screen-local child
ordering may use local `ZIndex`.

## Live simulation panels

Economy values can change every server tick, especially at 10×–100×. Do not rebuild a panel
at that cadence while the user is interacting with it.

The property market, business management, trade, history, permit tray and compact inspector
follow the shared rule:

- compare a screen signature and do nothing when it is unchanged;
- cap structural refresh to about eight per real second with `UiRefreshStamp`;
- defer a structural refresh while a real control in that subtree is hovered or pressed;
- mark the full-screen backdrop `UiRefreshExempt`, so parking the pointer outside the panel
  does not freeze its values; and
- preserve scroll position when rebuilding the same record.

Prefer stable entities plus diff-gated text/state updates for a new complex screen. A bounded
rebuild is the compatibility path for the existing data-heavy ledgers, not permission to
despawn a button under the cursor.

## Type scale and ledgers

The UI scale is derived from the 1600x900 launcher frame, so small captions land at roughly
6 pt on a Retina laptop. Panels declare a type scale at the top of their module (title 22,
heading 17, value 15, button 14, body 13.5, label 11-12) and nothing a player must read to make a
decision sits below body size. Place pages lead with key-figure tiles, then grouped sections
(`group_rows` in `encyclopedia/places.rs`); history charts draw real lines
(`spawn_chart_segment` rotates a thin node with `UiTransform`). The UI font has no middle-dot
glyph; separate with ` / `.

## The encyclopedia is one window with pages

Ledgers (`history.rs`) and company controls (`business_management.rs`) are not modals: they
render inside `EncyclopediaPageHost`, full size, under one BACK bar that names its destination
("BACK TO ALDRIC GRAIN & BREAD"). Their target resources (`HistoryPanelTarget`,
`BusinessManagementTarget`) are the page state; setting one from anywhere — a company record,
the compact settlement card, the market board — opens the encyclopedia on the matching tab and
hosts the page next frame. ESC pops a page before it closes the window; the X closes everything
and `close_pages_with_encyclopedia` clears the page targets so nothing reopens itself. New
pages follow the same shape: spawn into the host, carry a `UiRefreshStamp` root, no own close.

## Responsive layout and scrolling

- Large panels use viewport-relative width/height plus pixel maxima. The global UI scale keeps
  the 1600×900 design canvas usable at lower output resolutions.
- Every bounded content column has `min_height: 0`, `Overflow::scroll_y()` and a visible
  scrollbar width. Nested scroll views rely on `UiScrollPlugin` bubbling; do not consume wheel
  events in a screen-specific system.
- Output resolution, display mode and 3D render scale are separate. The world may render below
  native resolution, but UI renders at the window resolution.

## Regression checks

The client unit suite covers query scoping, one-scrim modals, the button-contract audit,
disabled/selected contrast, hover-safe refresh and nested scrolling:

```bash
cargo test -p client --lib
```

The deterministic visual harness can render the real property board without opening the game
manually:

```bash
FISTFORCE_CAPTURE_SETTLEMENT=village \
FISTFORCE_CAPTURE_PROPERTY=1 \
FISTFORCE_CAPTURE_HUD=god \
BEVY_ASSET_ROOT="$PWD/client/assets" \
cargo run --profile playtest -p client --bin capture -- \
  --at 0,0 --name property-ui --zoom 160 \
  --out /tmp/fistworld-ui --warmup 90 --settle 30
```

Inspect the result at `/tmp/fistworld-ui/property-ui.png`. `FISTFORCE_CAPTURE_PROPERTY=sale` opens the
board on its FOR SALE tab instead of PERMITS. The world must remain visible under
one dark scrim; no surface may flash to `BUTTON_NORMAL` merely because the pointer entered it.

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
