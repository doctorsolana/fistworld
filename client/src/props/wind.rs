//! Wind-swayed foliage material: `StandardMaterial` extended with a vertex
//! shader that bends the upper part of the mesh in layered sine gusts.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type WindFoliageMaterial = ExtendedMaterial<StandardMaterial, WindExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct WindExtension {
    /// x: sway strength (m at tip), y: wind speed, z: sway_min_y (mesh-local),
    /// w: 1 / sway range (mesh-local).
    #[uniform(100)]
    pub params: Vec4,
    /// x: per-instance height jitter fraction (grass gets ±25% so identical
    /// tufts come out ragged), y: base height stretch (grass grows ~30%
    /// taller and slimmer, BotW-style), z: map half extent (m), w: climate
    /// seed phase — both for the frost tint (see wind_foliage.wgsl).
    #[uniform(101)]
    pub extra: Vec4,
}

impl MaterialExtension for WindExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/wind_foliage.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/wind_foliage.wgsl".into()
    }
}

/// Compute wind params for a mesh: sway starts a quarter of the way up so
/// trunk bases stay planted.
pub fn wind_params_for_mesh(mesh: &Mesh, strength: f32, speed: f32) -> Vec4 {
    let (min_y, max_y) = mesh_y_bounds(mesh).unwrap_or((0.0, 1.0));
    let range = (max_y - min_y).max(0.01);
    let sway_min_y = min_y + range * 0.25;
    let sway_range = (max_y - sway_min_y).max(0.01);
    Vec4::new(strength, speed, sway_min_y, 1.0 / sway_range)
}

pub fn mesh_y_bounds(mesh: &Mesh) -> Option<(f32, f32)> {
    use bevy::mesh::VertexAttributeValues;
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in positions {
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }
    (min_y.is_finite() && max_y.is_finite()).then_some((min_y, max_y))
}

/// Bake a root-to-tip color ramp into the mesh's vertex colors (multiplied
/// with base color by the standard PBR shader): slightly dark/desaturated at
/// the base, brighter and warmer at the tips — the classic stylized-foliage
/// gradient.
pub fn bake_foliage_color_ramp(mesh: &mut Mesh) {
    use bevy::mesh::VertexAttributeValues;
    let Some((min_y, max_y)) = mesh_y_bounds(mesh) else {
        return;
    };
    let range = (max_y - min_y).max(0.01);
    let VertexAttributeValues::Float32x3(positions) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned().unwrap()
    else {
        return;
    };
    const ROOT: [f32; 3] = [0.74, 0.78, 0.68];
    const TIP: [f32; 3] = [1.12, 1.09, 0.97];
    let colors: Vec<[f32; 4]> = positions
        .iter()
        .map(|p| {
            let t = ((p[1] - min_y) / range).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            [
                ROOT[0] + (TIP[0] - ROOT[0]) * t,
                ROOT[1] + (TIP[1] - ROOT[1]) * t,
                ROOT[2] + (TIP[2] - ROOT[2]) * t,
                1.0,
            ]
        })
        .collect();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(colors),
    );
}
