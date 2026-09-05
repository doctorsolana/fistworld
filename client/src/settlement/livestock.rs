//! Pasture scenes and deterministic local sheep flock presentation.

use bevy::prelude::*;
use shared::components::{
    LivestockPasture, PlayerPosition, PlayerRotation, SettlementBuildingKind, TimeWarp,
};
use shared::terrain::WorldTerrain;

#[derive(Component)]
pub struct LivestockPastureVisual;

/// One pasture sheep, simulated locally: a flock member that grazes, ambles
/// to a fresh patch, keeps loosely with the others and never overlaps a
/// neighbour or the fence. Deterministic per (pasture, index), no bandwidth.
#[derive(Component)]
pub(super) struct PastureAnimal {
    pub(super) rng: u64,
    /// Pasture-local XZ (fence-centred).
    pub(super) position: Vec2,
    pub(super) yaw: f32,
    pub(super) speed: f32,
    pub(super) state: SheepState,
    /// Metres walked; drives the leg gait and body bob.
    pub(super) gait: f32,
    /// 0 = head up, 1 = muzzle in the grass.
    pub(super) graze: f32,
    /// Fence half extents this sheep is penned by.
    pub(super) half: Vec2,
}

#[derive(Clone, Copy)]
pub(super) enum SheepState {
    /// Head down, standing. `remaining` seconds until it looks for a new patch.
    Graze { remaining: f32 },
    /// Head up, standing, looking about.
    Look { remaining: f32 },
    /// Ambling toward a spot; `give_up` bounds a walk a neighbour keeps blocking.
    Walk { target: Vec2, give_up: f32 },
}

/// A named node of the sheep art the client animates itself (no clips).
#[derive(Component)]
pub(super) struct SheepPart {
    pub(super) sheep: Entity,
    pub(super) kind: SheepPartKind,
    pub(super) rest: Quat,
}

#[derive(Clone, Copy)]
pub(super) enum SheepPartKind {
    Head,
    LegFrontLeft,
    LegFrontRight,
    LegBackLeft,
    LegBackRight,
}

pub(super) const SHEEP_SCENE: &str = "game_assets/environment/animals/Sheep.glb#Scene0";

pub(super) const SHEEP_PER_PASTURE: usize = 6;

/// Sheep amble; anything faster reads as fleeing.
pub(super) const SHEEP_WALK_SPEED: f32 = 0.55;

pub(super) const SHEEP_TURN_RATE: f32 = 2.2;

/// Keep bodies off the rails.
pub(super) const SHEEP_FENCE_MARGIN: f32 = 1.1;

/// Centre-to-centre spacing below which neighbours push apart.
pub(super) const SHEEP_SEPARATION: f32 = 1.5;

/// Farther than this from the flock's centre, the next walk heads back.
pub(super) const SHEEP_FLOCK_REACH: f32 = 3.5;

/// Draw a fenced pasture and its flock.
///
/// Only the pasture itself is replicated. The sheep are the shipped `Sheep.glb`
/// (six named parts) simulated entirely on the client: they do not navigate,
/// collide with the world, think, or cost bandwidth, so a town with many farms
/// stays cheap while every pasture still reads as a living industry.
pub(super) fn attach_livestock_pasture_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>)>>,
    pastures: Query<
        (Entity, &LivestockPasture, &PlayerPosition, &PlayerRotation),
        Without<LivestockPastureVisual>,
    >,
) {
    let (post_mesh, rail_mesh, wood) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.16, 1.25, 0.16)),
                meshes.add(Cuboid::new(1.0, 0.12, 0.12)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.31, 0.19, 0.09),
                    perceptual_roughness: 0.96,
                    ..default()
                }),
            )
        })
        .clone();

    for (entity, pasture, position, rotation) in pastures.iter() {
        commands.entity(entity).insert((
            LivestockPastureVisual,
            Name::new(format!("Livestock pasture ({})", pasture.settlement)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
        commands.entity(entity).with_children(|parent| {
            let half = SettlementBuildingKind::LivestockFarm
                .pasture_half_extents()
                .unwrap_or(Vec2::new(8.0, 7.0));
            for x in [-half.x, half.x] {
                for step in 0..=7 {
                    let z = -half.y + half.y * 2.0 * step as f32 / 7.0;
                    parent.spawn((
                        Mesh3d(post_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, 0.62, z),
                    ));
                }
            }
            for z in [-half.y, half.y] {
                for step in 0..=8 {
                    // Leave a modest gate in the front fence for the herders.
                    if z < 0.0 && (3..=5).contains(&step) {
                        continue;
                    }
                    let x = -half.x + half.x * 2.0 * step as f32 / 8.0;
                    parent.spawn((
                        Mesh3d(post_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, 0.62, z),
                    ));
                }
            }
            for (length, x, z, yaw) in [
                (half.y * 2.0, -half.x, 0.0, std::f32::consts::FRAC_PI_2),
                (half.y * 2.0, half.x, 0.0, std::f32::consts::FRAC_PI_2),
                (half.x * 2.0, 0.0, half.y, 0.0),
                (half.x - 2.0, -(half.x + 2.0) * 0.5, -half.y, 0.0),
                (half.x - 2.0, (half.x + 2.0) * 0.5, -half.y, 0.0),
            ] {
                for height in [0.42, 0.92] {
                    parent.spawn((
                        Mesh3d(rail_mesh.clone()),
                        MeshMaterial3d(wood.clone()),
                        Transform::from_xyz(x, height, z)
                            .with_rotation(Quat::from_rotation_y(yaw))
                            .with_scale(Vec3::new(length, 1.0, 1.0)),
                    ));
                }
            }
            // The flock starts scattered over the middle of the field, each
            // sheep on its own deterministic dice so two pastures never move
            // in lockstep.
            let inner = half - Vec2::splat(SHEEP_FENCE_MARGIN);
            for index in 0..SHEEP_PER_PASTURE {
                let mut rng = shared::worldgen::splitmix64(
                    entity
                        .to_bits()
                        .wrapping_add((index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                );
                let position = Vec2::new(
                    (shared::worldgen::rand01(&mut rng) * 2.0 - 1.0) * inner.x * 0.6,
                    (shared::worldgen::rand01(&mut rng) * 2.0 - 1.0) * inner.y * 0.6,
                );
                let yaw = shared::worldgen::rand01(&mut rng) * std::f32::consts::TAU;
                let remaining = 1.0 + shared::worldgen::rand01(&mut rng) * 6.0;
                parent.spawn((
                    Name::new(format!("Pasture sheep {}", index + 1)),
                    PastureAnimal {
                        rng,
                        position,
                        yaw,
                        speed: 0.0,
                        state: SheepState::Graze { remaining },
                        gait: 0.0,
                        graze: 1.0,
                        half,
                    },
                    Transform::from_xyz(position.x, 0.0, position.y)
                        .with_rotation(Quat::from_rotation_y(yaw)),
                    Visibility::Inherited,
                    WorldAssetRoot(asset_server.load(SHEEP_SCENE)),
                ));
            }
        });
    }
}

/// Pasture-local forward vector for a yaw: the art faces -Z at identity.
pub(super) fn sheep_forward(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}

/// The yaw whose forward points along `direction` (see [`sheep_forward`]).
pub(super) fn sheep_yaw_toward(direction: Vec2) -> f32 {
    f32::atan2(-direction.x, -direction.y)
}

/// Graze, look about, amble to a fresh patch; drift back toward the flock when
/// straying; step aside from a neighbour; never touch the fence.
///
/// Runs in pasture-local space and follows the ground under each sheep, so a
/// pasture on a slope keeps its hooves on the grass instead of one side
/// floating and the other sunk.
pub(super) fn animate_pasture_animals(
    time: Res<Time>,
    warp: Query<&TimeWarp>,
    terrain: Option<Res<WorldTerrain>>,
    pastures: Query<(&Transform, &Children), With<LivestockPastureVisual>>,
    mut animals: Query<(&mut PastureAnimal, &mut Transform), Without<LivestockPastureVisual>>,
) {
    let speed_scale = warp.iter().next().map_or(1.0, |warp| warp.0.max(0.0));
    let dt = (time.delta_secs() * speed_scale).min(0.25);
    if dt <= 0.0 {
        return;
    }
    let mut flock: Vec<(Entity, Vec2)> = Vec::with_capacity(SHEEP_PER_PASTURE);
    for (pasture_transform, children) in pastures.iter() {
        flock.clear();
        flock.extend(children.iter().filter_map(|child| {
            animals
                .get(child)
                .ok()
                .map(|(animal, _)| (child, animal.position))
        }));
        if flock.is_empty() {
            continue;
        }
        let centroid = flock.iter().map(|(_, p)| *p).sum::<Vec2>() / flock.len() as f32;

        for index in 0..flock.len() {
            let (entity, _) = flock[index];
            let Ok((mut animal, mut transform)) = animals.get_mut(entity) else {
                continue;
            };
            let inner = animal.half - Vec2::splat(SHEEP_FENCE_MARGIN);
            let mut rng = animal.rng;
            let mut roll = || shared::worldgen::rand01(&mut rng);

            // Behaviour.
            let mut target_speed = 0.0;
            let mut target_graze = 1.0;
            animal.state = match animal.state {
                SheepState::Graze { remaining } => {
                    let remaining = remaining - dt;
                    if remaining > 0.0 {
                        SheepState::Graze { remaining }
                    } else if roll() < 0.22 {
                        target_graze = 0.0;
                        SheepState::Look {
                            remaining: 1.2 + roll() * 2.5,
                        }
                    } else {
                        target_graze = 0.0;
                        SheepState::Walk {
                            target: pick_sheep_target(animal.position, centroid, inner, &mut roll),
                            give_up: 12.0,
                        }
                    }
                }
                SheepState::Look { remaining } => {
                    target_graze = 0.0;
                    let remaining = remaining - dt;
                    if remaining > 0.0 {
                        SheepState::Look { remaining }
                    } else if roll() < 0.5 {
                        SheepState::Graze {
                            remaining: 3.0 + roll() * 8.0,
                        }
                    } else {
                        SheepState::Walk {
                            target: pick_sheep_target(animal.position, centroid, inner, &mut roll),
                            give_up: 12.0,
                        }
                    }
                }
                SheepState::Walk { target, give_up } => {
                    target_graze = 0.0;
                    let to_target = target - animal.position;
                    let give_up = give_up - dt;
                    if to_target.length() < 0.3 || give_up <= 0.0 {
                        SheepState::Graze {
                            remaining: 4.0 + roll() * 10.0,
                        }
                    } else {
                        target_speed = SHEEP_WALK_SPEED;
                        // Turn toward the patch, then walk; a sheep does not
                        // pivot on the spot at full stride.
                        let wanted = sheep_yaw_toward(to_target);
                        let diff = (wanted - animal.yaw + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        let step = diff.clamp(-SHEEP_TURN_RATE * dt, SHEEP_TURN_RATE * dt);
                        animal.yaw += step;
                        if diff.abs() > 1.0 {
                            target_speed *= 0.35;
                        }
                        SheepState::Walk { target, give_up }
                    }
                }
            };
            animal.rng = rng;

            // Motion: ease speed, advance, keep apart, stay off the fence.
            animal.speed += (target_speed - animal.speed) * (dt * 3.0).min(1.0);
            let advance = sheep_forward(animal.yaw) * animal.speed * dt;
            let mut next = animal.position + advance;
            for (other, other_pos) in flock.iter() {
                if *other == entity {
                    continue;
                }
                let away = next - *other_pos;
                let distance = away.length();
                if distance < SHEEP_SEPARATION && distance > 1.0e-3 {
                    next += away / distance * (SHEEP_SEPARATION - distance) * (dt * 4.0).min(1.0);
                }
            }
            next = next.clamp(-inner, inner);
            animal.gait += (next - animal.position).length();
            animal.position = next;
            animal.graze += (target_graze - animal.graze) * (dt * 2.0).min(1.0);

            // Pose: hooves on the real ground under this sheep.
            let world = pasture_transform.transform_point(Vec3::new(next.x, 0.0, next.y));
            let ground = terrain
                .as_deref()
                .map_or(pasture_transform.translation.y, |terrain| {
                    terrain.get_height(world.x, world.z)
                });
            let local_y = (ground - pasture_transform.translation.y).clamp(-1.5, 1.5);
            let bob = (animal.gait * 6.0).sin().abs() * 0.02 * (animal.speed / SHEEP_WALK_SPEED);
            transform.translation = Vec3::new(next.x, local_y + bob, next.y);
            transform.rotation = Quat::from_rotation_y(animal.yaw);
        }
    }
}

/// A fresh patch to amble to: a short hop in a random direction, pulled back
/// toward the flock when this sheep has strayed, clamped inside the fence.
pub(super) fn pick_sheep_target(
    position: Vec2,
    centroid: Vec2,
    inner: Vec2,
    roll: &mut impl FnMut() -> f32,
) -> Vec2 {
    let to_flock = centroid - position;
    let base = if to_flock.length() > SHEEP_FLOCK_REACH {
        position + to_flock * 0.6
    } else {
        position
    };
    let angle = roll() * std::f32::consts::TAU;
    let distance = 1.2 + roll() * 2.8;
    (base + Vec2::from_angle(angle) * distance).clamp(-inner, inner)
}

/// Discover the sheep art's named nodes as each scene instantiates, so the
/// head and legs can be posed without a single glTF clip.
pub(super) fn tag_pasture_sheep_parts(
    mut commands: Commands,
    named: Query<(Entity, &Name, &Transform), Added<Name>>,
    parents: Query<&ChildOf>,
    animals: Query<(), With<PastureAnimal>>,
) {
    for (entity, name, transform) in named.iter() {
        let kind = match name.as_str() {
            "SheepHead" => SheepPartKind::Head,
            "SheepLegFL" => SheepPartKind::LegFrontLeft,
            "SheepLegFR" => SheepPartKind::LegFrontRight,
            "SheepLegBL" => SheepPartKind::LegBackLeft,
            "SheepLegBR" => SheepPartKind::LegBackRight,
            _ => continue,
        };
        let mut ancestor = entity;
        let sheep = loop {
            let Ok(child_of) = parents.get(ancestor) else {
                break None;
            };
            ancestor = child_of.parent();
            if animals.contains(ancestor) {
                break Some(ancestor);
            }
        };
        let Some(sheep) = sheep else {
            continue;
        };
        commands.entity(entity).insert(SheepPart {
            sheep,
            kind,
            rest: transform.rotation,
        });
    }
}

/// Swing the legs with the gait and nod the head into the grass while
/// grazing. Pitch is about local X: the art faces -Z, so a negative angle
/// lowers the muzzle.
///
/// Skipped for sheep the camera cannot see: posing writes five transforms per
/// sheep per frame, each a GPU re-upload, and an off-screen flock earns none.
pub(super) fn animate_pasture_sheep_parts(
    animals: Query<(&PastureAnimal, &ViewVisibility)>,
    mut parts: Query<(&SheepPart, &mut Transform)>,
) {
    for (part, mut transform) in parts.iter_mut() {
        let Ok((animal, visible)) = animals.get(part.sheep) else {
            continue;
        };
        if !visible.get() {
            continue;
        }
        let stride = (animal.speed / SHEEP_WALK_SPEED).clamp(0.0, 1.0);
        // ~1.4 m per full stride cycle at a 0.4 m leg.
        let swing = (animal.gait * 4.5).sin() * 0.42 * stride;
        let pitch = match part.kind {
            SheepPartKind::Head => -0.95 * animal.graze + 0.06 * stride * (animal.gait * 9.0).sin(),
            SheepPartKind::LegFrontLeft | SheepPartKind::LegBackRight => swing,
            SheepPartKind::LegFrontRight | SheepPartKind::LegBackLeft => -swing,
        };
        transform.rotation = part.rest * Quat::from_rotation_x(pitch);
    }
}
