//! Run difficulty (Phase X — run configurator). A pre-run difficulty choice
//! (persisted on `Meta.difficulty`, picked in the BUILD screen's DIFFICULTY tab)
//! scales the run's enemy HP. `apply_difficulty` runs once per freshly-spawned
//! enemy (via `Added<Enemy>`), so it stacks cleanly on top of the boss-tier
//! overlay without threading a multiplier through every spawn path.

use crate::components::{Enemy, Health};
use crate::meta::Meta;
use bevy::prelude::*;

/// The selectable difficulties: `(name, enemy-HP multiplier)`. Index 0 (Normal)
/// is the default and leaves HP unchanged.
pub const DIFFICULTIES: [(&str, f32); 3] = [("Normal", 1.0), ("Hard", 1.5), ("Brutal", 2.2)];

/// Enemy-HP multiplier for difficulty `level`, clamped to the table (default 1.0).
pub fn difficulty_hp_mult(level: u8) -> f32 {
    DIFFICULTIES.get(level as usize).map(|(_, m)| *m).unwrap_or(1.0)
}

/// Display name for difficulty `level` (clamped to the table).
pub fn difficulty_name(level: u8) -> &'static str {
    DIFFICULTIES.get(level as usize).map(|(n, _)| *n).unwrap_or("Normal")
}

/// Scale each freshly-spawned enemy's HP by the run difficulty (once, via
/// `Added<Enemy>`). On Normal the multiplier is 1.0, so this is a no-op.
pub fn apply_difficulty(meta: Res<Meta>, mut fresh: Query<&mut Health, Added<Enemy>>) {
    let mult = difficulty_hp_mult(meta.difficulty);
    if (mult - 1.0).abs() < f32::EPSILON {
        return;
    }
    for mut hp in &mut fresh {
        hp.max *= mult;
        hp.current = hp.max;
    }
}
