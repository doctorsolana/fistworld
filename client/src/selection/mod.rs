//! Selection: left click to select, right click to order.
//!
//! The RTS input contract, and the first thing in this repo that can pick a
//! world ENTITY rather than a point on the heightfield.
//!
//! Deliberately its own module rather than more branches inside
//! `hero::control`: selection is about to cover retinues, caravans and
//! settlements, and none of that belongs in a file named after the hero. What
//! is selectable is expressed by [`Selectable`], so new kinds opt in by
//! spawning a component instead of by editing the picker.

pub mod order;
pub mod pick;
pub mod ring;

use bevy::prelude::*;

use crate::states::GameState;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>();
        app.init_resource::<RightDrag>();
        app.init_resource::<DragBox>();
        app.add_systems(
            Update,
            (
                // Order matters: drop a dead selection before anything reads it,
                // then pick, then let the ring follow what is now selected.
                tag_characters_selectable,
                tag_settlements_selectable,
                clear_stale_selection,
                pick::pick_on_left_click,
                order::issue_order_on_right_click,
                ring::sync_selection_ring,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), clear_on_exit);
    }
}

/// Anything the player can click on.
///
/// `radius` is the world-space pick radius around the entity's vertical axis;
/// `height` is how tall it is above its origin. The hero's origin is at its
/// feet, so a hero is `height: 1.7`.
#[derive(Component, Debug, Clone, Copy)]
pub struct Selectable {
    pub radius: f32,
    pub height: f32,
}

impl Selectable {
    /// A building-sized target: a hall is clicked at its walls, not its axis.
    pub fn hall() -> Self {
        Self {
            radius: 4.5,
            height: 5.0,
        }
    }

    /// A person-sized target.
    pub fn person() -> Self {
        Self {
            radius: 0.55,
            height: 1.7,
        }
    }
}

/// What the player currently has selected.
///
/// A Resource holding an `Entity`, not a marker Component, for one specific
/// reason: the selected entity is REPLICATED and the server can despawn it at
/// any time (the hero leaves interest range, or its owner is disconnected and
/// it is culled). A marker would vanish silently with the entity and leave the
/// HUD describing something that no longer exists; a resource holding a
/// possibly-dead `Entity` can be validated in one place, which is exactly what
/// [`clear_stale_selection`] does.
#[derive(Resource, Debug, Default)]
pub struct Selection {
    /// Order is preserved rather than using a set, so the "primary" selection --
    /// the one a single-name UI shows -- is stable instead of hash-ordered.
    pub entities: Vec<Entity>,
}

impl Selection {
    pub fn is_selected(&self, entity: Entity) -> bool {
        self.entities.contains(&entity)
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// The one to name when the UI has room for a single name.
    pub fn primary(&self) -> Option<Entity> {
        self.entities.first().copied()
    }

    pub fn clear(&mut self) {
        if !self.entities.is_empty() {
            self.entities.clear();
        }
    }

    /// Replace the selection, but only when it actually differs: this is a
    /// change-detected resource and several systems rebuild when it changes.
    pub fn set(&mut self, entities: Vec<Entity>) {
        if self.entities != entities {
            self.entities = entities;
        }
    }
}

/// Tracks whether the current right-button press has become a camera drag.
///
/// Right button does double duty: HELD it orbits the camera, TAPPED it issues a
/// move order. Without this the two are indistinguishable and every orbit would
/// fling the hero at wherever the cursor happened to stop. This repo has been
/// bitten by right-click ambiguity before -- see the comment in
/// `hero::control::handle_world_clicks` about why Escape, not right-click,
/// cancels an armed placement.
#[derive(Resource, Debug, Default)]
pub struct RightDrag {
    /// Cursor position when the button went down, in logical window pixels.
    pub press_at: Option<Vec2>,
    /// Accumulated RAW DEVICE motion since the press.
    ///
    /// Tracked separately from the radial distance below because the two catch
    /// different gestures and are in different units -- see [`DRAG_MOTION_PX`].
    pub motion: f32,
    /// Seconds the button has been held.
    pub held_secs: f32,
    /// Set once this press has been disqualified from counting as a click.
    pub became_drag: bool,
}

/// An in-progress left-drag selection box, in WINDOW pixels.
///
/// Only becomes a box once the cursor has travelled past [`BOX_MIN_PX`]; below
/// that the gesture is a plain click, so a slightly shaky single-unit click does
/// not turn into a one-pixel marquee that selects nothing.
#[derive(Resource, Debug, Default)]
pub struct DragBox {
    pub start: Option<Vec2>,
    pub current: Vec2,
    pub active: bool,
}

impl DragBox {
    /// Min/max corners, or `None` when there is no active box.
    pub fn rect(&self) -> Option<(Vec2, Vec2)> {
        let start = self.start?;
        if !self.active {
            return None;
        }
        Some((start.min(self.current), start.max(self.current)))
    }
}

/// Cursor travel before a left-press becomes a selection box.
pub const BOX_MIN_PX: f32 = 5.0;

/// Radial cursor displacement from the press point that means "orbit", in
/// logical window pixels.
///
/// RADIAL, not path length: summing per-frame travel turns a slow shaky click
/// into a drag, because +/-1px of jitter over several frames adds up while the
/// cursor has not actually gone anywhere. Distance from where the press started
/// is what the player perceives as having moved the mouse.
pub const DRAG_RADIAL_PX: f32 = 6.0;

/// Accumulated raw device motion that means "orbit".
///
/// Needed IN ADDITION to the radial test because the cursor is never grabbed in
/// RTS mode: an orbit drag that runs into the edge of the window stops moving
/// the cursor while MouseMotion keeps streaming and the camera keeps yawing.
/// Radial displacement alone would call that a click and fire an order at the
/// end of every edge-of-screen orbit.
///
/// This is in raw device pixels, which on a Retina display are roughly half a
/// logical pixel, so the threshold is deliberately larger than the radial one.
pub const DRAG_MOTION_PX: f32 = 14.0;

/// Seconds after which a right-press is an orbit regardless of movement.
///
/// Covers press-hold-think-release with a perfectly steady hand, which would
/// otherwise land as an order the player forgot they were queuing.
pub const DRAG_HOLD_SECS: f32 = 0.35;

/// Whether a right-press that moved this much should still count as a click.
pub fn is_click(radial_px: f32, motion_px: f32, held_secs: f32) -> bool {
    radial_px <= DRAG_RADIAL_PX && motion_px <= DRAG_MOTION_PX && held_secs <= DRAG_HOLD_SECS
}

/// Forget a selection whose entity can no longer be shown.
///
/// Replication can despawn the hero underneath us at any time.
///
/// Validity is "still has a world position", NOT "still has [`Selectable`]".
/// That distinction is load-bearing: `tag_heroes_selectable` inserts through
/// deferred `Commands`, so on the frame a hero appears it does not yet carry
/// `Selectable` even though this system has already run. Testing for the tag
/// would clear any selection made in the same frame the entity arrived -- which
/// is exactly what silently broke the capture harness's pre-seeded selection.
/// `PlayerPosition` is also the honest condition, because it is what the ring
/// and the HUD plate actually need in order to draw anything.
fn clear_stale_selection(
    mut selection: ResMut<Selection>,
    positioned: Query<(), With<shared::components::PlayerPosition>>,
) {
    if selection.entities.is_empty() {
        return;
    }
    // Bypass change detection in the common no-op case: this runs every frame,
    // and touching the resource unconditionally would make every consumer of the
    // selection rebuild every frame.
    if selection
        .entities
        .iter()
        .all(|entity| positioned.get(*entity).is_ok())
    {
        return;
    }
    selection
        .bypass_change_detection()
        .entities
        .retain(|entity| positioned.get(*entity).is_ok());
    selection.set_changed();
}

/// Make every replicated PERSON clickable.
///
/// Gated on `CharacterKind`, not `Hero`. `Hero` means "a player's body", so
/// keying on it made villagers unclickable entirely -- you could not even
/// inspect one, which contradicted this module's own documentation. Selectable
/// is about being a thing in the world you can point at; whether you may command
/// it is a separate question answered by [`is_owned_by`].
///
/// The hero creator's preview rig is still excluded, because it carries no
/// `PlayerPosition` -- it is a parked model in front of the camera, not someone
/// standing somewhere. That requirement, not the `Hero` filter, is what was
/// really keeping it out of the picker.
///
/// Polls `Without<Selectable>` rather than reacting to `Added<..>` because
/// replication delivers a character's components in separate batches, and a
/// one-shot on `Added` would miss whoever's position arrived on a later tick.
/// Halls are clickable, so a settlement can be inspected by pointing at it.
///
/// Separate from the character tagger because a settlement is not a person and
/// wants a different hit shape -- and because command never applies to it: a
/// place cannot be ordered anywhere.
fn tag_settlements_selectable(
    mut commands: Commands,
    settlements: Query<
        Entity,
        (
            With<shared::components::Settlement>,
            With<shared::components::PlayerPosition>,
            Without<Selectable>,
        ),
    >,
) {
    for entity in settlements.iter() {
        commands.entity(entity).insert(Selectable::hall());
    }
}

fn tag_characters_selectable(
    mut commands: Commands,
    characters: Query<
        Entity,
        (
            With<shared::components::CharacterKind>,
            With<shared::components::PlayerPosition>,
            Without<Selectable>,
        ),
    >,
) {
    for entity in characters.iter() {
        commands.entity(entity).insert(Selectable::person());
    }
}

fn clear_on_exit(
    mut selection: ResMut<Selection>,
    mut drag: ResMut<RightDrag>,
    mut drag_box: ResMut<DragBox>,
) {
    selection.clear();
    *drag = RightDrag::default();
    *drag_box = DragBox::default();
}

/// Closest approach between a ray and a vertical segment, as
/// `(distance_along_ray, gap)`.
///
/// Used to hit-test a standing character: the character is the segment from its
/// feet to its head, and the click hits if `gap` is within its pick radius.
/// Returns `None` when the ray points away from the segment.
///
/// This is a world-space test on purpose. The screen-space alternative
/// (projecting the entity with `world_to_viewport` and comparing pixels) has to
/// undo this app's render-target scaling by hand, and getting that subtly wrong
/// produces a picking offset that only appears at non-1.0 render scale.
pub fn ray_vs_vertical_segment(
    ray_origin: Vec3,
    ray_dir: Vec3,
    base: Vec3,
    height: f32,
) -> Option<(f32, f32)> {
    let seg_dir = Vec3::Y;
    let w0 = ray_origin - base;

    let a = ray_dir.dot(ray_dir);
    let b = ray_dir.dot(seg_dir);
    let c = seg_dir.dot(seg_dir);
    let d = ray_dir.dot(w0);
    let e = seg_dir.dot(w0);

    let denom = a * c - b * b;
    // Where along the BODY the ray passes closest. Parallel rays fall back to
    // the feet. `d` is unused in that branch, hence the explicit discard.
    let t_seg = if denom.abs() < 1e-6 {
        let _ = d;
        0.0
    } else {
        (a * e - b * d) / denom
    }
    // Clamp to the real extents: the body stops at the head and the feet.
    .clamp(0.0, height);

    // Solve the ray parameter against that clamped point, so a click at the very
    // top or bottom of the body still measures the true gap rather than the gap
    // to an imaginary infinite pole.
    let t_ray = (seg_dir * t_seg + base - ray_origin).dot(ray_dir) / a.max(1e-6);
    if t_ray < 0.0 {
        return None;
    }

    let on_ray = ray_origin + ray_dir * t_ray;
    let on_seg = base + seg_dir * t_seg;
    Some((t_ray, on_ray.distance(on_seg)))
}

/// Extra pick radius per metre of distance, so distant units stay clickable.
///
/// A 0.55m-wide hero is roughly two pixels across at 1km zoom; without this,
/// selecting anything above a few hundred metres would be pixel hunting. The
/// coefficient is chosen so the target stays about a dozen pixels wide at
/// 1080p with the default 45-degree vertical FOV:
/// `2 * tan(fov/2) / viewport_height * pixels`.
pub const PICK_ANGULAR_SLOP: f32 = 0.011;

/// Pick radius for a target `distance` metres away.
pub fn pick_radius_at(base_radius: f32, distance: f32) -> f32 {
    base_radius.max(distance * PICK_ANGULAR_SLOP)
}

/// Whether YOU may order this unit.
///
/// The one place command is decided client-side, because it is asked in three
/// contexts -- can I box-select this, does the ring glow, what does the HUD say
/// -- and three copies of the same test is three chances for them to disagree.
///
/// Command follows [`CommandedBy`], NOT the banner. A banner is affiliation, and
/// clans are joinable by several players, so banner-as-command would hand your
/// units to anyone who joined your clan. It would also break outright at ROADMAP
/// Phase 8 when the placeholder banner becomes a real ClanId.
///
/// This is a DISPLAY predicate. The server runs the same check itself before
/// moving anything; a client that lies to itself here only lies to itself.
///
/// [`CommandedBy`]: shared::components::CommandedBy
pub fn can_command(
    commanded: Option<&shared::components::CommandedBy>,
    my_account: Option<&str>,
) -> bool {
    match (commanded, my_account) {
        (Some(commanded), Some(account)) => commanded.0 == account,
        _ => false,
    }
}

/// Spread `count` move targets around `centre` so a group ordered to one point
/// arrives as a group instead of stacking into one body.
///
/// Concentric rings rather than a grid, because a ring reads as a gathering and
/// keeps every unit roughly equidistant from the click -- a grid puts its back
/// row noticeably further away and looks like a queue. `spacing` is the gap
/// between neighbours.
///
/// This is formation as ARRIVAL SPREAD, not as a maintained formation: there is
/// no unit collision yet, so this only stops the pile-up. Real formations belong
/// with flow fields (ROADMAP Phase 6).
pub fn formation_targets(centre: Vec3, count: usize, spacing: f32) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(count);
    if count == 0 {
        return out;
    }
    out.push(centre);
    let mut placed = 1;
    let mut ring = 1;
    while placed < count {
        let radius = ring as f32 * spacing;
        // Circumference divided by spacing, so rings do not crowd as they grow.
        let capacity = ((std::f32::consts::TAU * radius) / spacing).floor().max(1.0) as usize;
        for i in 0..capacity {
            if placed >= count {
                break;
            }
            let angle = std::f32::consts::TAU * i as f32 / capacity as f32;
            out.push(centre + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius));
            placed += 1;
        }
        ring += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the discriminator: an orbit must never issue an order,
    /// and a tap must never fail to.
    /// A group ordered to one point must not stack into one body, and must not
    /// scatter so far that the order stops reading as "go there".
    #[test]
    fn formation_spreads_without_scattering() {
        let centre = Vec3::new(10.0, 0.0, -4.0);
        let spacing = 1.2;
        for count in [1usize, 2, 5, 12, 40] {
            let targets = formation_targets(centre, count, spacing);
            assert_eq!(targets.len(), count, "wrong target count for {count}");

            // Nobody shares a spot.
            for (i, a) in targets.iter().enumerate() {
                for b in targets.iter().skip(i + 1) {
                    assert!(
                        a.distance(*b) > spacing * 0.5,
                        "targets {a:?} and {b:?} are stacked for count {count}"
                    );
                }
            }
            // The whole group stays near where the player actually clicked.
            for target in &targets {
                assert!(
                    target.distance(centre) < spacing * count as f32,
                    "target {target:?} scattered too far for count {count}"
                );
                assert_eq!(target.y, centre.y, "formation must stay in the ground plane");
            }
        }
    }

    /// Command gating decides whether you can move a thing, so getting it wrong
    /// either hands you someone else's units or takes away your own.
    #[test]
    fn only_your_own_retinue_is_commandable() {
        use shared::components::CommandedBy;

        let mine = CommandedBy("aldric".to_string());
        let theirs = CommandedBy("bryn".to_string());

        assert!(can_command(Some(&mine), Some("aldric")));
        assert!(!can_command(Some(&theirs), Some("aldric")), "took someone else's unit");
        // Nobody's unit: a villager not in any retinue.
        assert!(!can_command(None, Some("aldric")), "claimed an unconscripted villager");
        // Before the account is known, nothing is commandable.
        assert!(!can_command(Some(&mine), None), "claimed a unit with no account");
        // Account keys are lowercase on both sides; a case mismatch must NOT
        // silently grant command.
        assert!(!can_command(Some(&mine), Some("Aldric")), "case-insensitive match");
    }

    #[test]
    fn a_single_unit_is_ordered_exactly_where_clicked() {
        let centre = Vec3::new(3.0, 1.0, 9.0);
        assert_eq!(formation_targets(centre, 1, 1.2), vec![centre]);
    }

    #[test]
    fn taps_are_clicks_and_drags_are_not() {
        // A clean tap.
        assert!(is_click(0.0, 0.0, 0.02));
        // Hand tremor on a tap must survive: a couple of pixels of wobble and a
        // little accumulated device motion is still a click.
        assert!(is_click(2.0, 5.0, 0.10), "a shaky tap was rejected");
        // A deliberate orbit.
        assert!(!is_click(120.0, 300.0, 0.6), "an orbit was treated as a click");
        // Cursor pinned at the window edge: radial displacement stops growing
        // while device motion keeps streaming. This is the case radial-only
        // discrimination gets wrong.
        assert!(
            !is_click(3.0, 400.0, 0.2),
            "an edge-of-screen orbit was treated as a click"
        );
        // Press-and-hold with a perfectly steady hand is not a click either.
        assert!(!is_click(0.0, 0.0, 1.5), "a long hold was treated as a click");
    }

    #[test]
    fn ray_through_a_body_hits_it() {
        // Looking down -Z at a body standing at the origin.
        let hit = ray_vs_vertical_segment(
            Vec3::new(0.0, 1.0, 10.0),
            Vec3::NEG_Z,
            Vec3::ZERO,
            1.7,
        );
        let (distance, gap) = hit.expect("ray should reach the body");
        assert!((distance - 10.0).abs() < 1e-3, "distance {distance}");
        assert!(gap < 1e-3, "gap {gap} should be ~0 through the centre");
    }

    #[test]
    fn ray_beside_a_body_measures_the_gap() {
        let (_, gap) = ray_vs_vertical_segment(
            Vec3::new(2.0, 1.0, 10.0),
            Vec3::NEG_Z,
            Vec3::ZERO,
            1.7,
        )
        .expect("still in front");
        assert!((gap - 2.0).abs() < 1e-3, "gap {gap} should be the 2m offset");
    }

    /// A click above the head must not hit. Without clamping the segment to
        /// `height` the infinite-line solution would report a hit for a ray
    /// passing well over the character.
    #[test]
    fn ray_over_the_head_misses() {
        let (_, gap) = ray_vs_vertical_segment(
            Vec3::new(0.0, 6.0, 10.0),
            Vec3::NEG_Z,
            Vec3::ZERO,
            1.7,
        )
        .expect("in front, just high");
        assert!(gap > 4.0, "gap {gap} should be the height shortfall");
    }

    #[test]
    fn ray_pointing_away_misses_entirely() {
        assert!(
            ray_vs_vertical_segment(Vec3::new(0.0, 1.0, 10.0), Vec3::Z, Vec3::ZERO, 1.7).is_none(),
            "a ray pointing away from the body must not hit it"
        );
    }

    /// Distant targets must stay clickable, near ones must not become giant
    /// invisible hitboxes.
    #[test]
    fn pick_radius_grows_with_distance_but_never_shrinks() {
        let base = 0.55;
        assert_eq!(pick_radius_at(base, 10.0), base, "near target lost its radius");
        let far = pick_radius_at(base, 1000.0);
        assert!(far > base * 10.0, "distant target radius {far} is too tight");
    }
}
