//! Shared debug-visualization resources.

/// Toggles debug gizmo rendering (prop colliders, trajectories, hitboxes).
#[derive(bevy::prelude::Resource, Default)]
pub struct DebugGizmoMode(pub bool);
