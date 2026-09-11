// Reuse the authored grayscale puff as density. Multiplying gray smoke by
// its dark image RGB made domestic plumes look sooty and disappear over roofs.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    mesh_view_bindings::lights,
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let sampled = pbr_input.material.base_color;
    let density = clamp(dot(sampled.rgb, vec3<f32>(0.3333)) * 2.4, 0.0, 1.0);
    // A cool pale centre with a warmer edge reads in sun and shadow. Alpha
    // carries both the authored soft contour and the pooled lifetime stage.
    let tint = mix(vec3<f32>(0.52, 0.51, 0.47), vec3<f32>(0.78, 0.79, 0.76), density);
    // Cards have no useful surface normal for volume lighting. Follow the
    // existing sky/key light instead; an unmodulated unlit puff glows white
    // over an otherwise moonlit village. Reuse view bindings, no extra FX
    // uniforms, lights, CPU material changes or per-house updates.
    var sky = lights.ambient_color.rgb;
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        sky += lights.directional_lights[i].color.rgb
            * max(lights.directional_lights[i].direction_to_light.y, 0.0);
    }
    let daylight = smoothstep(3000.0, 50000.0, max(max(sky.r, sky.g), sky.b));
    let illumination = mix(vec3<f32>(0.08, 0.10, 0.16), vec3<f32>(1.0), daylight);
    pbr_input.material.base_color = vec4<f32>(tint * illumination, sampled.a * sqrt(density));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    return out;
}
