//! Game-flow systems: what happens at the edges of `Playing`.
//!
//! Phase 1 keeps this minimal — on death we clear the field and let the player
//! restart the slice. The real title / results / shop screens arrive in Phase 5
//! (`docs/port-plan.md` §6); for now `GameOver` is a bare, replayable state.

use crate::components::{Bullet, Enemy};
use crate::states::GameState;
use bevy::prelude::*;

/// On entering `GameOver`, despawn the leftover enemies and bullets so a
/// restart starts from a clean field. The player ship is already despawned by
/// `systems::damage::apply_damage` at the moment of death.
pub fn game_over_cleanup(
    mut commands: Commands,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Bullet>)>>,
) {
    for e in &leftovers {
        commands.entity(e).despawn();
    }
    info!("GAME OVER — press R (or Enter) to fly again");
}

/// In `GameOver`, R or Enter restarts the slice by returning to `Playing`,
/// which re-runs the `OnEnter(Playing)` spawns (fresh ship + drifter).
pub fn restart_input(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::KeyR) || keys.just_pressed(KeyCode::Enter) {
        next.set(GameState::Playing);
    }
}
