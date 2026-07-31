//! Prop-kind helpers.

use shared::props::PropKind;

pub(crate) fn is_tree_kind(kind: PropKind) -> bool {
    use shared::props::PropKind::*;
    matches!(
        kind,
        Tree_01
            | Tree_02
            | Tree_08
            | Tree_09
            | Tree_10
            | Tree_18
            | Tree_29
            | Dead_tree_1
            | Dead_tree_2
            | Dead_tree_3
            | Pine_Tree_1
            | Pine_Tree_2
            | Pine_Tree_3
            | Pine_Tree_4
    )
}

/// Kinds whose LOD is a swapped mesh handle on ONE entity, rather than a scene
/// hierarchy with a `VisibilityRange` per child.
///
/// Trees were always this; grass joined because at 278 patches per chunk the
/// three-entities-per-patch scene path put a hard ceiling on how far ground
/// cover could be drawn.
pub(crate) fn uses_swap_mesh_lod(kind: PropKind) -> bool {
    use shared::props::PropKind::*;
    is_tree_kind(kind)
        || matches!(
            kind,
            GrassBlade_9v | Env_Grass_Tall_04 | Env_Grass_06 | Env_Grass_07
        )
}
