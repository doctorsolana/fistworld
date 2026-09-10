//! Restrained meadow colour: green trees remain the majority, accents form copses.
use super::PropKind;

/// Use the scatter's existing species draw; this must never consume more RNG.
/// `patch` is continuous, seed-derived noise at woodland-edge scale. Copper and
/// blossom favour different patches, without straight cell boundaries or extra trees.
pub(super) fn accent_tree(choice: f32, patch: f32) -> Option<PropKind> {
    let beech_share = 0.04 + 0.08 * patch.clamp(0.0, 1.0);
    if choice < 0.14 {
        Some(PropKind::FieldMapleA)
    } else if choice < 0.14 + beech_share {
        Some(PropKind::CopperBeechA)
    } else if choice < 0.30 {
        Some(PropKind::WildCherryA)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_stay_a_minority_and_neighbouring_copses_differ() {
        for patch in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut counts = [0usize; 4];
            for i in 0..10_000 {
                let slot = match accent_tree((i as f32 + 0.5) / 10_000.0, patch) {
                    Some(PropKind::FieldMapleA) => 0,
                    Some(PropKind::CopperBeechA) => 1,
                    Some(PropKind::WildCherryA) => 2,
                    None => 3,
                    other => panic!("unexpected meadow accent: {other:?}"),
                };
                counts[slot] += 1;
            }
            assert_eq!(counts[0], 1400);
            assert_eq!(counts[3], 7000);
            assert!((400..=1200).contains(&counts[1]));
            assert!((400..=1200).contains(&counts[2]));
        }
        assert_ne!(accent_tree(0.22, 0.0), accent_tree(0.22, 1.0));
    }
}
