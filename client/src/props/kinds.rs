//! Prop-kind helpers.

use shared::props::PropKind;

pub(crate) fn is_tree_kind(kind: PropKind) -> bool {
    use shared::props::PropKind::*;
    matches!(
        kind,
        BroadleafNarrowA
            | OakA
            | BroadleafLargeA
            | BroadleafSpreadingA
            | BirchA
            | BirchB
            | ChestnutA
            | BroadleafHighCrownA
            | BroadleafTallA
            | DeadTreeA
            | DeadTreeB
            | DeadTreeC
            | DeadGnarledA
            | PineA
            | PineB
            | PineTallA
            | PineTallB
            | PineYoungA
            | PineYoungB
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
    is_tree_kind(kind) || matches!(kind, GrassShortA | GrassTallA)
}
