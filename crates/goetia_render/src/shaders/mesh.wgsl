// Flat-shaded instanced mesh pass with brute-force point lights, emissive,
// vertex-procedural motion (wobble replaces skeletal animation), dissolve.

const MAX_LIGHTS: u32 = 64u;

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    params: vec4<f32>, // x: light count, y: time
    lights_pos_radius: array<vec4<f32>, MAX_LIGHTS>,
    lights_color_intensity: array<vec4<f32>, MAX_LIGHTS>,
};
@group(0) @binding(0) var<uniform> G: Globals;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) emissive: vec4<f32>, // rgb premultiplied by strength, w: phase
    @location(8) anim: vec4<f32>,     // x: wobble amp, y: wobble speed, z: dissolve
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) emissive: vec4<f32>,
    @location(4) dissolve: f32,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    var local = in.pos;
    // Procedural flail: sway grows with height, per-instance phase.
    let t = G.params.y * in.anim.y + in.emissive.w;
    let sway = sin(t + local.y * 2.0) * in.anim.x * local.y;
    let sway2 = cos(t * 1.31 + local.y * 1.7) * in.anim.x * local.y * 0.6;
    local.x += sway;
    local.z += sway2;

    let wp4 = model * vec4<f32>(local, 1.0);
    let wp = wp4.xyz / wp4.w;
    // Normal via model rotation (assumes uniform-ish scale; fine for this game).
    let n = normalize((model * vec4<f32>(in.normal, 0.0)).xyz);

    var out: VsOut;
    out.clip = G.view_proj * vec4<f32>(wp, 1.0);
    out.world_pos = wp;
    out.normal = n;
    out.color = in.color;
    out.emissive = in.emissive;
    out.dissolve = in.anim.z;
    return out;
}

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3<f32>(127.1, 311.7, 74.7));
    return fract((q.x + q.y) * (q.z + 33.33) * 43758.5453);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Dissolve: eat the surface with animated noise (death burn-off).
    if (in.dissolve > 0.0) {
        let n = hash3(floor(in.world_pos * 14.0));
        if (n < in.dissolve) {
            discard;
        }
    }

    let N = normalize(in.normal);
    // Baked key light direction — the isometric sun of the underworld.
    let key_dir = normalize(vec3<f32>(-0.35, -1.0, -0.25));
    let key = max(dot(N, -key_dir), 0.0) * vec3<f32>(0.55, 0.5, 0.58);

    var lit = G.ambient.rgb + key;
    let count = u32(G.params.x);
    for (var i = 0u; i < count; i = i + 1u) {
        let lp = G.lights_pos_radius[i];
        let lc = G.lights_color_intensity[i];
        let to_l = lp.xyz - in.world_pos;
        let d = length(to_l);
        if (d < lp.w) {
            let att = pow(clamp(1.0 - d / lp.w, 0.0, 1.0), 2.0) * lc.w;
            let diff = max(dot(N, to_l / max(d, 1e-4)), 0.0);
            // Half-lambert-ish wrap so glows read on unlit sides too.
            let wrap = diff * 0.7 + 0.3;
            lit += lc.rgb * att * wrap;
        }
    }

    var rgb = in.color.rgb * lit + in.emissive.rgb;

    // Distance fog toward the void.
    let dist = length(in.world_pos - G.camera_pos.xyz);
    let f = 1.0 - exp(-max(dist - 150.0, 0.0) * G.fog.w);
    rgb = mix(rgb, G.fog.rgb, clamp(f, 0.0, 0.9));

    return vec4<f32>(rgb, in.color.a);
}
