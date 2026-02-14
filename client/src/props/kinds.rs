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
