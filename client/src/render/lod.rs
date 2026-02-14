use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct VisibilityRangeBuilder {
    pub start: f32,
    pub end: f32,
    pub fade_distance: f32,
    pub fade_ratio: f32,
    pub use_aabb: bool,
}

impl VisibilityRangeBuilder {
    #[allow(dead_code)]
    pub(crate) fn build(&self) -> Option<VisibilityRange> {
        if self.end <= self.start {
            return None;
        }
        let mut fade = self.fade_distance;
        let max_fade = (self.end - self.start) * self.fade_ratio;
        if fade > max_fade {
            fade = max_fade;
        }
        if fade < 0.01 {
            fade = 0.0;
        }

        Some(VisibilityRange {
            start_margin: self.start..self.start,
            end_margin: (self.end - fade).max(self.start)..self.end,
            use_aabb: self.use_aabb,
        })
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct LodPolicy {
    pub split_ratio: f32,
    pub lod1_start_fallback: f32,
    pub lod1_end_fallback: f32,
    pub fade_distance: f32,
    pub fade_ratio: f32,
    pub use_aabb: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum LodLevel {
    Lod0,
    Lod1,
}

#[derive(Clone, Copy, Debug)]
pub enum LodDebugAction {
    Normal,
    ForceVisible,
    ForceHidden,
}

pub(crate) fn apply_lod_visibility(
    commands: &mut Commands,
    entity: Entity,
    range: Option<VisibilityRange>,
    debug_action: LodDebugAction,
) {
    match debug_action {
        LodDebugAction::ForceHidden => {
            commands.entity(entity).insert(Visibility::Hidden);
            commands.entity(entity).remove::<VisibilityRange>();
        }
        LodDebugAction::ForceVisible => {
            commands.entity(entity).insert(Visibility::Inherited);
            commands.entity(entity).remove::<VisibilityRange>();
        }
        LodDebugAction::Normal => {
            commands.entity(entity).insert(Visibility::Inherited);
            if let Some(range) = range {
                commands.entity(entity).insert(range);
            } else {
                commands.entity(entity).remove::<VisibilityRange>();
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) fn build_lod_visibility_range(
    base_end: Option<f32>,
    max_distance: f32,
    policy: LodPolicy,
    lod_level: Option<LodLevel>,
) -> Option<VisibilityRange> {
    if base_end.is_none() && lod_level.is_none() {
        return None;
    }

    let mut lod_end = base_end.unwrap_or(policy.lod1_end_fallback);
    lod_end = lod_end.min(max_distance);
    if lod_end <= 0.0 {
        return None;
    }

    let lod_split = base_end
        .map(|end| end * policy.split_ratio)
        .unwrap_or(policy.lod1_start_fallback)
        .min(lod_end);

    let (start, end) = match lod_level {
        Some(LodLevel::Lod0) => (0.0, lod_split),
        Some(LodLevel::Lod1) => (lod_split, lod_end),
        None => (0.0, lod_end),
    };

    if end <= start {
        return None;
    }

    let mut fade = policy.fade_distance;
    let max_fade = (end - start) * policy.fade_ratio;
    if fade > max_fade {
        fade = max_fade;
    }
    if fade < 0.01 {
        fade = 0.0;
    }

    let (start_margin, end_margin) = match lod_level {
        Some(LodLevel::Lod1) => (start..(start + fade), (end - fade).max(start + fade)..end),
        Some(LodLevel::Lod0) => (start..start, end..(end + fade)),
        None => (start..start, (end - fade).max(start)..end),
    };

    Some(VisibilityRange {
        start_margin,
        end_margin,
        use_aabb: policy.use_aabb,
    })
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct ShadowCullPolicy {
    pub max_distance: f32,
}

#[allow(dead_code)]
impl ShadowCullPolicy {
    pub(crate) fn max_distance_sq(self) -> f32 {
        self.max_distance * self.max_distance
    }

    pub(crate) fn should_cast(self, distance_sq: f32) -> bool {
        distance_sq <= self.max_distance_sq()
    }
}
