//! Deep-space nebula backdrop (`docs/port-plan.md` §3.4). A handful of large,
//! low-alpha circle blobs in blues, purples, and magentas sit at z ≈ -65..-60
//! — further back than the starfield (z ≈ -50..-45) — and drift downward more
//! slowly, giving the background layered parallax depth. Overlapping blobs
//! within each cluster produce a wispy cloud effect; the camera's bloom
//! softens the edges further. Scatter is a cheap deterministic hash; no extra
//! dependencies.

use crate::resources::PlayBounds;
use bevy::prelude::*;
// 0.18 split sprite rendering into `bevy_sprite_render`; `ColorMaterial` is in
// the prelude but `AlphaMode2d` must be reached through the render crate.
use bevy::sprite_render::AlphaMode2d;

/// A background nebula blob, drifting down at `drift` world-units/second.
#[derive(Component)]
pub struct Nebula {
    pub drift: f32,
}

/// Field extends 20% past the play bounds so wrap-around stays off-screen.
const FIELD_MARGIN: f32 = 1.20;

/// Cheap deterministic [0, 1) hash (murmur-style finalizer) — no `rand` dep.
/// Matches the pattern used by `starfield`.
fn rand01(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9E37_79B9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85EB_CA6B);
    x = (x ^ (x >> 13)).wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

/// Blob definition: SRGBA color (r,g,b,a), radius range (min, max), count,
/// drift speed (u/s), z depth.
struct BlobLayer {
    color: (f32, f32, f32, f32),
    r_min: f32,
    r_max: f32,
    count: u32,
    drift: f32,
    z: f32,
}

pub fn spawn_nebula(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    bounds: Res<PlayBounds>,
) {
    let field = bounds.half * FIELD_MARGIN;

    // Six shared palette entries. Low alpha keeps them translucent;
    // overlapping blobs within a cluster build opacity naturally.
    //
    // NOTE FOR PARENT: `ColorMaterial { alpha_mode: AlphaMode2d::Blend, .. }`
    // is the Bevy 0.18 API for alpha-blended 2D materials.  If compilation
    // fails here, check `bevy::sprite::AlphaMode2d` in the 0.18 docs — the
    // variant or path may differ (e.g. `AlphaMode2d::Transparent` or the
    // field may not yet be stable). The fallback is to remove `alpha_mode`
    // entirely and use `ColorMaterial::from(color)` — that may render opaque
    // or black but will at least compile, letting you isolate the API issue.
    let layers: &[BlobLayer] = &[
        // deep indigo cluster — two back-most blobs, very faint
        BlobLayer { color: (0.08, 0.04, 0.28, 0.06), r_min: 280.0, r_max: 480.0, count: 2, drift: 2.2, z: -65.0 },
        // cobalt-blue wisps — three blobs one step forward
        BlobLayer { color: (0.06, 0.12, 0.40, 0.08), r_min: 200.0, r_max: 380.0, count: 3, drift: 2.8, z: -63.5 },
        // purple haze — two medium blobs
        BlobLayer { color: (0.22, 0.05, 0.38, 0.07), r_min: 180.0, r_max: 320.0, count: 2, drift: 3.2, z: -62.0 },
        // magenta accent — two smaller, slightly brighter blobs
        BlobLayer { color: (0.40, 0.04, 0.28, 0.10), r_min: 150.0, r_max: 260.0, count: 2, drift: 3.8, z: -61.0 },
        // dusty violet — two soft blobs
        BlobLayer { color: (0.18, 0.06, 0.32, 0.09), r_min: 160.0, r_max: 300.0, count: 2, drift: 4.2, z: -60.5 },
        // faint rose glow — one larger blob, nearest layer
        BlobLayer { color: (0.45, 0.08, 0.20, 0.05), r_min: 220.0, r_max: 400.0, count: 1, drift: 4.8, z: -60.0 },
    ];

    let mut seed = 0xDEAD_BEEFu32;

    for layer in layers {
        let (r, g, b, a) = layer.color;
        let mat = materials.add(ColorMaterial {
            color: Color::srgba(r, g, b, a),
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        });

        for _ in 0..layer.count {
            let x = (rand01(seed) * 2.0 - 1.0) * field.x;
            let y = (rand01(seed + 1) * 2.0 - 1.0) * field.y;
            let radius = layer.r_min + rand01(seed + 2) * (layer.r_max - layer.r_min);
            seed += 3;

            let mesh = meshes.add(Circle::new(radius));
            commands.spawn((
                Nebula { drift: layer.drift },
                Mesh2d(mesh),
                MeshMaterial2d(mat.clone()),
                Transform::from_xyz(x, y, layer.z),
            ));
        }
    }
}

/// Drift nebula blobs slowly downward, wrapping vertically like the starfield.
pub fn drift_nebula(
    time: Res<Time>,
    bounds: Res<PlayBounds>,
    mut q: Query<(&Nebula, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let field_h = bounds.half.y * FIELD_MARGIN;
    let span = field_h * 2.0;
    for (neb, mut tf) in &mut q {
        tf.translation.y -= neb.drift * dt;
        if tf.translation.y < -field_h {
            tf.translation.y += span;
        }
    }
}
