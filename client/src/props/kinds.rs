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
            | FieldMapleA
            | CopperBeechA
            | WildCherryA
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
/// cover could be drawn. Ferns and flower patches followed for the same
/// reason: they are common accents whose two-node GLBs would otherwise fall
/// through to a three-entity scene per patch. Must agree with
/// `assets::tree_mesh_labels` (pinned by a test there).
pub(crate) fn uses_swap_mesh_lod(kind: PropKind) -> bool {
    use shared::props::PropKind::*;
    is_tree_kind(kind)
        || matches!(
            kind,
            GrassShortA
                | GrassTallA
                | FernPatchA
                | FernPatchB
                | FlowerA
                | FlowerB
                | FlowerC
                | FlowerD
        )
}
