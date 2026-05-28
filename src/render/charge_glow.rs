//! Energy charge-glow on the hull (graphical parity with rainboids'
//! `drawChargingGlowCore` / `drawEnergyChargeGlow`). A halo child of the ship
//! grows and shifts colour as the ENERGY meter fills, so the ship visibly
//! "charges up" toward a power-weapon shot: dim blue while building, cyan once
//! there's enough energy to fire, bright pulsing white-cyan when full.
//!
//! `update_charge_glow` drives one child entity spawned in `spawn_player`; it
//! scales the halo by the energy fraction each frame and only rebuilds the
//! (tessellated) `Shape` when the tier changes. Pure presentation — bloom flares
//! the over-bright fill into the actual glow.

use crate::resources::EnergyMeter;
use crate::systems::power_weapon::PowerWeapon;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// The hull charge-glow halo (a child of the ship). `tier` caches the last
/// colour band drawn so the `Shape` is only re-tessellated on a band change
/// (255 = unset, forces a rebuild on the first update).
#[derive(Component)]
pub struct ChargeGlow {
    pub tier: u8,
}

impl Default for ChargeGlow {
    fn default() -> Self {
        Self { tier: 255 }
    }
}

/// Base halo radius (world u); the live size is this × the energy-driven scale.
pub const GLOW_BASE_R: f32 = 18.0;

/// HDR fill for each charge band (bloom turns these into the glow halo):
/// 0 = building (dim blue), 1 = ready-to-fire (cyan), 2 = full (white-cyan).
fn tier_color(tier: u8) -> Color {
    match tier {
        2 => Color::linear_rgb(6.0, 8.0, 9.0),
        1 => Color::linear_rgb(0.5, 4.0, 5.0),
        _ => Color::linear_rgb(0.6, 1.2, 3.0),
    }
}

/// A filled disc of `radius` in `color` — the halo silhouette.
pub fn glow_disc(radius: f32, color: Color) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).fill(color).build()
}

/// Scale (and, on a band change, recolour) the hull halo from the energy meter.
pub fn update_charge_glow(
    time: Res<Time>,
    energy: Res<EnergyMeter>,
    pw: Res<PowerWeapon>,
    mut q: Query<(&mut Transform, &mut ChargeGlow, &mut Shape)>,
) {
    let max = energy.max.max(1.0);
    let progress = (energy.current / max).clamp(0.0, 1.0);
    let cost = pw.kind.energy_cost();
    let tier = if energy.current >= max * 0.999 {
        2
    } else if energy.current >= cost {
        1
    } else {
        0
    };

    for (mut tf, mut glow, mut shape) in &mut q {
        // Size ramps with stored energy; an empty meter hides the halo entirely.
        let mut s = if energy.current <= 0.0 { 0.0 } else { 1.0 + progress * 2.5 };
        // A full meter pulses to read as "ready" at a glance.
        if tier == 2 {
            s *= 1.0 + 0.08 * (time.elapsed_secs() * 8.0).sin();
        }
        tf.scale = Vec3::splat(s);
        if glow.tier != tier {
            glow.tier = tier;
            *shape = glow_disc(GLOW_BASE_R, tier_color(tier));
        }
    }
}
