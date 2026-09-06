//! Bounded, pooled assets for stone flight, impact dust and kicked-up chips.
use super::*;
use bevy::light::NotShadowCaster;
#[derive(Resource)]
pub(super) struct SiegeAssets {
    pub stone: Handle<Mesh>,
    pub rock: Handle<StandardMaterial>,
    dust: Vec<Handle<StandardMaterial>>,
    puff: Handle<Mesh>,
}
#[derive(Component)]
pub(super) struct FlightVisual;
#[derive(Component)]
pub(super) struct ImpactVisual;
#[derive(Component)]
pub(super) struct Particle {
    owner: Entity,
    index: usize,
    chip: bool,
}
pub(super) fn load_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let stone = meshes.add(Sphere::new(0.30).mesh().ico(1).unwrap());
    let puff = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    let rock = materials.add(StandardMaterial {
        base_color: Color::srgb(0.29, 0.27, 0.23),
        perceptual_roughness: 1.0,
        ..default()
    });
    let dust = (0..16)
        .map(|i| {
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.36, 0.29, 0.20, (1.0 - i as f32 / 15.0) * 0.26),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(SiegeAssets {
        stone,
        rock,
        dust,
        puff,
    });
}
pub(super) fn attach(
    mut commands: Commands,
    assets: Res<SiegeAssets>,
    stones: Query<Entity, (With<SiegeProjectile>, Without<FlightVisual>)>,
    impacts: Query<(Entity, &SiegeImpact), Without<ImpactVisual>>,
) {
    for entity in &stones {
        commands.entity(entity).insert((
            FlightVisual,
            Mesh3d(assets.stone.clone()),
            MeshMaterial3d(assets.rock.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    for (entity, impact) in &impacts {
        commands
            .entity(entity)
            .remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>, FlightVisual)>()
            .insert((
                ImpactVisual,
                Transform::from_translation(impact.position),
                Visibility::default(),
            ));
        for i in 0..22 {
            let chip = i >= 12;
            commands.spawn((
                Particle {
                    owner: entity,
                    index: i,
                    chip,
                },
                Mesh3d(if chip {
                    assets.stone.clone()
                } else {
                    assets.puff.clone()
                }),
                MeshMaterial3d(if chip {
                    assets.rock.clone()
                } else {
                    assets.dust[0].clone()
                }),
                Transform::default(),
                NotShadowCaster,
                ChildOf(entity),
            ));
        }
    }
}
fn noise(seed: u64, index: usize) -> f32 {
    let v = seed
        .wrapping_add(index as u64 * 7919)
        .wrapping_mul(6364136223846793005);
    ((v >> 32) as u32) as f32 / u32::MAX as f32
}
pub(super) fn animate(
    clock: Query<&WorldTime>,
    assets: Res<SiegeAssets>,
    mut stones: Query<(&SiegeProjectile, &mut Transform, &mut Visibility), With<FlightVisual>>,
    impacts: Query<&SiegeImpact>,
    mut particles: Query<
        (
            &Particle,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<FlightVisual>,
    >,
    mut gizmos: Gizmos,
) {
    let now = clock.iter().next().map_or(0.0, super::seconds);
    for (stone, mut transform, mut visible) in &mut stones {
        *visible = if now >= stone.launched_at && now < stone.impact_at {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation = stone.position(now.min(stone.impact_at));
        transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            (now - stone.launched_at) as f32 * 4.0,
            noise(stone.seed, 0) * 6.0,
            now as f32 * 2.0,
        );
        if now >= stone.launched_at && now < stone.impact_at {
            for i in 1..5 {
                gizmos.line(
                    stone.position(now - i as f64 * 0.025),
                    stone.position(now - (i - 1) as f64 * 0.025),
                    Color::srgba(0.65, 0.59, 0.45, 0.3 / i as f32),
                );
            }
        }
    }
    for (particle, mut transform, mut material) in &mut particles {
        let Ok(impact) = impacts.get(particle.owner) else {
            continue;
        };
        let age = (now - impact.at).max(0.0) as f32;
        let n = noise(impact.seed, particle.index);
        let angle = particle.index as f32 * 2.39996 + n;
        if particle.chip {
            let t = age.min(1.5);
            let speed = 2.0 + n * 4.5;
            transform.translation = Vec3::new(
                angle.cos() * speed * t,
                (1.8 + n * 4.0) * t - 4.9 * t * t,
                angle.sin() * speed * t,
            );
            transform.translation.y = transform.translation.y.max(0.06);
            transform.scale =
                Vec3::splat((0.2 + n * 0.3) * (1.0 - ((age - 1.2) / 1.1).clamp(0.0, 1.0)));
            transform.rotation = Quat::from_euler(EulerRot::XYZ, t * 9.0, n * 8.0, t * 3.0);
        } else {
            let radius = (1.0 - (-age * 3.0).exp()) * (1.3 + n * 3.5);
            transform.translation = Vec3::new(
                angle.cos() * radius,
                0.15 + age * (0.6 + n),
                angle.sin() * radius,
            );
            transform.scale =
                Vec3::new(1.0, 0.5 + n * 0.3, 1.0) * ((0.35 + age * 1.4) * (1.0 + n * 0.45));
            let alpha = ((age / 2.2).clamp(0.0, 1.0) * 15.0) as usize;
            material.0 = assets.dust[alpha].clone();
        }
    }
}
