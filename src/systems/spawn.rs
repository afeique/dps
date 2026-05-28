//! Spawn the player ship on entering `Playing`. Enemies are no longer
//! hard-coded here — `systems::wave` spawns them over time via
//! `systems::enemy::spawn`. The ship silhouette is the lyon port from
//! `js/modules/render/shapes.js` (see `crate::render::shapes`).

use crate::components::*;
use crate::render::{charge_glow, shapes, shield_bubble};
use bevy::prelude::*;

pub fn spawn_player(mut commands: Commands, meta: Res<crate::meta::Meta>) {
    // The hull wears the account's selected cosmetic skin (Phase UI).
    let skin = shapes::skin_for(meta.skin);
    commands
        .spawn((
            Ship::default(),
            Intent::default(),
            Weapon::default(),
            Velocity::default(),
            Collider { radius: 20.0 },
            // Spec II.2 player model: base max HP 40, one spare health tank
            // (total effective lives 2), shield = 15% flat damage reduction.
            Health::new(40.0),
            ItemHpBonus::default(),
            Lives { count: 1, progress: 0.0 },
            Shield {
                reduction: BASE_SHIELD_REDUCTION,
            },
            // Player elemental resistances (E5/E8) — neutral until item resist
            // affixes populate them; read by `enemy_contact_player`.
            crate::combat::element::Resistances::new(),
            Faction::Player,
            shapes::ship_hull_skin(skin),
            Transform::from_xyz(0.0, -140.0, 0.0),
        ))
        .with_children(|ship| {
            ship.spawn((
                shapes::ship_cockpit(),
                Transform::from_translation(shapes::SHIP_COCKPIT_OFFSET.extend(1.0)),
            ));
            // Energy charge-glow halo behind the hull (driven by update_charge_glow).
            ship.spawn((
                charge_glow::ChargeGlow::default(),
                charge_glow::glow_disc(
                    charge_glow::GLOW_BASE_R,
                    Color::linear_rgb(0.6, 1.2, 3.0),
                ),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05))
                    .with_scale(Vec3::ZERO),
            ));
            // Invuln shield-bubble ring (hidden until update_shield_bubble shows it).
            ship.spawn((
                shield_bubble::ShieldBubble,
                shield_bubble::bubble_ring(),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.3)).with_scale(Vec3::ZERO),
            ));
        });
}
