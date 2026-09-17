
---

# Follow-up 2026-09-17 afternoon: animation LOD + wardrobe stash (branch `perf-crowd-lod`)

Same harness, dense-stress town, camera locked at `112,-158`, display on and unlocked
(verified via `pmset -g log`), one binary with env kill switches:

| run | env | mesh entities | rigs skipped/frame | p50 | probe after |
|---|---|---:|---:|---:|---|
| lod-base | `FISTFORCE_ANIM_LOD_OFF=1 FISTFORCE_WARDROBE_STASH_OFF=1` | 23,820 | 0 | **33.09** | 84 |
| lod-only | `FISTFORCE_WARDROBE_STASH_OFF=1` | 23,834 | 747 | **30.03** | 94 |
| stash-only | `FISTFORCE_ANIM_LOD_OFF=1` | 5,857 | 0 | **31.58** | 116 (warm) |
| lod-both | (none) | 5,849 | 750 | **28.20** | 83 |

- Animation update-rate LOD (tiers 40/100/200 m -> 20/12/10 Hz): -3.1 ms.
- Despawning unworn wardrobe primitives (stash + rebuild on re-dress): -1.5 ms.
- Together: -4.9 ms (33.1 -> 28.2 ms, ~15%). Effects are additive.
- Tuning knobs: `ANIM_LOD_TIERS` / `ANIM_LOD_FAR_INTERVAL` in `hero/animation.rs`.
  Visual check of the 10 Hz far tier at RTS zoom is still to be done by eye.
- Next lever: one skinned mesh per villager, see `CROWD-MESH-MERGE-PLAN.md`.

## Follow-up: one-mesh-per-villager (Phase A) — implemented, measured, defaulted OFF

Works (1,001 merged rigs, mesh entities 5.8k -> 2.8k, pixel-identical capture, tests
pass) but measures 0-1.5 ms in back-to-back pairs, within noise. Left behind
`FISTFORCE_MERGE_OUTFITS=1`. Details and the corrected reasoning in
`CROWD-MESH-MERGE-PLAN.md`.
