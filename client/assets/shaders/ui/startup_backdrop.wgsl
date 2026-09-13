#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var<uniform> finish: vec4<f32>;
@group(1) @binding(1) var village: texture_2d<f32>;
@group(1) @binding(2) var village_sampler: sampler;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let size = max(in.size, vec2<f32>(1.0));
    let source = vec2<f32>(textureDimensions(village));
    let drawn = source * max(size.x / source.x, size.y / source.y);
    let uv = (in.uv - 0.5) * size / drawn + 0.5;
    let step = vec2<f32>(finish.y) / source;
    var color = textureSample(village, village_sampler, uv).rgb * 0.28;
    color += textureSample(village, village_sampler, uv + vec2<f32>(step.x, step.y)).rgb * 0.18;
    color += textureSample(village, village_sampler, uv + vec2<f32>(-step.x, step.y)).rgb * 0.18;
    color += textureSample(village, village_sampler, uv + vec2<f32>(step.x, -step.y)).rgb * 0.18;
    color += textureSample(village, village_sampler, uv - step).rgb * 0.18;
    let right_shade = smoothstep(0.54, 0.95, in.uv.x) * finish.z;
    let edge = smoothstep(0.40, 0.78, length(in.uv - 0.5)) * 0.09;
    color *= 1.0 - clamp(finish.x + right_shade + edge, 0.0, 0.65);
    return vec4<f32>(color, 1.0);
}
