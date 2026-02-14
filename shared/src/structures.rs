//! Collider shape primitives shared by server collision systems.

use bevy::prelude::*;

use crate::props::PropKind;

/// Generic structure collider shape.
#[derive(Debug, Clone)]
pub enum StructureCollider {
    Dome {
        radius: f32,
        height: f32,
    },
    Cylinder {
        radius: f32,
        height: f32,
    },
    Box {
        half_extents: Vec3,
    },
    Arch {
        width: f32,
        height: f32,
        depth: f32,
        thickness: f32,
    },
    BakedProp(PropKind),
}
