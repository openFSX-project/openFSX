struct Scene { camera: mat4x4<f32>, model: mat4x4<f32> }
@group(0) @binding(0) var<uniform> scene: Scene;
@group(1) @binding(0) var diffuse_texture: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;
struct Material { diffuse: vec4<f32>, parameters: vec4<f32> }
@group(1) @binding(2) var<uniform> material: Material;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) uv: vec2<f32>, @location(3) model_space: f32) -> VertexOut {
    var output: VertexOut;
    let world = select(vec4<f32>(p, 1.0), scene.model * vec4<f32>(p, 1.0), model_space > 0.5);
    output.position = scene.camera * world;
    output.normal = select(n, (scene.model * vec4<f32>(n, 0.0)).xyz, model_space > 0.5);
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
    return vec4<f32>(color * shade, alpha);
}
