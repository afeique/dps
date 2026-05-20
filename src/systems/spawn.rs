//! Spawn the player ship on entering `Playing`. Enemies are no longer
//! hard-coded here — `systems::wave` spawns them over time via
//! `systems::enemy::spawn`. The ship silhouette is the lyon port from
//! `js/modules/render/shapes.js` (see `crate::render::shapes`).

use crate::components::*;
use crate::render::shapes;
use bevy::prelude::*;

pub fn spawn_player(mut commands: Commands) {
    commands
        .spawn((
            Ship::default(),
            Intent::default(),
            Weapon::default(),
            Velocity::default(),
            Collider { radius: 20.0 },
            Health::new(100.0),
            Lives { count: 2 },
            Shield {
                current: 50.0,
                max: 50.0,
            },
            Faction::Player,
            shapes::ship_hull(),
            Transform::from_xyz(0.0, -140.0, 0.0),
        ))
        .with_children(|ship| {
            ship.spawn((
                shapes::ship_cockpit(),
                Transform::from_translation(shapes::SHIP_COCKPIT_OFFSET.extend(1.0)),
            ));
        });
}
