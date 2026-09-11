//! Small orthographic rasterizer for an immutable character thumbnail job.
//! Supports the canonical palette/textured low-poly GLB, not a second general
//! purpose renderer. Lighting and framing deliberately stay independent of time
//! of day, world camera and character animation.

use std::sync::Arc;

use bevy::prelude::*;

use image::RgbaImage;

pub(super) struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub color: Vec4,
    pub uv: Vec2,
}

pub(super) struct Geometry {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct Part {
    pub geometry: Arc<Geometry>,
    pub color: Vec4,
    pub texture: Option<Arc<RgbaImage>>,
}

/// Two samples per axis produce smooth hair/helmet silhouettes at small HUD
/// sizes. Work happens once on a single background task, never per UI frame.
pub(super) fn render(
    parts: Vec<Part>,
    background: Option<Arc<RgbaImage>>,
    output_size: u32,
) -> Vec<u8> {
    let size = output_size * 2;
    let mut pixels = vec![Vec4::ZERO; (size * size) as usize];
    let mut depth = vec![f32::INFINITY; pixels.len()];
    if let Some(background) = background {
        for y in 0..size {
            for x in 0..size {
                let bx = (x * background.width() / size).min(background.width() - 1);
                let by = (y * background.height() / size).min(background.height() - 1);
                let texel = background.get_pixel(bx, by).0;
                let color = Color::srgba_u8(texel[0], texel[1], texel[2], texel[3]).to_linear();
                pixels[(y * size + x) as usize] = Vec4::from_array(color.to_f32_array());
            }
        }
    }
    let (view, top) = portrait_framing(&parts);
    let height = PORTRAIT_SPAN;
    let light = Vec3::new(-0.8, 0.9, -1.0).normalize();
    let fill = Vec3::new(0.8, 0.2, -0.4).normalize();
    for part in parts {
        let texture = &part.texture;
        let projected: Vec<_> = part
            .geometry
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
        for indices in part.geometry.indices.chunks_exact(3) {
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
            let vertices = [
                &part.geometry.vertices[ia],
                &part.geometry.vertices[ib],
                &part.geometry.vertices[ic],
            ];
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

// A wider upper-body composition leaves room for the blurred village around
// the subject. Keep one composition across HUD and book sizes so all consumers
// can share the same outfit cache entries.
const PORTRAIT_SPAN: f32 = 1.55;
const HEADROOM_FRACTION: f32 = 0.13;

fn portrait_framing(parts: &[Part]) -> (Mat4, f32) {
    // The humanoid faces -Z. A gentle three-quarter angle shows its authored
    // faceted side while retaining both eyes in the smallest medallions.
    let view = Mat4::look_at_rh(
        Vec3::new(1.25, 1.48, -2.6),
        Vec3::new(0.0, 1.32, 0.0),
        Vec3::Y,
    );
    // Measure in the actual view, including the selected hair/helmet, rather
    // than mixing world-space height with a tilted projection. This keeps air
    // above tall headwear without clipping it behind the decorative brass rim.
    let highest = parts
        .iter()
        .flat_map(|part| &part.geometry.vertices)
        .map(|vertex| view.transform_point3(vertex.position).y)
        .reduce(f32::max)
        .unwrap_or(0.4);
    (view, highest + PORTRAIT_SPAN * HEADROOM_FRACTION)
}

fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (p.x - a.x) * (b.y - a.y) - (p.y - a.y) * (b.x - a.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(color: Vec4, z: f32) -> Part {
        Part {
            geometry: Arc::new(Geometry {
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
            }),
            color,
            texture: None,
        }
    }

    #[test]
    fn occlusion_is_independent_of_primitive_order() {
        let front = || triangle(Vec4::new(1.0, 0.0, 0.0, 1.0), -0.1);
        let back = || triangle(Vec4::new(0.0, 1.0, 0.0, 1.0), 0.1);
        assert_eq!(
            render(vec![front(), back()], None, 32),
            render(vec![back(), front()], None, 32)
        );
    }

    #[test]
    fn portrait_has_transparent_background_and_antialiased_coverage() {
        let image = render(vec![triangle(Vec4::ONE, 0.0)], None, 32);
        assert_eq!(&image[..4], &[0, 0, 0, 0]);
        assert!(image.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(image
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255));
    }

    #[test]
    fn different_skin_or_cloth_colours_produce_different_portraits() {
        assert_ne!(
            render(vec![triangle(Vec4::new(0.8, 0.3, 0.1, 1.0), 0.0)], None, 32),
            render(
                vec![triangle(Vec4::new(0.1, 0.03, 0.01, 1.0), 0.0)],
                None,
                32
            ),
        );
    }

    #[test]
    fn full_frame_geometry_is_clipped_to_the_circular_medallion() {
        let mut part = triangle(Vec4::ONE, 0.0);
        Arc::get_mut(&mut part.geometry).unwrap().vertices = [
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
        Arc::get_mut(&mut part.geometry).unwrap().indices = vec![0, 1, 2, 0, 2, 3];
        let image = render(vec![part], None, 32);
        for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31)] {
            assert_eq!(image[(y * 32 + x) * 4 + 3], 0);
        }
        assert_eq!(image[(16 * 32 + 16) * 4 + 3], 255);
    }
    #[test]
    fn upper_body_portraits_keep_hair_clear_of_the_rim_and_faces_legible_at_hud_size() {
        for top in [1.74, 1.9] {
            let mut bust = triangle(Vec4::ONE, 0.0);
            let geometry = Arc::get_mut(&mut bust.geometry).unwrap();
            // Canonical head width with ordinary hair or a tall helmet; a
            // separate shirt extends below it to exercise circular body crop.
            geometry.vertices = [
                Vec3::new(-0.35, 1.08, -0.25),
                Vec3::new(0.35, 1.08, -0.25),
                Vec3::new(0.35, top, -0.25),
                Vec3::new(-0.35, top, -0.25),
                Vec3::new(-0.43, 0.52, 0.0),
                Vec3::new(0.43, 0.52, 0.0),
                Vec3::new(0.43, 1.08, 0.0),
                Vec3::new(-0.43, 1.08, 0.0),
            ]
            .map(|position| Vertex {
                position,
                normal: Vec3::NEG_Z,
                color: Vec4::ONE,
                uv: Vec2::ZERO,
            })
            .into();
            geometry.indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
            for size in [48, 192, 384] {
                let pixels = render(vec![bust.clone()], None, size);
                let coverage = |x, y| pixels[((y * size + x) * 4 + 3) as usize] > 0;
                let first_row = (0..size)
                    .find(|y| (0..size).any(|x| coverage(x, *y)))
                    .unwrap();
                assert!(first_row as f32 >= size as f32 * 0.10);
                assert!((first_row as f32) < size as f32 * 0.17);
                // The face still occupies at least a third of the diameter;
                // zooming out must not sacrifice the small HUD portrait.
                let face_row = size / 3;
                let face_width = (0..size).filter(|x| coverage(*x, face_row)).count();
                assert!(face_width >= size as usize / 3);
                // There is visible scenery beside the upper body, not a face
                // enlarged all the way to the ring on either side.
                assert!(!coverage(size / 8, size / 2));
                assert!(!coverage(size * 7 / 8, size / 2));
            }
        }
    }

    #[test]
    fn prepared_backdrop_fills_the_circle_without_blurring_the_subject() {
        let background = Arc::new(RgbaImage::from_pixel(4, 4, image::Rgba([30, 90, 20, 255])));
        let subject = || vec![triangle(Vec4::new(0.8, 0.3, 0.1, 1.0), 0.0)];
        let sharp = render(subject(), None, 32);
        let scene = render(subject(), Some(background), 32);
        assert_eq!(&scene[..4], &[0, 0, 0, 0]);
        let mut unchanged_subject = 0;
        let mut added_background = 0;
        for (original, composed) in sharp.chunks_exact(4).zip(scene.chunks_exact(4)) {
            if original[3] == 255 {
                assert_eq!(original, composed);
                unchanged_subject += 1;
            }
            if original[3] == 0 && composed[3] == 255 {
                added_background += 1;
            }
        }
        assert!(unchanged_subject > 0 && added_background > 0);
    }
}
