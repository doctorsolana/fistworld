#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> finish: vec4<f32>;
@group(1) @binding(1) var illustration: texture_2d<f32>;
@group(1) @binding(2) var illustration_sampler: sampler;

fn grain(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    // Fixed coordinate noise: no time input, no shimmer when the book is idle.
    let h = fract(sin(vec4<f32>(
        dot(cell, vec2<f32>(127.1, 311.7)),
        dot(cell + vec2<f32>(1.0, 0.0), vec2<f32>(127.1, 311.7)),
        dot(cell + vec2<f32>(0.0, 1.0), vec2<f32>(127.1, 311.7)),
        dot(cell + vec2<f32>(1.0, 1.0), vec2<f32>(127.1, 311.7))
    )) * 43758.5453);
    return mix(mix(h.x, h.y, u.x), mix(h.z, h.w, u.x), u.y);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.size, vec2<f32>(1.0));
    let source = vec2<f32>(textureDimensions(illustration));
    let cover = max(size.x / source.x, size.y / source.y);
    let contained = min(size.x / source.x, size.y / source.y);
    let is_vignette = finish.x > 0.5 && finish.x < 1.5;
    // Printed vignettes preserve the whole composition, including windmill
    // sails. Medallions instead fill their circle with a central crop.
    let drawn = source * select(cover, contained, is_vignette);
    let p = (in.uv - 0.5) * size + drawn * 0.5;
    let uv = p / drawn;
    let color = textureSample(illustration, illustration_sampler, uv);
    var opacity = 1.0;
    if finish.x > 1.5 {
        let radius = length((in.uv - 0.5) * 2.0);
        opacity = 1.0 - smoothstep(0.97, 1.0, radius);
    } else if finish.x > 0.5 {
        let edge = min(min(p.x, drawn.x - p.x), min(p.y, drawn.y - p.y));
        let width = clamp(min(drawn.x, drawn.y) * 0.12, 3.0, 23.0);
        let roughness = grain(p * 0.12) * 0.65 + grain(p * 0.39) * 0.35;
        let inset = width * (0.18 + roughness * 0.7);
        opacity = smoothstep(inset, inset + width * 0.72, edge);
    }
    return vec4<f32>(color.rgb, color.a * opacity);
}
