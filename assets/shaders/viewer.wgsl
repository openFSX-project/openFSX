// fog: rgb colour and density per metre (0 disables); eye is the world camera;
// light: rgb time-of-day tint applied to lit (non-self-luminous) surfaces.
struct Scene { camera: mat4x4<f32>, model: mat4x4<f32>, eye: vec4<f32>, fog: vec4<f32>, light: vec4<f32> }
@group(0) @binding(0) var<uniform> scene: Scene;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;
struct Material { diffuse: vec4<f32>, parameters: vec4<f32> }
@group(1) @binding(2) var<uniform> material: Material;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
    @location(3) fogged: f32,
    @location(4) aircraft: f32,
}
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) model_space: f32) -> VertexOut {
    var output: VertexOut;
    // model_space: 0 world, 1 aircraft model, 2 world without haze or tint
    // (sky dome, lights).
    let in_model = model_space > 0.5 && model_space < 1.5;
    let world = select(vec4<f32>(p, 1.0), scene.model * vec4<f32>(p, 1.0), in_model);
    output.world = world.xyz;
    output.fogged = select(1.0, 0.0, model_space > 1.5);
    output.aircraft = select(0.0, 1.0, in_model);
    output.position = scene.camera * world;
    output.normal = select(n, (scene.model * vec4<f32>(n, 0.0)).xyz, in_model);
    // Preserve Direct3D-style MDLX coordinates when uploading DDS rows unchanged.
    output.uv = uv;
    return output;
}
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(0.3, 0.8, 0.5));
    let shade = 0.3 + 0.7 * abs(dot(normalize(input.normal), light));
    let sample = textureSample(diffuse_texture, diffuse_sampler, input.uv);
    let has_texture = material.parameters.x > 0.5;
    let color = select(material.diffuse.rgb, sample.rgb, has_texture);
    let alpha = select(1.0, select(1.0, sample.a, has_texture) * material.diffuse.a, material.parameters.y > 0.5);
    let test_alpha = select(material.diffuse.a, sample.a, has_texture);
    let function = u32(material.parameters.z);
    let reference = material.parameters.w;
    if function == 1u || (function == 2u && !(test_alpha < reference)) ||
       (function == 3u && !(abs(test_alpha-reference) < 0.001)) ||
       (function == 4u && !(test_alpha <= reference)) ||
       (function == 5u && !(test_alpha > reference)) ||
       (function == 6u && !(abs(test_alpha-reference) >= 0.001)) ||
       (function == 7u && !(test_alpha >= reference)) { discard; }
    let d = distance(input.world, scene.eye.xyz) * scene.fog.w;
    let haze = (1.0 - exp(-d * d)) * input.fogged;
    // The aircraft keeps a floor of panel/instrument lighting at night.
    let lit = select(scene.light.rgb, max(scene.light.rgb, vec3<f32>(0.3)), input.aircraft > 0.5);
    let tint = select(vec3<f32>(1.0), lit, input.fogged > 0.5);
    return vec4<f32>(mix(color * shade * tint, scene.fog.rgb, haze), alpha);
}
