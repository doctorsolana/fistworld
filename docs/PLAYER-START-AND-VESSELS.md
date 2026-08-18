# Player start and vessels

Audited against the executable state on 2026-08-18. This document describes the normal
player arrival and the reusable water-navigation seam. The server remains authoritative;
the client only presents the vessel and sends destinations.

## New-player flow

1. A player submits an account name. A returning account on the same running server
   re-adopts its existing hero and skips creation.
2. A genuinely new account receives the mandatory **Create Your Hero** screen. Confirming
   sends only the outfit; the client does not choose a spawn position.
3. The server samples an approach corridor from the active map edge using the stable account
   seed. It accepts only continuous water leading to reachable dry shore, then starts the
   Dinghy roughly 48–56 metres offshore: close enough for a short opening voyage, while
   leaving enough water for sailing to feel like an arrival rather than starting on the beach.
   A fresh server begins at 08:00 on the display clock, with the sun already above the
   horizon, so the first arrival is a readable warm sunrise rather than darkness.
4. The server creates exactly one Hero and one starter Dinghy. The Hero is seated at the
   authored `Anchor_Helm` position. Presentation waits for the replicated authoritative
   heading before instantiating the hull, so the boat cannot begin at a fallback rotation
   and visibly turn around during the opening shot. Boat translation also keeps the Hero's
   `sit_idle` pose; vessel speed is never interpreted as walking speed.
5. After the dressed character and Dinghy scene are both ready, the camera uses the
   animated head joint for a 3.5-second portrait while the one-shot game-intro cue plays.
   That portrait slowly pulls back and rises rather than remaining frozen, then the camera
   cranes farther backward and upward while keeping the Hero and full Dinghy in frame. It
   hands off at the normal RTS angle with the Dinghy selected. Reconnects do not replay
   either the camera or the cue.
6. Right-clicking water issues a server-authoritative sailing order. Right-clicking dry
   ground within 11 metres disembarks the Hero. The starter Dinghy becomes a slack,
   non-commandable shoreline wreck and selection passes to the Hero.

The server intentionally starts a fresh world and account state on process restart. This
flow does not claim durable cross-restart world persistence. Development God Mode remains
available: a God-capable client can press `G` even while the new-player creator is open.
The automated `FISTWORLD_AUTOSPAWN_HERO=1` smoke path also bypasses the mandatory modal.

## Navigation contract

`Vessel` is the generic server-side opt-in for Dinghies, future merchant ships and war
ships. `PlayerBoat` currently selects the Dinghy presentation; it is not the general
definition of a ship.

- Water and land navigation are separate. A vessel route is never fed to the road or
  character navigator and every traversed segment is certified as water.
- Cursor picking intersects the visible water surface rather than the seabed. This keeps
  near-edge water clicks at the point shown under the pointer instead of projecting them
  outside the playable map.
- Open-water clicks use a direct-line fast path. Obstructed routes use a six-metre water
  A* grid followed by water-certified line-of-sight simplification.
- Exact clicked endpoints connect to nearby water grid cells, avoiding false refusal when
  a valid water click rounds to a grid centre just across the shoreline.
- Route requests enter a bounded queue. At most four vessel searches are performed per
  fixed server tick, and a newer order replaces an older pending order for the same ship.
  A fleet command therefore cannot create an unbounded network-ingress stall.
- Movement checks the retained route against current terrain again. If world changes make
  it invalid, the vessel stops at the last valid water point instead of beaching.
- `VesselNavigation` supplies hull speed. Future ship classes reuse the planner and add
  their own speed, cargo, crew, draft and combat components.
- Navigation coordinates remain on the server's stable water plane. On the client, the Dinghy
  samples the shared deterministic swell at its centre, bow, stern and both sides, then smoothly
  follows that surface with height, pitch and roll. The seated Hero is visually pinned to the
  resulting helm transform while its replicated position remains authoritative. The matching
  tapered false sole sits just above the authored waterline and hides residual wave curvature
  through the open floorboards without raising the whole hull out of the sea.

Depth/draft, ship-to-ship avoidance, docks, boarding, repair, cargo vessels and naval
combat are future mechanics. The present navigator proves coastline-constrained travel
without pretending those systems already exist.

## Wind and sails

Wind direction, gust speed and sailing response live in `shared::wind`, so server motion
and client presentation use one deterministic source.

- Heading relative to the wind changes actual server speed. The forgiving initial sailing
  curve preserves steerage into the wind and is fastest on a broad reach, so a new player
  cannot become trapped offshore.
- The authored `DinghySailRig` turns toward apparent wind and the `wind_fill` morph reacts
  to wind strength and crosswind.
- When the asynchronous Dinghy scene appears, its sail is initialized from the current wind before
  it is revealed. Later apparent-wind changes turn the rig smoothly instead of snapping.
- Apparent-wind visuals divide replicated velocity by Time Warp. Fast-forward changes
  voyage duration but cannot make the sail point differently from the same voyage at 1x.
- A wreck stops sailing, hides its failed sail rig and sets sail fill to zero.

The source Dinghy contract and asset validation notes live in
`asset_creation/boats/DINGHY_HANDOVER.md`.

For an offline visual regression without starting a server:

```bash
FISTFORCE_CAPTURE_DINGHY=underway cargo run -p client --bin capture -- \
  --out /tmp/dinghy --at 0,0 --zoom 13 --tilt 0.52
FISTFORCE_CAPTURE_DINGHY=wreck cargo run -p client --bin capture -- \
  --out /tmp/dinghy-wreck --at 0,0 --zoom 13 --tilt 0.52
```

The fixture resolves the authored local water height and the normal commander camera anchors close
water views to that surface rather than the seabed.

For a true networked regression (normal name acceptance, server-side Hero/Dinghy spawn,
interest management, camera transition, and a real sailing order), run a server and then:

```bash
FISTFORCE_AUTOCONNECT=VoyageQa \
FISTWORLD_AUTOCREATE_VOYAGE=1 \
FISTWORLD_VOYAGE_CAPTURE_DIR=/tmp/fistworld-voyage \
FISTWORLD_VOYAGE_CAPTURE_EXIT=1 \
cargo run --profile playtest -p client
```

This writes `01_face.png`, `02_transition.png`, `03_rts.png`, and `04_sailing.png`.
The harness presses and releases the ordinary right-click input path. The fourth image is
taken only after the server acknowledges that resulting move order and the replicated
Dinghy has travelled at least seven metres.
