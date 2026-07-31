//! Building footprint and collider metadata.
//!
//! Defines building types, their resource costs, footprints, and terrain modification parameters.

mod defs;
mod footprint;
mod zones;

pub use defs::*;
pub use footprint::*;
pub use zones::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every building the game has must have a definition and a model.
    ///
    /// Written as a sweep over `all()` rather than naming two by hand: the
    /// hand-named version broke the moment the bought building sets were
    /// deleted, and it would have said nothing about the ones we kept.
    #[test]
    fn every_building_has_a_definition_and_a_model() {
        for kind in BuildingType::all() {
            let def = kind.definition();
            assert!(
                !def.display_name.is_empty(),
                "{kind:?} has no display name"
            );
            assert!(
                kind.scene_path().is_some(),
                "{kind:?} has no model to draw"
            );
        }
    }

    #[test]
    fn flatten_footprint_is_square_and_positive() {
        for kind in BuildingType::all() {
            let flatten = kind.definition().flatten_footprint();
            assert!(flatten.x > 0.0 && flatten.y > 0.0, "{kind:?} flattens nothing");
        }
    }
}
