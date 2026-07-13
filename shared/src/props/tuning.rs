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

pub fn default_render_tuning(kind: PropKind) -> PropRenderTuning {
    match kind {
        PropKind::Flower_01
        | PropKind::Flower_02
        | PropKind::Flower_03
        | PropKind::Flower_04
        | PropKind::Flower_05
        | PropKind::Spring_Flower_06
        | PropKind::Spring_Flower_07
        | PropKind::Spring_Flower_08
        | PropKind::Spring_Flower_09
        | PropKind::Env_Leaves_02
        | PropKind::Env_Leaves_03
        | PropKind::Bush_01
        | PropKind::Bush_02
        | PropKind::Bush_03
        | PropKind::Bush_04 => PropRenderTuning {
            casts_shadows: false,
            visible_end_distance: Some(120.0),
        },
        // Ground cover: placed VERY densely (BotW-style fields), so the draw
        // distance is short — it bounds the in-view instance count.
        PropKind::GrassBlade_9v
        | PropKind::Env_Grass_Tall_04
        | PropKind::Env_Grass_06
        | PropKind::Env_Grass_07 => PropRenderTuning {
            casts_shadows: false,
            visible_end_distance: Some(55.0),
        },
        PropKind::Env_Ivy_08 | PropKind::Env_Ivy_13 => PropRenderTuning {
            casts_shadows: false,
            visible_end_distance: Some(100.0),
        },
        _ => PropRenderTuning {
            casts_shadows: true,
            visible_end_distance: Some(450.0),
        },
    }
}

pub fn default_unmapped_render_tuning() -> PropRenderTuning {
    PropRenderTuning {
        casts_shadows: true,
        visible_end_distance: Some(450.0),
    }
}
