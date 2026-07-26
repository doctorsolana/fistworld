use bevy::prelude::*;

use crate::props::PropKind;

/// Per-prop render tuning (client uses this to disable shadows / add culling).
#[derive(Component, Clone, Copy, Debug)]
pub struct PropRenderTuning {
    /// If false, meshes under this prop should be marked `NotShadowCaster` (client-side).
    pub casts_shadows: bool,
    /// If set, meshes under this prop should get a `VisibilityRange` (client-side).
    pub visible_end_distance: Option<f32>,
}

/// How a prop reads from the top-down commander camera.
///
/// The camera sits 55–900m from the ground, so "distance to camera" is dominated by
/// zoom rather than by how far the prop is across the map. Anything whose whole purpose
/// was close-up detail is invisible at every playable zoom and is pure cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropVisualRole {
    /// Reads as a silhouette from altitude: trees, rocks, structures.
    Landmark,
    /// Reads only as a colour/texture hint in clumps: bushes, flowers.
    Accent,
    /// Individually invisible from altitude — single grass blades, leaf scatter, ivy.
    /// These belong in the terrain texture, not as entities.
    GroundDetail,
}

pub fn visual_role(kind: PropKind) -> PropVisualRole {
    match kind {
        PropKind::GrassBlade_9v
        | PropKind::Env_Grass_Tall_04
        | PropKind::Env_Grass_06
        | PropKind::Env_Grass_07
        | PropKind::Env_Ivy_08
        | PropKind::Env_Ivy_13
        | PropKind::Env_Leaves_02
        | PropKind::Env_Leaves_03 => PropVisualRole::GroundDetail,

        PropKind::Flower_01
        | PropKind::Flower_02
        | PropKind::Flower_03
        | PropKind::Flower_04
        | PropKind::Flower_05
        | PropKind::Spring_Flower_06
        | PropKind::Spring_Flower_07
        | PropKind::Spring_Flower_08
        | PropKind::Spring_Flower_09
        | PropKind::Bush_01
        | PropKind::Bush_02
        | PropKind::Bush_03
        | PropKind::Bush_04 => PropVisualRole::Accent,

        _ => PropVisualRole::Landmark,
    }
}

/// Draw distances are tuned for the top-down camera.
///
/// The FPS values (grass 55m, bushes 120m) were correct for an eye-level camera and are
/// useless here: the commander camera is *itself* 220m+ away, so those props were culled
/// before they were ever drawn. Ranges now bound in-view instance count relative to how
/// far the camera can actually be.
pub fn default_render_tuning(kind: PropKind) -> PropRenderTuning {
    match visual_role(kind) {
        // Not spawned at all (see the client spawn filter); kept short in case one
        // slips through via an authored map.
        PropVisualRole::GroundDetail => PropRenderTuning {
            casts_shadows: false,
            visible_end_distance: Some(80.0),
        },
        // Clumps of colour: worth drawing at mid zoom, dropped when zoomed way out
        // where they would be sub-pixel anyway.
        PropVisualRole::Accent => PropRenderTuning {
            casts_shadows: false,
            visible_end_distance: Some(420.0),
        },
        // Silhouettes that give the landscape its shape — these must survive to max zoom.
        PropVisualRole::Landmark => PropRenderTuning {
            casts_shadows: true,
            visible_end_distance: Some(1400.0),
        },
    }
}

pub fn default_unmapped_render_tuning() -> PropRenderTuning {
    PropRenderTuning {
        casts_shadows: true,
        visible_end_distance: Some(1400.0),
    }
}
