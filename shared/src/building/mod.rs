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

    #[test]
    fn test_building_definitions() {
        let church = BuildingType::Church.definition();
        assert_eq!(church.display_name, "Church");
        let windmill = BuildingType::Windmill.definition();
        assert_eq!(windmill.display_name, "Windmill");
    }

    #[test]
    fn test_flatten_footprint() {
        let def = BuildingType::Windmill.definition();
        let flatten = def.flatten_footprint();
        assert_eq!(flatten.x, 12.0);
        assert_eq!(flatten.y, 12.0);
    }
}
