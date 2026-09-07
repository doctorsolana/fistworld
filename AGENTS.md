# Repository instructions for coding agents

Read `CONTRIBUTING.md` and `docs/ARCHITECTURE.md` before changing simulation,
networking, terrain, rendering, or UI code. The server is authoritative; generated
worlds are deterministic recipes; existing user changes in a dirty worktree must be
preserved.

Before authoring building or prop assets, read `asset_creation/PROP_PIPELINE.md`,
including the ground-contact, supported-cargo, structural-joint, hinge and roof-underside checks.

Keep generated screenshots, recordings and assembled Blender review scenes out of Git.
Use `logs/` or the ignored `asset_creation/**/renders/` directories for review output;
retain canonical editable `.blend` sources, runtime assets, generators and capture
scenarios. Only intentionally maintained regression baselines belong in version control.

## Visual work is not complete until it is seen

For any renderer, terrain, water, asset, camera, animation, visibility, LOD, or retained-UI
change, use the real Bevy capture harness described in `docs/VISUAL-CAPTURE.md`.

- Prefer a checked-in RON scenario under `capture/scenarios/` when camera position, map,
  world fixture, timing, readiness, or assertions matter.
- Wait on semantic readiness rather than adding arbitrary sleeps.
- Inspect both the PNG and its `.capture.json`; compilation and logs are not visual proof.
- Use `target: scene` for deterministic 3D output and `target: window` for composed UI.
- Reproduce moving/streaming defects with a continuous scenario, not a static screenshot.
- Only update a baseline after personally inspecting the new output. Never loosen tolerances
  to hide nondeterminism.
- Connected NPC behavior still requires the real client/server lab; the offline harness is
  for rendering and deterministic UI fixtures.

Minimum code verification remains `cargo check --workspace --all-targets` and the relevant
tests. Visual changes additionally require at least one real capture of the affected view.
