// Low-resolution world material. Land keeps StandardMaterial's normal PBR
// response; ocean vertices are a stable, unlit continuation of the detailed
// water surface. COLOR_0 alpha is the authored land mask (1 land, 0 ocean),
// interpolated by the rasterizer to soften the far shoreline.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither;
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water_params: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_COLORS
    let ocean = 1.0 - clamp(in.color.a, 0.0, 1.0);
#else
    let ocean = 0.0;
#endif

    // COLOR_0 RGB already contains the depth ramp. Alpha is metadata here,
    // not transparency, so restore opacity before StandardMaterial shading.
    let authored_rgb = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(authored_rgb, 1.0);

    // Keep the far ocean beneath close-water fades, but retain the old hole
    // for land so coarse far geometry cannot poke through detailed terrain.
    // water_params.yz is the detail-hole center and w its half extent.
    if (water_params.w > 0.0
        && all(abs(in.world_position.xz - water_params.yz) <= vec2<f32>(water_params.w))
        && ocean < 0.5) {
        discard;
    }

    // Preserve the existing far-land renderer, including its atmospheric
    // processing. Far water intentionally follows the detailed water shader's
    // unlit path instead of being dimmed like a steeply-lit seabed.
    let lit = apply_pbr_lighting(pbr_input);
    let lit_land = main_pass_post_lighting_processing(pbr_input, lit);

    let day_w = smoothstep(-0.08, 0.12, water_params.x);
    let water_rgb = mix(
        authored_rgb * vec3<f32>(0.20, 0.26, 0.45),
        authored_rgb,
        day_w,
    );

    var out: FragmentOutput;
    out.color = vec4<f32>(mix(lit_land.rgb, water_rgb, ocean), 1.0);
    return out;
}
