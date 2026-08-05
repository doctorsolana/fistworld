//! Pure production-rate rules shared by tactical and strategic workers.

use super::PERFECT_FIELD_SECONDS_PER_WHEAT;

/// Actual field labour required for one inventory unit. Progress is retained
/// between shifts, so short working days reduce current output without erasing
/// work or requiring a special per-day cap. Almost barren fields can still
/// yield eventually, but are proportionally unattractive places to build.
pub(crate) fn farmer_seconds_per_wheat(quality: f32) -> f32 {
    let quality = if quality.is_finite() {
        quality.clamp(0.01, 1.0)
    } else {
        0.01
    };
    PERFECT_FIELD_SECONDS_PER_WHEAT / quality
}

/// Pier quality uses the same readable scale as farmland: ideal water can
/// approach six catches per ordinary shift, while a two-thirds-quality shore
/// approaches four. Travel along the pier is real labour overhead rather than
/// being hidden inside a daily output table.
pub(crate) fn fisher_seconds_per_food(quality: f32) -> f32 {
    farmer_seconds_per_wheat(quality)
}

/// A completed interaction with a real tree creates one physical carried
/// bundle, whose size reflects the timber density sampled where the hut was
/// built. There is deliberately no daily ceiling.
pub(crate) fn lumber_tree_yield(quality: f32) -> u32 {
    if quality < 0.34 {
        1
    } else if quality < 0.67 {
        2
    } else {
        3
    }
}
