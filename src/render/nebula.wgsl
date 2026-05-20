// Procedural JWST-style nebula for a 2D `Mesh2d` quad (see render::nebula).
//
// Domain-warped fractal noise builds wispy gas filaments, clustered into a few
// regions (dark space between them), with large dark dust lanes and HDR
// emission cores. Output is linear HDR (> 1.0 in the cores) so the camera's
// bloom lights the bright wisps. Animated via the global time uniform.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::globals

// x = brightness, y = noise scale, z = scroll speed, w = unused.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: vec4<f32>;

fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

// Value noise with smooth (quintic-ish) interpolation.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Fractal Brownian motion; rotated octaves to break up axis artifacts.
fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var v = 0.0;
    var amp = 0.55;
    let m = mat2x2<f32>(1.6, 1.2, -1.2, 1.6);
    for (var i = 0; i < 6; i = i + 1) {
        v = v + amp * vnoise(p);
        p = m * p;
        amp = amp * 0.5;
    }
    return v;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let brightness = params.x;
    let scale = params.y;
    let scroll = params.z;
    let t = globals.time;

    var uv = mesh.uv * scale;
    uv.y = uv.y - t * scroll; // slow upward parallax

    // Large-scale mask: nebula clusters here, empty space elsewhere.
    let region = fbm(uv * 0.35 + vec2<f32>(11.3, 4.7));

    // Domain warp → wispy, turbulent filaments.
    let warp = vec2<f32>(
        fbm(uv * 1.1 + vec2<f32>(0.0, 1.7)),
        fbm(uv * 1.1 + vec2<f32>(5.2, 9.3)),
    );
    let density = fbm(uv * 1.6 + warp * 2.5);

    // Gas present only where detail density AND the region mask are high.
    let neb = smoothstep(0.45, 0.95, density) * smoothstep(0.40, 0.80, region);

    // Per-region hue: some areas teal-dominant, others warm gold. Saturated
    // and weighted toward teal for the vivid JWST teal-vs-gold contrast.
    let hue = fbm(uv * 0.28 + vec2<f32>(20.0, -7.0));
    let teal = vec3<f32>(0.0, 0.62, 0.85);
    let gold = vec3<f32>(1.0, 0.42, 0.06);
    let base = mix(teal, gold, smoothstep(0.45, 0.85, hue));

    var col = base * neb * 1.6;

    // Tight hot cores (warm white, HDR > 1.0 → bloom) — kept narrow so the
    // colored gas reads instead of washing the dense regions to grey.
    let core = smoothstep(0.88, 1.0, density) * neb;
    col = col + vec3<f32>(1.5, 1.2, 0.8) * core;

    // Dark dust lanes absorbing light across large filaments.
    let dust = fbm(uv * 0.8 + vec2<f32>(30.0, 12.0));
    col = col * mix(0.35, 1.0, smoothstep(0.30, 0.70, dust));

    let alpha = clamp(neb, 0.0, 1.0);
    return vec4<f32>(col * brightness, alpha);
}
