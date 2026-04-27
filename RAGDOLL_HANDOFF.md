# Ragdoll Handoff

Status as of March 7, 2026.

This document records what we learned from the Oilman ragdoll investigation so the repo can move forward without losing the important conclusions.

## Goal

The target was server-authoritative NPC ragdolls in Bevy/Rapier that:

- behave identically on all clients,
- use a 16-body physics chain,
- and render the skinned Oilman corpse in the same pose as the streamed authoritative ragdoll.

## Short Conclusion

The networking side basically works. The remaining problem is not a tuning problem. It is an asset-contract problem between the Mixamo-derived Oilman skeleton and the runtime physics rig.

The current code should be treated as a useful investigation, not as a design to preserve.

## What We Built

- Expanded the ragdoll body set to a 16-body chain:
  - pelvis
  - spine lower
  - spine upper
  - head
  - upper arms
  - forearms
  - hands
  - thighs
  - calves
  - feet
- Streamed authoritative ragdoll activation and pose updates from server to clients.
- Added extensive client-side debug instrumentation for:
  - rig map dumps
  - body coverage checks
  - start yaw deltas
  - model-root solve logs
  - body-to-bone offset logs
  - periodic alignment summaries
- Moved client ragdoll pose application to the correct Bevy window:
  - after animation
  - before transform propagation
- Repeatedly tuned server-side ragdoll physics:
  - damping
  - friction
  - death impulses
  - fixed vs generic joints
  - elbow and knee limits
  - hand and foot collider inclusion
  - living-body removal at death

## What Actually Worked

- The server-authoritative ragdoll stream works.
- The 16-body mapping works for the Mixamo/Oilman skeleton names.
- Root drift was mostly solved.
- In some builds, the client could make the mesh follow the streamed body poses very closely.
- The debug instrumentation is useful and should be kept.

Important observation:

- We reached states where `avg_root_err` was effectively zero.
- We also reached states where overall mesh/body alignment was near perfect for many bodies.
- That proves the transport and basic mapping path are not the main blockers.

## What Failed

We repeatedly got one of two bad outcomes:

- the yellow debug ragdoll looked plausible but the skinned corpse floated, curled, stretched, or sank,
- or the skinned corpse tracked closely while the underlying motion looked stiff, unstable, or physically wrong.

Typical failure modes:

- mesh half underground or floating
- corpse offset behind the debug bodies
- corpse flying away while the debug bodies stayed sensible
- arms pulling backward and legs forward on death
- half-upright leaning collapse instead of a natural fall
- large stretch spikes under force, especially in hands and forearms

Important observation:

- when things were bad, distal limbs failed first
- hands and forearms could diverge by meters while root error stayed near zero

That means the unsolved issue is not root placement. It is body-frame, joint-frame, and bind-pose mismatch.

## Why The Current Runtime Approach Is Not A Good Long-Term Path

We tried to make the current asset fit at runtime with layers of compensation:

- root correction
- bind-derived model-root translation
- start-time yaw correction
- per-body local rotation correction
- multiple bone-translation solve strategies

Each layer improved one symptom and destabilized another. The runtime now contains too much fit logic for something that should be data-driven and authored consistently.

This is the key lesson:

- do not keep searching for one more constant or one more corrective transform
- do not assume the current client bone solve is fundamentally correct
- do not assume the current Oilman GLB is a valid long-term ragdoll source asset

## Current Diagnosis

The Oilman asset and the current runtime physics rig were not authored to the same contract.

The likely mismatches are:

- bone joint frames vs physics body frames
- joint anchors vs actual character pivots
- collider offsets vs expected bone attachment points
- bind-pose semantics
- possibly helper or twist-bone assumptions inside the animated hierarchy

If those frames are not authored together, exact runtime fitting becomes fragile and expensive. Root fixes can hide the problem near the pelvis while distal limbs still explode.

## What To Keep

- Server-authoritative ragdoll ownership and streaming.
- The 16-body target.
- The current debug tooling and logging.
- The scheduling fix that applies ragdoll poses after animation and before transform propagation.

## What To Stop Treating As The Main Path

- More tuning passes on constants alone.
- More heuristic root-fit math.
- More translation solves derived from current global transforms.
- More attempts to salvage the current Mixamo/Oilman asset as the final ragdoll contract.

## Recommended Redesign

Future NPCs should be authored to a strict ragdoll contract.

### Asset Contract

- Applied transforms only.
- Uniform scale `1`.
- No hidden root yaw fix.
- Exact required physics-chain bones with stable names and clean parentage.
- Bone pivots must be the real joint pivots.
- Slight elbow and knee bend in bind pose.
- No helper or twist bones inside the physics chain.
- Physics body frames should equal bone joint frames.

### Metadata Contract

Store per-archetype ragdoll metadata in an authored asset file.

That metadata should include:

- body name
- parent body
- source bone name
- body frame relative to bone frame
- collider shape and offset
- joint anchor frames
- joint limits
- mass and damping data

Both client and server should load the same metadata.

### Runtime Contract

- Server spawns bodies directly from authored ragdoll metadata.
- Server simulates and streams authoritative body poses.
- Client applies streamed body poses directly to matching bones using authored frame data.
- Runtime should not need root-fit heuristics or translation reconstruction from current animated state.

## Practical Recommendation For This Repo

Treat the current Oilman ragdoll path as a prototype that proved:

- the network model is viable,
- the client can consume and visualize the stream,
- but the current asset/rig pairing is not a reliable production contract.

For the next iteration:

1. Define a ragdoll metadata schema.
2. Add import-time validation for skeleton and metadata compatibility.
3. Re-author Oilman to the contract, or replace it with an asset that satisfies the contract cleanly.
4. Reduce runtime fitting logic instead of adding more.

## Relevant Files

- `client/src/render/systems/npc/ragdoll.rs`
- `client/src/render/systems/npc/debug.rs`
- `client/src/render/systems/npc/spawn.rs`
- `client/src/render/systems/npc/state.rs`
- `client/src/app_wiring/systems.rs`
- `server/src/ai/ragdoll.rs`
- `shared/src/protocol/messages.rs`
- `shared/src/protocol/config.rs`

## Repo State

At the time this handoff was written:

- `cargo check --workspace` passed
- `cargo test -p shared` passed

This means the repo is in a workable state to continue game development while this redesign is deferred.
