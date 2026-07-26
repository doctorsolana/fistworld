# P2 — Items & Inventory: removal manifest

Repo: `/Users/terminator2/Coding/citysim` (Bevy 0.18.1 + lightyear 0.26, workspace: `shared/ client/ server/ editor/`)
Recovery: everything below exists at git tag `citysim-final` → `git show citysim-final:<path>`.
**EXCEPT** `server_data/` — it is gitignored (`.gitignore:24`), so it is **NOT** in the tag and **NOT** recoverable.

Scope verified by reading every file listed. Every path and symbol below was confirmed to exist.

---

## 0. DANGER — read before touching anything

### D1. Removing `PlayerProfile.inventory_slots` silently destroys every existing save. VERIFIED EMPIRICALLY.

`shared/src/player_profile.rs:45` — `pub inventory_slots: [Option<ItemStack>; INVENTORY_SLOTS]`
`shared/src/player_profile.rs:47` — `pub hotbar_selection: u8`

Profiles are bincode **1.3.3** (`shared/Cargo.toml:12`, `server/Cargo.toml:10`), written by
`server/src/persistence/profiles.rs:118` (`bincode::serialize`) and read at
`server/src/persistence/profiles.rs:48` and `server/src/player/roster_cache.rs:38`
(`bincode::deserialize`).

Facts that make this dangerous:

1. **bincode is not self-describing.** `#[serde(default)]` (used on `level`, `prestige`,
   `reputation`, `stamina`, `intelligence`, `bank_gold` at `player_profile.rs:71-87`) does
   **nothing** for bincode. Fields are read positionally. Delete a field in the middle of the
   struct and every byte after it is reinterpreted.
2. **The version guard cannot catch this.** `version: u32` is the **first** field
   (`player_profile.rs:20`), so it still decodes as `1` after the shift. The check at
   `profiles.rs:52` (`if profile.version != PROFILE_VERSION`) therefore **passes**, and the
   `.vN.backup` copy at `profiles.rs:53-60` is **never written**.
3. **bincode's free `deserialize` allows trailing bytes**
   (`bincode-1.3.3/src/lib.rs:183` calls `.allow_trailing_bytes()`), so a short-read that
   leaves 25 unread bytes is *not* an error.

I re-implemented the exact bincode layout in Python and ran it against the **48 real profiles**
currently in `server_data/players/*.bin`:

| scenario | result |
| --- | --- |
| current schema | 48/48 decode cleanly (parser validated) |
| after removing `inventory_slots` + `hotbar_selection` | **48/48 fail** with `invalid Option tag 6` — `in_vehicle: bool` eats `inventory_slots[0]`'s `Some` tag (`1` → `true`), then `vehicle_type: Option<VehicleType>` reads the low byte of the `ItemType` u32 discriminant `6` (= `Weapon`) and rejects it |
| same, but with an **empty** inventory (24 × `0x00` + hotbar `0x00`) | **decodes "successfully" into garbage**: version=1 (passes the guard!), position/health survive, but `last_login` collapses to the UNIX epoch, `total_playtime_secs`/`intelligence`/`bank_gold` are zeroed, and 25 bytes are silently dropped |

(Script kept at `…/scratchpad/parse_profile.py` if you want to re-run it.)

**What the server actually does with the failure** (traced, no panic anywhere):

- `PlayerProfiles::load_profile` (`server/src/persistence/profiles.rs:38`) returns
  `Err("Failed to deserialize …")`.
- `handle_player_name_submission` (`server/src/player/spawn.rs:90-98`) treats *any* `Err` as
  "no profile": logs `Creating new profile for '{}': {}` and builds
  `PlayerProfile::new_player(name)`. The player silently loses position, `level`, `prestige`,
  `reputation`, `stamina`, `intelligence`, `bank_gold`, `total_playtime_secs`.
- On the next autosave (`persistence/autosave.rs:145`) or disconnect
  (`net/connection.rs:335`) the fresh profile is written over `<name>.bin` via
  `save_profile_to_dir` → **original bytes gone, unrecoverable** (gitignored).
- `PlayerRosterCache::from_storage_dir` (`server/src/player/roster_cache.rs:38-43`) does
  `let Ok(profile) = … else { continue }` → all 48 entries vanish; the roster/leaderboard
  (`PlayerRosterEntry`) is empty until players reconnect.
- No OOM risk: `player_name: String` is decoded *before* the removed fields, so no bogus
  length prefix is ever read.

**Required mitigation — do at least one, ideally all three:**

- [ ] Bump `PROFILE_VERSION` (`shared/src/player_profile.rs:14`) `1 → 2` in the **same commit**
      as the field removal. This is the only thing that catches the silent-garbage case (D1 row 3).
- [ ] Make `load_profile` back up on deserialize failure too, not just on version mismatch —
      move the `.backup` copy at `profiles.rs:53-60` above the version check, or add a
      `.corrupt` copy in the `map_err` at `profiles.rs:48-49`.
- [ ] Since these are 48 throwaway dev saves (names like `asd`, `qweq`, `123123123`), the
      pragmatic move is to `mv server_data/players server_data/players.pre-p2` **before** the
      first post-change server boot. There is no other copy.

### D2. `shared/src/building/` is on the KEEP list and imports `ItemType`.

`shared/src/building/defs.rs:4` `use crate::items::ItemType;`
`shared/src/building/defs.rs:224` `pub cost: &'static [(ItemType, u32)],`
Populated at lines 118, 128, 138, 148, 158 (and `&[]` at 209).

**Verified dead data**: `rg '\.cost\b'` finds *no reader* of `BuildingDef::cost` anywhere in
`shared/`, `server/`, `client/`, or `editor/`. Delete the field and its 6 initializers; nothing
else changes. Do **not** leave the `use crate::items::ItemType` behind — `shared/src/building`
must still compile after `shared/src/items` is gone.

### D3. `shared/src/protocol/plugin.rs` is on the KEEP list (protocol plugin structure survives).

7 `register_component` and 7 `register_message` calls must be removed together with the imports
at lines 11-14. Registration order is not semantically load-bearing here (lightyear derives net
ids from registration), but client and server **must** be rebuilt together — a client with the
old registration set and a server with the new one will mismatch component/message ids and
desync or drop messages. Ship both binaries at once.

### D4. `client/src/pickup/` also owns the **vehicle** mount prompt (that's P3, not P2).

`client/src/pickup/mod.rs` declares `NearbyVehicle`, `VehiclePrompt`; `prompts.rs` implements
`detect_nearby_vehicles` (line 6) and `show_vehicle_prompt` (line 65). Deleting the directory in
P2 kills the "Press E to get on bike" prompt early. That is fine if P2 lands before or with P3;
just don't be surprised. `VEHICLE_INTERACTION_RANGE` (`shared/src/items/constants.rs:14`) is its
only other consumer — note the server does **not** use it (`shared/src/vehicle/physics/interaction.rs:7`
hardcodes `dist < 3.0`), so the constant dies with `items/`.

### D5. `InputState::inventory_open` is read by KEEP-list UI.

`client/src/input.rs:65` (field), `:98` (init), `:110` (`ui_blocking()`), `:119`, `:171`, `:350`.
External readers that survive P2:
- `client/src/ui/world_map/layout.rs:15` (world map UI — KEEP)
- `client/src/ui/pause_menu/actions.rs:461` (pause menu — KEEP)
- `client/src/ui/debug_time_menu/actions.rs:22`

Removing the field requires editing all of those. Safer path: **keep the field**, delete only its
writers (which all live in deleted files), and revisit in P6. If you do remove it, `ui_blocking()`
at `input.rs:110` must drop the first disjunct.

### D6. No editor / map-schema coupling. (Good news, verified.)

`rg` over `editor/src` finds zero references to `ItemType`/`Inventory`/`Chest` (the only hits are
`egui::style.item_spacing` and `Iterator<Item = u64>`). `shared/src/map`, `shared/src/props`,
`shared/src/city` contain **no** item or chest prop kinds — chests are spawned purely in code by
`server/src/inventory/chest.rs:26 spawn_world_chests`. Nothing in a `.ron` map file references
items, so **no map migration is needed**.

### D7. No shop / dialogue-shop / bank hooks exist.

`client/src/dialogue.rs` (380 lines) has **zero** item/inventory/trade/buy/sell references —
searched. The only currency artifacts are:
- `ItemType::GoldCoin` (`shared/src/items/types.rs:18`) — dies with `items/`.
- `PlayerProfile.bank_gold: u64` (`player_profile.rs:87`) — an orphan. It is only ever
  round-tripped (`net/connection.rs:310-314`, `persistence/autosave.rs:134-138`) and initialised
  to `0` (`player_profile.rs:189`). Nothing ever increments it. Removing it is optional but
  falls under the same D1 byte-layout hazard, so do it in the same commit + version bump if at all.
- `shared/src/economy.rs` `STARTING_COMPANY_MONEY` / `shared/src/rail.rs:27 Company.money` are
  the **rail** economy — that is P3, untouched by P2.

---

## 1. Delete wholesale

| Path | Lines | Why |
| --- | --- | --- |
| `shared/src/items/` (`mod.rs`, `constants.rs`, `types.rs`, `inventory.rs`, `world.rs`, `messages.rs`) | 705 | the entire item/inventory/chest/ground-item domain + its 7 net messages |
| `server/src/inventory/` (`mod.rs`, `chest.rs`, `death_drop.rs`, `ground_items.rs`, `hotbar.rs`) | 728 | server-authoritative pickup/drop/chest/hotbar + `OpenChests` resource + `PreviousHotbarSlot` |
| `client/src/ui/inventory/` (`mod.rs`, `chest.rs`, `drag_drop.rs`, `layout.rs`, `slots.rs`) | 1185 | `InventoryPlugin`, the grid UI, drag&drop, chest panel, `ItemPreviewAssets` |
| `client/src/pickup/` (`mod.rs`, `assets.rs`, `items.rs`, `prompts.rs`, `visuals.rs`) | 547 | `PickupPlugin`, ground-item 3D visuals + bobbing, pickup prompt — **and the vehicle prompt, see D4** |
| `client/src/chest.rs` | 315 | `ChestPlugin`, chest proximity/visuals/`OpenChest` |

Subtotal ≈ **3,480** lines of Rust deleted outright.

---

## 2. Files that survive but need edits

### 2a. `shared/` (KEEP-list crate — must still compile)

- [ ] **`shared/src/lib.rs:6`** — remove `pub mod items;`
- [ ] **`shared/src/prelude.rs:10`** — remove
      `pub use crate::items::{ChestStorage, HotbarSelection, Inventory, ItemStack, ItemType};`
      (nothing in the repo does `use shared::prelude::*` today — verified — but the file must compile)
- [ ] **`shared/src/player_profile.rs`** — see D1 first.
  - `:7` drop `use crate::items::{ItemStack, ItemType, INVENTORY_SLOTS};`
  - `:14` bump `PROFILE_VERSION` to `2`
  - `:43-47` remove the `// === Inventory ===` block: `inventory_slots`, `hotbar_selection`
  - `:98-151` remove the whole starting-inventory construction inside `new_player`
  - `:168-169` remove `inventory_slots,` / `hotbar_selection: 0,` from the struct literal
  - (optional, same commit) `:85-87` `bank_gold` and `:189` its initialiser
  - NOTE: `equipped_weapon: WeaponType` (`:39`) and `weapon_ammo_in_mag` (`:41`) are **P1**, not P2.
    If P1 lands separately you get a *second* byte-layout break — do the version bump once per
    schema change, or land P1+P2 profile edits together.
- [ ] **`shared/src/protocol/plugin.rs`** (KEEP — see D3)
  - `:11-14` remove the entire `use crate::items::{…}` block (12 symbols)
  - `:74-76` remove `register_component::<Inventory>`, `<GroundItem>`, `<GroundItemPosition>`
  - `:83` remove `register_component::<HotbarSelection>`
  - `:86-87` remove `register_component::<ChestStorage>`, `<ChestPosition>`
  - `:127-140` remove `register_message::<PickupRequest | DropRequest | SelectHotbarSlot | InventoryMoveRequest | OpenChestRequest | CloseChestRequest | ChestTransferRequest>` (each with its `.add_direction(NetworkDirection::ClientToServer)`)
  - the `// === INVENTORY COMPONENTS ===`, `// === EQUIPMENT / HOTBAR ===`, `// === CHEST / STORAGE ===` comment banners go too
- [ ] **`shared/src/building/defs.rs`** (KEEP — see D2)
  - `:4` drop `use crate::items::ItemType;`
  - `:224` remove `pub cost: &'static [(ItemType, u32)],` and the doc comment at `:223`
  - remove the `cost:` initialiser from all 6 `BuildingDef` literals: `:118, :128, :138, :148, :158, :209`
- [ ] **`shared/src/weapons/types.rs`** (P1 kill list, but breaks the build if P2 lands first)
  - `:3` `use crate::items::ItemType;`
  - `:177-187` `pub fn ammo_type(&self) -> ItemType`
  - `:189-195` `pub fn as_item_type(&self) -> Option<ItemType>`
  - `:16` the comment `// NOTE: profiles persist this enum via bincode — append variants only.`
    becomes obsolete once `equipped_weapon` leaves the profile in P1.
- [ ] **`shared/src/components/combat.rs`** (P1 kill list, same caveat)
  - `:118-131` `EquippedWeapon::reload_from_inventory`
  - `:133-137` `EquippedWeapon::get_reserve_from_inventory`

### 2b. `server/`

- [ ] **`server/src/main.rs:12`** — remove `mod inventory;`
- [ ] **`server/src/app/resources.rs`**
  - `:12` drop `use crate::inventory;`
  - `:30` drop `app.init_resource::<inventory::chest::OpenChests>();`
- [ ] **`server/src/app/schedule.rs`**
  - `:16` drop `use crate::inventory;`
  - `:56` remove `Inventory,` from `enum FpsServerSet`
  - `:74` remove `FpsServerSet::Inventory,` from the `.chain()` ordering
  - `:222-239` delete the whole `add_systems(FixedUpdate, (…).in_set(FpsServerSet::Inventory))`
    block — 9 systems: `hotbar::handle_hotbar_selection_requests`,
    `hotbar::handle_inventory_move_requests`, `ground_items::handle_pickup_requests`,
    `ground_items::handle_drop_requests`, `hotbar::sync_equipped_weapon_from_hotbar`,
    `chest::handle_open_chest_requests`, `chest::handle_close_chest_requests`,
    `chest::handle_chest_transfer_requests`, `chest::update_distant_chest_auto_close`
  - `:256` remove `inventory::death_drop::handle_inventory_drop_on_death,` from the `Combat` chain
  - `:274` **`.after(inventory::chest::update_distant_chest_auto_close)`** on
    `telemetry::perf::handle_perf_npc_inventory_build_phase_end` — re-anchor to the last surviving
    AI system (e.g. `ai::obstacles::sync_obstacle_grid` or whatever P4 leaves)
  - `:278, :280, :282` three `.after(inventory::death_drop::handle_inventory_drop_on_death)`
    anchors on `handle_perf_weapons_phase_end`, `update_server_perf_log`,
    `sample_replication_change_pressure` — re-anchor to a surviving combat system, e.g.
    `combat::cleanup::cleanup_bullets`
  - `:3` doc comment mentions "inventory" — cosmetic
- [ ] **`server/src/app/bootstrap.rs:104`** — remove `crate::inventory::chest::spawn_world_chests,`
      from the `!rail_mode_enabled()` startup tuple (leaves
      `crate::vehicle::bootstrap::spawn_world_vehicles` alone in the tuple — de-tuple it)
- [ ] **`server/src/net/connection.rs`**
  - `:12-15` drop the whole `use shared::items::{…}` block
  - `:107-113` remove the `MessageReceiver::<PickupRequest | DropRequest | SelectHotbarSlot | InventoryMoveRequest | OpenChestRequest | CloseChestRequest | ChestTransferRequest>` insert tuple (the whole `commands.entity(client_entity).insert((…))` statement)
  - `:146-147` remove `&Inventory,` and `&HotbarSelection,` from the `players: Query<(…)>` tuple in
    `handle_disconnections`
  - `:193-194`, `:208-209`, `:225-226` remove `inventory,` / `hotbar,` from the three
    destructuring / re-binding tuples
  - `:286-287` remove `inventory_slots: *inventory.slots(),` and `hotbar_selection: hotbar.index,`
    from the `PlayerProfile { … }` literal
- [ ] **`server/src/persistence/autosave.rs`**
  - `:8` drop `use shared::items::{HotbarSelection, Inventory};`
  - `:33-34` remove `&Inventory,` `&HotbarSelection,` from the query tuple
  - `:58-59` remove `inventory,` `hotbar,` from the `for (…) in players.iter()` destructuring
  - `:110-111` remove `inventory_slots: *inventory.slots(),` and `hotbar_selection: hotbar.index,`
- [ ] **`server/src/player/spawn.rs`** (this file is heavily inventory-shaped; the whole
      profile→spawn tuple should be simplified)
  - `:14` drop `use shared::items::{HotbarSelection, Inventory};`
  - `:26` drop `use crate::inventory::hotbar::PreviousHotbarSlot;`
  - `:99-119` the 9-tuple type annotation loses `Inventory` and the `u8` hotbar slot
  - `:135` `Inventory::new(),` (dead-player branch)
  - `:142-146`, `:174-178`, `:201-205` — three identical
    `let mut inventory = Inventory::new(); for (i, slot) in profile.inventory_slots.iter()…` loops
  - `:246-250` remove `inventory,`, `HotbarSelection { index: hotbar_sel },` and
    `PreviousHotbarSlot { index: Some(hotbar_sel as usize) },` from the `commands.spawn((…))` bundle
  - `:122` log string "spawning at spawn point with empty inventory" — cosmetic
- [ ] **`server/src/telemetry/perf.rs`** (KEEP-adjacent; the perf log survives)
  - `:5` drop `use shared::items::GroundItem;`
  - `:248` remove the `ground_items: Query<(), With<GroundItem>>` system param
  - `:331` remove `ground_items={}` from the format string; `:359` remove
    `ground_items.iter().count(),`
  - `:19, :34` the `Phase::NpcInventoryBuild` variant name is now a misnomer (it brackets
    AI+inventory). Rename to `Phase::NpcBuild` or leave; the two systems at `:219` / `:225` keep
    working, only their schedule anchors change (see schedule.rs `:274` above).
- [ ] **`server/src/combat/reload.rs`** (P1 kill list, but breaks the build if P2 lands first)
  - `:8` `use shared::items::Inventory;`
  - `:29, :76` `&mut Inventory,` query params
  - `:41, :60, :107-108, :115, :126` `inventory.add_item / remove_item / count_item` calls
- [ ] `server/src/combat/mod.rs:10` — doc comment says "May depend on … `inventory`" — cosmetic

### 2c. `client/`

- [ ] **`client/src/main.rs:6`** remove `mod chest;`, **`:11`** remove `mod pickup;`
- [ ] **`client/src/app_wiring/mod.rs:34`** — remove `chest,` and `pickup,` from the
      `use crate::{…}` list
- [ ] **`client/src/app_wiring/plugins.rs:124-130`** — remove
      `app.add_plugins(ui::InventoryPlugin);`, `app.add_plugins(pickup::PickupPlugin);`,
      `app.add_plugins(chest::ChestPlugin);` from the `if !rail_mode` block; fix the comment at
      `:124-125`
- [ ] **`client/src/ui/mod.rs:4`** remove `pub mod inventory;`, **`:13`** remove
      `pub use inventory::InventoryPlugin;`
- [ ] **`client/src/render/systems/connection.rs`**
  - `:106-109` remove `MessageSender::<shared::items::PickupRequest | DropRequest | SelectHotbarSlot | InventoryMoveRequest>::default()`
  - `:116-120` remove the entire second `commands.entity(client_entity).insert((…))` chest-sender
    statement (`OpenChestRequest`, `CloseChestRequest`, `ChestTransferRequest`) plus its comment
- [ ] **`client/src/input.rs`** — see D5. Either keep `inventory_open` untouched (recommended for
      P2) or remove `:65, :98` and fix `:110, :119, :171, :350` **plus** the three external readers
      listed in D5.
- [ ] **`client/src/weapon_view/hotbar_input.rs`** (P1 file, whole file is hotbar) —
      `:4` `use shared::items::{HotbarSelection, SelectHotbarSlot};`, `:13`, `:16`, `:48`.
      Delete the file with P1 or stub `handle_weapon_switch`.
- [ ] **`client/src/weapon_view/weapon_hud.rs`** (P1)
  - `:3` `use shared::items::HotbarSelection;`
  - `:98` `&shared::items::Inventory, &HotbarSelection` in the `local_player` query
  - `:137` destructuring, `:146-163` the hotbar-slot display loop (uses
    `shared::items::HOTBAR_SLOTS` at `:149` and `inventory.get_slot`)
  - `:173` `inventory.count_item(weapon.weapon_type.ammo_type())`
  - `:192-212` the whole `fn hotbar_item_name(item_type: &shared::items::ItemType)`
- [ ] **`client/src/weapons/input.rs`** (P1) — `:311` `&shared::items::Inventory` query param,
      `:335-345` `inventory.count_item(...)` reserve-ammo gate

---

## 3. Assets used only by this slice

| Path | Size | Sole consumer |
| --- | --- | --- |
| `client/assets/game_assets/items/` (`bullet.glb`, `rifle_bullet.glb`, `shotgun_bullet.glb`, `stone.glb`, `wood.glb`) | 424 KB | `client/src/pickup/assets.rs:5-31` |
| `client/assets/ui/item_preview/bullet512.png`, `rifle_bullet512.png`, `shotgun_bullet512.png`, `stone512.png`, `wood512.png` | ~66 KB | `client/src/ui/inventory/layout.rs:37-61` |
| `client/assets/ui/item_preview/automatic512.png`, `shotgun512.png`, `sniper512.png`, `revolver512.png` | ~45 KB | `client/src/ui/inventory/layout.rs:20-36` — **weapon** icons, so strictly P1's call, but the *only* loader is the (deleted) inventory UI, so they become orphans at P2 |

After removing all 9 PNGs the directory `client/assets/ui/item_preview/` is empty and can go.
`client/assets/ui/fistforce.png` is **not** part of this slice (main-menu logo, P6 naming).
`rg 'item_preview'` confirms no other consumer.

---

## 4. Persisted / on-disk data

| Artifact | Format | Risk |
| --- | --- | --- |
| `server_data/players/*.bin` (48 files) | bincode 1.3 `PlayerProfile` | **CRITICAL** — see D1. All 48 fail to deserialize; server silently reissues fresh profiles and overwrites them. Gitignored → no backup exists anywhere. |
| `PROFILE_VERSION` (`shared/src/player_profile.rs:14`) | `u32 = 1` | Must be bumped; the guard at `profiles.rs:52` cannot otherwise detect the layout shift because `version` is field #0. |
| `ItemType` (`shared/src/items/types.rs:7`) | bincode enum, u32 variant tag | Embedded in every saved `ItemStack`. Deleting the enum deletes the only decoder. |
| `WeaponType` (`shared/src/weapons/types.rs:8`) | bincode enum | Carries the explicit `// append variants only` contract at `:16` because it is persisted twice: `PlayerProfile.equipped_weapon` and inside `ItemStack::item_type`. P2 removes the second occurrence; P1 removes the first. |
| map `.ron` files (`client/assets/maps/`) | RON | **Unaffected** — no item/chest entries; chests are code-spawned only (D6). |
| `client/assets/colliders.bin` | bincode `ColliderDb` (`shared/src/colliders.rs:29`) | **Unaffected** — different schema, no item types. |

---

## 5. Cargo dependencies

**None become unused.** `bincode` stays needed by `server/src/persistence/*` +
`server/src/player/roster_cache.rs` and by `shared/src/colliders.rs` +
`shared/src/protocol/messages.rs` tests. `rand` (client) is used far beyond
`client/src/pickup/visuals.rs:52`. No crate can be dropped from any `Cargo.toml` for P2 alone.

---

## 6. Suggested execution order

1. Move the saves aside: `mv server_data/players server_data/players.pre-p2` (D1).
2. `shared/`: bump `PROFILE_VERSION`, gut `player_profile.rs`, strip `protocol/plugin.rs`,
   strip `building/defs.rs`, then delete `shared/src/items/` + `lib.rs`/`prelude.rs` lines.
   Compile `shared` alone — the `weapons/types.rs` and `components/combat.rs` breakages (P1
   symbols) surface here and must be fixed in the same pass.
3. `server/`: delete `server/src/inventory/`, then fix `main.rs`, `app/{resources,schedule,bootstrap}.rs`,
   `net/connection.rs`, `persistence/autosave.rs`, `player/spawn.rs`, `telemetry/perf.rs`,
   `combat/reload.rs`. Watch the four schedule `.after(...)` re-anchors (schedule.rs `:274, :278, :280, :282`).
4. `client/`: delete `ui/inventory/`, `pickup/`, `chest.rs`, then fix `main.rs`,
   `app_wiring/{mod,plugins}.rs`, `ui/mod.rs`, `render/systems/connection.rs`,
   `weapon_view/{hotbar_input,weapon_hud}.rs`, `weapons/input.rs`.
5. Delete assets (section 3).
6. `cargo check -p editor` — should be a no-op, but it is the KEEP-list canary.
7. Boot server + client **together** (D3) with a fresh `server_data/`, submit a name, confirm
   spawn + roster + autosave round-trip.
