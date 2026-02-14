#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;

struct SniperFisheyeSettings {
    strength: f32,
    edge_start: f32,
    _padding: vec2<f32>,
}

@group(0) @binding(2) var<uniform> settings: SniperFisheyeSettings;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let centered = in.uv * 2.0 - vec2<f32>(1.0, 1.0);
    let radius_sq = dot(centered, centered);
    let radius = sqrt(radius_sq);

    // Ease distortion in near the edges so the center reticle stays stable.
    let edge_weight = smoothstep(settings.edge_start, 1.1, radius);
    let warp = 1.0 + settings.strength * edge_weight * radius_sq;
    let sample_uv = centered * warp * 0.5 + vec2<f32>(0.5, 0.5);
    let clamped_uv = clamp(sample_uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));

    return textureSample(screen_texture, screen_sampler, clamped_uv);
}
