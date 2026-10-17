@group(0) @binding(0) var<uniform> camera: mat4x4<f32>;
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
}
@vertex fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) color: vec3<f32>) -> VertexOut {
    var output: VertexOut;
    output.position = camera * vec4<f32>(p, 1.0);
    output.normal = n;
    output.color = color;
    return output;
}
@fragment fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(0.3, 0.8, 0.5));
    let shade = 0.3 + 0.7 * abs(dot(normalize(input.normal), light));
    return vec4<f32>(max(input.color, vec3<f32>(0.12)) * shade, 1.0);
}
