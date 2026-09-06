//! Small, frame-rate independent UI springs. Transforms never reflow a ledger.
use bevy::{
    prelude::*,
    ui::{UiTransform, Val2},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Spring {
    pub value: f32,
    velocity: f32,
}
impl Spring {
    pub const fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }
    /// Exact solution for an underdamped spring (damping² < 4 × stiffness).
    pub fn step(&mut self, target: f32, dt: f32, stiffness: f32, damping: f32) -> bool {
        if self.value == target && self.velocity == 0.0 {
            return false;
        }
        if !dt.is_finite() || dt <= 0.0 {
            return false;
        }
        debug_assert!(damping > 0.0 && stiffness > damping * damping * 0.25);
        let half = damping * 0.5;
        let frequency = (stiffness - half * half).sqrt();
        let offset = self.value - target;
        let wave = (self.velocity + half * offset) / frequency;
        let (sin, cos) = (frequency * dt).sin_cos();
        let decay = (-half * dt).exp();
        self.value = target + decay * (offset * cos + wave * sin);
        self.velocity = decay * (self.velocity * cos - (half * wave + frequency * offset) * sin);
        if (self.value - target).abs() < 0.005 && self.velocity.abs() < 0.005 {
            self.value = target;
            self.velocity = 0.0;
        }
        true
    }
}

/// Attach to a panel or retained page, never to the full-screen input backdrop.
#[derive(Component)]
#[require(UiTransform)]
pub struct UiReveal {
    spring: Spring,
    visible: bool,
    distance: f32,
}
impl Default for UiReveal {
    fn default() -> Self {
        Self::panel()
    }
}
impl UiReveal {
    pub fn panel() -> Self {
        Self {
            spring: Spring::new(1.0),
            visible: false,
            distance: 24.0,
        }
    }
    pub fn page() -> Self {
        Self {
            distance: 8.0,
            ..Self::panel()
        }
    }
}

pub(super) fn animate_reveals(
    time: Res<Time>,
    mut panels: Query<(&Node, &mut UiReveal, &mut UiTransform)>,
) {
    for (node, mut reveal, mut transform) in &mut panels {
        if node.display == Display::None {
            reveal.visible = false;
            continue;
        }
        if !reveal.visible {
            reveal.spring = Spring::new(1.0);
            reveal.visible = true;
        }
        if reveal.spring.step(0.0, time.delta_secs(), 220.0, 22.0) {
            let next = Val2::px(0.0, reveal.spring.value * reveal.distance);
            if transform.translation != next {
                transform.translation = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spring_matches_across_frame_rates_and_survives_long_frames() {
        let simulate = |dt: f32, frames: usize| {
            let mut s = Spring::new(1.0);
            for _ in 0..frames {
                s.step(0.0, dt, 220.0, 22.0);
            }
            s
        };
        assert!((simulate(1.0 / 30.0, 15).value - simulate(1.0 / 120.0, 60).value).abs() < 0.0001);
        let mut hitch = simulate(0.5, 4);
        assert_eq!(hitch.value, 0.0);
        assert!(!hitch.step(0.0, 0.016, 220.0, 22.0));
    }
    #[test]
    fn reversing_a_spring_keeps_its_current_position() {
        let mut s = Spring::new(1.0);
        s.step(0.0, 0.1, 220.0, 22.0);
        let before = s.value;
        s.step(1.0, 0.001, 220.0, 22.0);
        assert!((s.value - before).abs() < 0.01);
        for _ in 0..180 {
            s.step(1.0, 1.0 / 60.0, 220.0, 22.0);
        }
        assert_eq!(s.value, 1.0);
    }
    #[test]
    fn retained_pages_replay_on_show_and_stop_changing_when_settled() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(1.0 / 60.0));
        app.insert_resource(time)
            .add_systems(Update, animate_reveals);
        let panel = app
            .world_mut()
            .spawn((Node::default(), UiReveal::page()))
            .id();
        app.update();
        assert!(
            matches!(app.world().get::<UiTransform>(panel).unwrap().translation.y, Val::Px(y) if y > 0.0)
        );
        for _ in 0..180 {
            app.update();
        }
        assert_eq!(
            app.world().get::<UiTransform>(panel).unwrap().translation,
            Val2::ZERO
        );
        app.world_mut().clear_trackers();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, Changed<UiTransform>>()
                .iter(app.world())
                .count(),
            0
        );
        app.world_mut().get_mut::<Node>(panel).unwrap().display = Display::None;
        app.update();
        app.world_mut().get_mut::<Node>(panel).unwrap().display = Display::Flex;
        app.update();
        assert!(
            matches!(app.world().get::<UiTransform>(panel).unwrap().translation.y, Val::Px(y) if y > 0.0)
        );
    }
}
