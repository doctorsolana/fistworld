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

/// Whether a mesh carries vertex colour that means something.
///
/// "Means something" is the operative part: an all-white COLOR_0 is what a DCC
/// tool emits when nobody painted anything, and treating that as authored would
/// stop the ramp working on meshes it is meant for.
fn has_authored_vertex_color(mesh: &Mesh) -> bool {
    use bevy::mesh::VertexAttributeValues;
    const NEAR_WHITE: f32 = 0.99;
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(c)) => c
            .iter()
            .any(|v| v[0] < NEAR_WHITE || v[1] < NEAR_WHITE || v[2] < NEAR_WHITE),
        Some(VertexAttributeValues::Float32x3(c)) => c
            .iter()
            .any(|v| v[0] < NEAR_WHITE || v[1] < NEAR_WHITE || v[2] < NEAR_WHITE),
        // Unorm8/16 variants: any channel below full scale is painted colour.
        Some(VertexAttributeValues::Unorm8x4(c)) => c.iter().any(|v| v[..3].iter().any(|&x| x < 253)),
        Some(VertexAttributeValues::Uint16x4(c)) => {
            c.iter().any(|v| v[..3].iter().any(|&x| x < 64_800))
        }
        _ => false,
    }
}

/// Bake a root-to-tip color ramp into the mesh's vertex colors (multiplied
/// with base color by the standard PBR shader): slightly dark/desaturated at
/// the base, brighter and warmer at the tips — the classic stylized-foliage
/// gradient.
pub fn bake_foliage_color_ramp(mesh: &mut Mesh) {
    use bevy::mesh::VertexAttributeValues;
    // A mesh that already carries authored vertex colour is left ALONE.
    //
    // This ramp overwrites ATTRIBUTE_COLOR outright, which is correct for the
    // bought trees -- their colour lives in a texture and vertex colour is only
    // a multiplier, so there is nothing to destroy. The vegetation built in
    // this repo is the opposite: it has no texture at all and every bark and
    // leaf colour is in COLOR_0. Ramping one of those replaces the whole tree
    // with grey-green, and it compiles perfectly while doing it.
    //
    // The two styles are cleanly separable because the textured trees ship no
    // COLOR_0 whatsoever. Flat white is still ramped: it carries no information,
    // so overwriting it loses nothing.
    if has_authored_vertex_color(mesh) {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
    use bevy::asset::RenderAssetUsages;

    fn triangle(colors: Option<Vec<[f32; 4]>>) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
        );
        if let Some(colors) = colors {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_COLOR,
                VertexAttributeValues::Float32x4(colors),
            );
        }
        mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
        mesh
    }

    fn colors_of(mesh: &Mesh) -> Vec<[f32; 4]> {
        match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(c)) => c.clone(),
            _ => vec![],
        }
    }

    /// The failure this guards against is invisible to the compiler: a tree whose
    /// bark and leaves have been replaced by a grey ramp builds and runs perfectly.
    #[test]
    fn authored_vertex_colour_survives_the_ramp() {
        let painted = vec![
            [0.31, 0.19, 0.11, 1.0],
            [0.31, 0.19, 0.11, 1.0],
            [0.24, 0.42, 0.18, 1.0],
        ];
        let mut mesh = triangle(Some(painted.clone()));
        bake_foliage_color_ramp(&mut mesh);
        assert_eq!(
            colors_of(&mesh),
            painted,
            "vegetation that keeps its colour in COLOR_0 must not be repainted grey"
        );
    }

    /// The textured trees ship no COLOR_0 at all, so the ramp is still their path.
    #[test]
    fn a_mesh_without_vertex_colour_still_gets_the_ramp() {
        let mut mesh = triangle(None);
        bake_foliage_color_ramp(&mut mesh);
        let colors = colors_of(&mesh);
        assert_eq!(colors.len(), 3);
        assert!(
            colors[2][0] > colors[0][0],
            "the tip must end up brighter than the root: {colors:?}"
        );
    }

    /// Flat white carries no information, so overwriting it loses nothing.
    #[test]
    fn flat_white_is_not_treated_as_authored() {
        let mut mesh = triangle(Some(vec![[1.0, 1.0, 1.0, 1.0]; 3]));
        bake_foliage_color_ramp(&mut mesh);
        let colors = colors_of(&mesh);
        assert!(
            colors[0][0] < 0.99,
            "an unpainted white mesh should still be ramped: {colors:?}"
        );
    }
}
