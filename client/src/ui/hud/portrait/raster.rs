//! Small orthographic rasterizer for an immutable character thumbnail job.
//! Supports the canonical palette/textured low-poly GLB, not a second general
//! purpose renderer. Lighting and framing deliberately stay independent of time
//! of day, world camera and character animation.

use bevy::prelude::*;

pub(super) struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub color: Vec4,
    pub uv: Vec2,
}

pub(super) struct Part {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<usize>,
    pub color: Vec4,
    pub texture: Option<Image>,
}

/// Two samples per axis produce smooth hair/helmet silhouettes at small HUD
/// sizes. Work happens once on a single background task, never per UI frame.
pub(super) fn render(parts: Vec<Part>, output_size: u32) -> Vec<u8> {
    let size = output_size * 2;
    let mut pixels = vec![Vec4::ZERO; (size * size) as usize];
    let mut depth = vec![f32::INFINITY; pixels.len()];
    let highest = parts
        .iter()
        .flat_map(|part| &part.vertices)
        .map(|v| v.position.y)
        .fold(1.7_f32, f32::max);
    // The game humanoid faces -Z. A small three-quarter turn gives its faceted
    // nose and hair depth while keeping both eyes legible in an 80px medallion.
    let view = Mat4::look_at_rh(
        Vec3::new(1.0, 1.50, -2.5),
        Vec3::new(0.0, 1.32, 0.0),
        Vec3::Y,
    );
    let top = highest - 1.32 + 0.08;
    let height = 1.23;
    let light = Vec3::new(-0.8, 0.9, -1.0).normalize();
    let fill = Vec3::new(0.8, 0.2, -0.4).normalize();
    for part in parts {
        let texture = part
            .texture
            .and_then(|image| image.try_into_dynamic().ok())
            .map(|image| image.to_rgba8());
        let projected: Vec<_> = part
            .vertices
            .iter()
            .map(|vertex| {
                let p = view.transform_point3(vertex.position);
                Vec3::new(
                    (p.x / height + 0.5) * size as f32,
                    (top - p.y) / height * size as f32,
                    -p.z,
                )
            })
            .collect();
        for indices in part.indices.chunks_exact(3) {
            let [ia, ib, ic] = [indices[0], indices[1], indices[2]];
            let (a, b, c) = (projected[ia], projected[ib], projected[ic]);
            let area = edge(a.truncate(), b.truncate(), c.truncate());
            if !area.is_finite() || area.abs() < 0.001 {
                continue;
            }
            let min = a
                .truncate()
                .min(b.truncate())
                .min(c.truncate())
                .floor()
                .max(Vec2::ZERO);
            let max = a
                .truncate()
                .max(b.truncate())
                .max(c.truncate())
                .ceil()
                .min(Vec2::splat(size as f32 - 1.0));
            let vertices = [&part.vertices[ia], &part.vertices[ib], &part.vertices[ic]];
            let face_normal = (vertices[1].position - vertices[0].position)
                .cross(vertices[2].position - vertices[0].position)
                .normalize_or_zero();
            for y in min.y as u32..=max.y.max(0.0) as u32 {
                for x in min.x as u32..=max.x.max(0.0) as u32 {
                    if x >= size || y >= size {
                        continue;
                    }
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let weights = Vec3::new(
                        edge(b.truncate(), c.truncate(), p),
                        edge(c.truncate(), a.truncate(), p),
                        edge(a.truncate(), b.truncate(), p),
                    ) / area;
                    if weights.min_element() < -0.0001 {
                        continue;
                    }
                    let z = weights.dot(Vec3::new(a.z, b.z, c.z));
                    let index = (y * size + x) as usize;
                    if z >= depth[index] {
                        continue;
                    }
                    let mut color = part.color
                        * (vertices[0].color * weights.x
                            + vertices[1].color * weights.y
                            + vertices[2].color * weights.z);
                    if let Some(texture) = &texture {
                        let uv = vertices[0].uv * weights.x
                            + vertices[1].uv * weights.y
                            + vertices[2].uv * weights.z;
                        let x = ((uv.x.rem_euclid(1.0) * texture.width() as f32) as u32)
                            .min(texture.width() - 1);
                        let y = ((uv.y.rem_euclid(1.0) * texture.height() as f32) as u32)
                            .min(texture.height() - 1);
                        let texel = texture.get_pixel(x, y).0;
                        let linear =
                            Color::srgba_u8(texel[0], texel[1], texel[2], texel[3]).to_linear();
                        color *= Vec4::from_array(linear.to_f32_array());
                    }
                    if color.w < 0.5 {
                        continue;
                    }
                    let normal = (vertices[0].normal * weights.x
                        + vertices[1].normal * weights.y
                        + vertices[2].normal * weights.z)
                        .try_normalize()
                        .unwrap_or(face_normal);
                    // Warm key from the upper left, soft neutral fill. The
                    // visible side of the head stays darker than its front,
                    // making the authored faceted shape legible at HUD size.
                    let key = normal.dot(light).max(0.0) * 0.65;
                    let ambient = 0.38 + normal.dot(fill).max(0.0) * 0.06;
                    let illumination = Vec3::splat(ambient) + Vec3::new(1.04, 0.98, 0.90) * key;
                    pixels[index] = Vec4::new(
                        color.x * illumination.x,
                        color.y * illumination.y,
                        color.z * illumination.z,
                        1.0,
                    );
                    depth[index] = z;
                }
            }
        }
    }
    let mut output = Vec::with_capacity((output_size * output_size * 4) as usize);
    for y in 0..output_size {
        for x in 0..output_size {
            // Bevy's ancestor overflow clip is rectangular. Crop coverage in
            // the thumbnail itself so shirt/shoulder corners never escape a
            // circular medallion, with the same 2x edge antialiasing as hair.
            let sample = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .into_iter()
                .map(|(dx, dy)| {
                    let sx = x * 2 + dx;
                    let sy = y * 2 + dy;
                    let at = Vec2::new(sx as f32 + 0.5, sy as f32 + 0.5) / size as f32
                        - Vec2::splat(0.5);
                    if at.length_squared() <= 0.25 {
                        pixels[(sy * size + sx) as usize]
                    } else {
                        Vec4::ZERO
                    }
                })
                .sum::<Vec4>()
                * 0.25;
            // Average coverage in premultiplied linear space, then return normal
            // sRGB + straight alpha for Bevy's ImageNode blend contract.
            let rgb = if sample.w > 0.0 {
                sample.truncate() / sample.w
            } else {
                Vec3::ZERO
            };
            let color = LinearRgba::new(rgb.x, rgb.y, rgb.z, sample.w);
            output.extend_from_slice(&Srgba::from(color).to_u8_array());
        }
    }
    output
}

fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(color: Vec4, z: f32) -> Part {
        Part {
            vertices: [
                Vec3::new(-0.3, 1.1, z),
                Vec3::new(0.3, 1.1, z),
                Vec3::new(0.0, 1.7, z),
            ]
            .map(|position| Vertex {
                position,
                normal: Vec3::NEG_Z,
                color: Vec4::ONE,
                uv: Vec2::ZERO,
            })
            .into(),
            indices: vec![0, 1, 2],
            color,
            texture: None,
        }
    }

    #[test]
    fn occlusion_is_independent_of_primitive_order() {
        let front = || triangle(Vec4::new(1.0, 0.0, 0.0, 1.0), -0.1);
        let back = || triangle(Vec4::new(0.0, 1.0, 0.0, 1.0), 0.1);
        assert_eq!(
            render(vec![front(), back()], 32),
            render(vec![back(), front()], 32)
        );
    }

    #[test]
    fn portrait_has_transparent_background_and_antialiased_coverage() {
        let image = render(vec![triangle(Vec4::ONE, 0.0)], 32);
        assert_eq!(&image[..4], &[0, 0, 0, 0]);
        assert!(image.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(image
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    }

    #[test]
    fn different_skin_or_cloth_colours_produce_different_portraits() {
        assert_ne!(
            render(vec![triangle(Vec4::new(0.8, 0.3, 0.1, 1.0), 0.0)], 32),
            render(vec![triangle(Vec4::new(0.1, 0.03, 0.01, 1.0), 0.0)], 32),
        );
    }

    #[test]
    fn full_frame_geometry_is_clipped_to_the_circular_medallion() {
        let mut part = triangle(Vec4::ONE, 0.0);
        part.vertices = [
            Vec3::new(-4.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(4.0, 4.0, 0.0),
            Vec3::new(-4.0, 4.0, 0.0),
        ]
        .map(|position| Vertex {
            position,
            normal: Vec3::NEG_Z,
            color: Vec4::ONE,
            uv: Vec2::ZERO,
        })
        .into();
        part.indices = vec![0, 1, 2, 0, 2, 3];
        let image = render(vec![part], 32);
        for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31)] {
            assert_eq!(image[(y * 32 + x) * 4 + 3], 0);
        }
        assert_eq!(image[(16 * 32 + 16) * 4 + 3], 255);
    }
}
