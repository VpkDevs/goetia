// Post stack: bloom prefilter/downsample/upsample (dual-filter style) and the
// final tonemap + grade + vignette composite. Bloom is 40% of the look.

struct PostParams {
    // x: threshold, y: soft knee, z: bloom strength, w: unused
    v: vec4<f32>,
};
@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<uniform> U: PostParams;
// Composite-only second input (bloom chain top).
@group(1) @binding(0) var bloom_tex: texture_2d<f32>;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> FsIn {
    // Single clip-space triangle covering the screen.
    var out: FsIn;
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

fn texel(uv: vec2<f32>) -> vec2<f32> {
    let d = vec2<f32>(textureDimensions(src));
    return 1.0 / d;
}

// 13-tap-ish downsample (weighted box of 4 quadrants + center).
fn down_sample(uv: vec2<f32>) -> vec3<f32> {
    let t = texel(uv);
    var c = textureSample(src, samp, uv).rgb * 0.5;
    c += textureSample(src, samp, uv + vec2<f32>(-t.x, -t.y)).rgb * 0.125;
    c += textureSample(src, samp, uv + vec2<f32>(t.x, -t.y)).rgb * 0.125;
    c += textureSample(src, samp, uv + vec2<f32>(-t.x, t.y)).rgb * 0.125;
    c += textureSample(src, samp, uv + vec2<f32>(t.x, t.y)).rgb * 0.125;
    return c;
}

@fragment
fn fs_prefilter(in: FsIn) -> @location(0) vec4<f32> {
    let c = down_sample(in.uv);
    let threshold = U.v.x;
    let knee = U.v.y;
    let brightness = max(c.r, max(c.g, c.b));
    let soft = clamp(brightness - threshold + knee, 0.0, 2.0 * knee);
    let contrib = max(soft * soft / (4.0 * knee + 1e-4), brightness - threshold);
    let w = contrib / max(brightness, 1e-4);
    return vec4<f32>(c * max(w, 0.0), 1.0);
}

@fragment
fn fs_down(in: FsIn) -> @location(0) vec4<f32> {
    return vec4<f32>(down_sample(in.uv), 1.0);
}

// Tent upsample; blended additively onto the target level.
@fragment
fn fs_up(in: FsIn) -> @location(0) vec4<f32> {
    let t = texel(in.uv) * 1.5;
    var c = textureSample(src, samp, in.uv).rgb * 0.25;
    c += textureSample(src, samp, in.uv + vec2<f32>(-t.x, 0.0)).rgb * 0.125;
    c += textureSample(src, samp, in.uv + vec2<f32>(t.x, 0.0)).rgb * 0.125;
    c += textureSample(src, samp, in.uv + vec2<f32>(0.0, -t.y)).rgb * 0.125;
    c += textureSample(src, samp, in.uv + vec2<f32>(0.0, t.y)).rgb * 0.125;
    c += textureSample(src, samp, in.uv + vec2<f32>(-t.x, -t.y)).rgb * 0.0625;
    c += textureSample(src, samp, in.uv + vec2<f32>(t.x, -t.y)).rgb * 0.0625;
    c += textureSample(src, samp, in.uv + vec2<f32>(-t.x, t.y)).rgb * 0.0625;
    c += textureSample(src, samp, in.uv + vec2<f32>(t.x, t.y)).rgb * 0.0625;
    return vec4<f32>(c, 1.0);
}

// ACES-ish filmic curve (Narkowicz).
fn aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_composite(in: FsIn) -> @location(0) vec4<f32> {
    var hdr = textureSample(src, samp, in.uv).rgb;
    let bloom = textureSample(bloom_tex, samp, in.uv).rgb;
    hdr += bloom * U.v.z;

    var c = aces(hdr);

    // Grade: sink shadows into cold violet, warm the highlights slightly.
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let shadow_tint = vec3<f32>(0.06, 0.03, 0.10);
    let high_tint = vec3<f32>(1.03, 1.0, 0.97);
    c = mix(c + shadow_tint * (1.0 - luma) * 0.35, c * high_tint, luma * 0.6);

    // Vignette.
    let p = in.uv - 0.5;
    let v = 1.0 - dot(p, p) * 0.9;
    c *= clamp(v, 0.3, 1.0);

    return vec4<f32>(c, 1.0);
}
