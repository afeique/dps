//! Account level-up aura (reward feedback). When the meta `level` rises during a
//! run (XP banked per kill by `meta::award_xp`), `level_up_aura` spawns a golden
//! expanding ring at the ship — reusing `reaction_fx`'s [`Shockwave`] +
//! `tick_shockwaves` (which grows + fades + despawns it). Pure presentation; the
//! first observed level is adopted silently so loading a run never pops an aura.

use crate::audio::Sfx;
use crate::components::Ship;
use crate::meta::Meta;
use crate::render::reaction_fx::{unit_ring, Shockwave};
use bevy::prelude::*;

/// Peak radius (px) the level-up ring expands to — a touch larger than reactions.
const AURA_PEAK: f32 = 150.0;

/// Spawn a golden ring (+ a chime) at the ship whenever the account level rises.
/// `Sfx` is optional so headless tests (which don't build the audio assets) run.
pub fn level_up_aura(
    meta: Res<Meta>,
    sfx: Option<Res<Sfx>>,
    mut prev: Local<Option<u32>>,
    mut commands: Commands,
    ship: Query<&Transform, With<Ship>>,
) {
    match *prev {
        // First sighting: adopt the current level without firing (covers a freshly
        // loaded run already at level N, and re-entering Playing).
        None => {
            *prev = Some(meta.level);
        }
        Some(p) if meta.level > p => {
            *prev = Some(meta.level);
            if let Ok(tf) = ship.single() {
                commands.spawn((
                    Shockwave { age: 0.0, peak: AURA_PEAK },
                    unit_ring(Color::linear_rgb(9.0, 7.0, 1.5)), // gold
                    Transform::from_translation(tf.translation.truncate().extend(1.7))
                        .with_scale(Vec3::splat(1.0)),
                ));
            }
            if let Some(sfx) = sfx {
                commands.spawn((AudioPlayer::new(sfx.levelup.clone()), PlaybackSettings::DESPAWN));
            }
        }
        Some(_) => {}
    }
}
