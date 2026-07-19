// GPU particles: spawn (ring copy), sim (integrate), draw (billboard quads).
// Dead particles collapse to a clipped vertex — no CPU involvement ever.

const MAX_LIGHTS: u32 = 64u;

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    params: vec4<f32>,
    lights_pos_radius: array<vec4<f32>, MAX_LIGHTS>,
    lights_color_intensity: array<vec4<f32>, MAX_LIGHTS>,
};

struct Particle {
    pos_life: vec4<f32>,  // xyz pos, w life remaining
    vel_size: vec4<f32>,  // xyz vel, w size
    color: vec4<f32>,
    params: vec4<f32>,    // x gravity, y drag, z max_life
};

struct SimParams {
    dt: f32,
    spawn_count: u32,
    cursor: u32,
    capacity: u32,
};

// ------------------------------------------------------------ compute

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> spawns: array<Particle>;
@group(0) @binding(2) var<uniform> P: SimParams;

@compute @workgroup_size(256)
fn cs_spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= P.spawn_count) {
        return;
    }
    let dst = (P.cursor + i) % P.capacity;
    particles[dst] = spawns[i];
}

@compute @workgroup_size(256)
fn cs_sim(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= P.capacity) {
        return;
    }
    var p = particles[i];
    if (p.pos_life.w <= 0.0) {
        return;
    }
    let dt = P.dt;
    var vel = p.vel_size.xyz;
    vel.y -= p.params.x * dt;
    vel *= max(1.0 - p.params.y * dt, 0.0);
    var pos = p.pos_life.xyz + vel * dt;
    // Ground bounce with damping — sparks skitter.
    if (pos.y < 0.02) {
        pos.y = 0.02;
        vel.y = abs(vel.y) * 0.35;
        vel.x *= 0.7;
        vel.z *= 0.7;
    }
    p.pos_life = vec4<f32>(pos, p.pos_life.w - dt);
    p.vel_size = vec4<f32>(vel, p.vel_size.w);
    particles[i] = p;
}

// ------------------------------------------------------------ draw

@group(0) @binding(0) var<uniform> G: Globals;
@group(1) @binding(0) var<storage, read> draw_particles: array<Particle>;

// Fixed isometric camera basis (matches camera.rs: yaw 45°, pitch atan(1/√2)).
const CAM_RIGHT: vec3<f32> = vec3<f32>(-0.70710678, 0.0, 0.70710678);
const CAM_UP: vec3<f32> = vec3<f32>(-0.40824829, 0.81649658, -0.40824829);

struct PVsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_particle(
    @builtin(vertex_index) vi: u32,
    @builtin(instance_index) ii: u32,
) -> PVsOut {
    var out: PVsOut;
    let p = draw_particles[ii];
    if (p.pos_life.w <= 0.0) {
        // Clip away: z outside [0,1].
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
        out.uv = vec2<f32>(0.0);
        out.color = vec4<f32>(0.0);
        return out;
    }
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    let t = clamp(p.pos_life.w / max(p.params.z, 1e-4), 0.0, 1.0); // 1 -> 0
    let size = p.vel_size.w * min(1.0, t * 4.0);
    let wp = p.pos_life.xyz + (CAM_RIGHT * c.x + CAM_UP * c.y) * size;
    out.clip = G.view_proj * vec4<f32>(wp, 1.0);
    out.uv = c;
    out.color = vec4<f32>(p.color.rgb, p.color.a * t);
    return out;
}

@fragment
fn fs_particle(in: PVsOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    let soft = smoothstep(1.0, 0.35, d);
    // Hot core boost so bloom picks sparks up.
    let core = smoothstep(0.5, 0.0, d) * 1.5;
    return vec4<f32>(in.color.rgb * (soft + core), in.color.a * soft);
}
