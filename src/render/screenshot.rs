//! Dev/CI screenshot hook. With `DPS_SCREENSHOT=<path>` set, the app grabs the
//! primary window's framebuffer ~1 s in, writes a PNG, and exits — no OS
//! screen-recording permission needed (it reads the GPU framebuffer, not the
//! display). Doubles as the `docs/port-plan.md` §9 "screenshot diff vs the web
//! build" capture. A no-op when the env var is unset, so normal runs are
//! unaffected.

use crate::components::{Intent, Invulnerable, Ship};
use crate::states::GameState;
use crate::systems::tower::{tower_shape, Tower, TowerKind};
use crate::systems::wave::Wave;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

#[derive(Resource)]
struct ScreenshotPlan {
    path: String,
    taken: bool,
}

pub struct ScreenshotPlugin;

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("DPS_SCREENSHOT") {
            app.insert_resource(ScreenshotPlan { path, taken: false })
                .add_systems(Update, capture_then_exit);
            // `DPS_DEMO=1` (with DPS_SCREENSHOT) keeps the player alive + auto-
            // firing so a late capture shows a populated combat scene. Without
            // it, the capture reflects *normal* play.
            if std::env::var("DPS_DEMO").is_ok() {
                app.add_systems(
                    Update,
                    (
                        demo_drive,
                        keep_player_alive,
                        force_fire.after(crate::systems::input::gather_input),
                        // DPS_RADIAL=1: hold F so the screenshot shows the radial.
                        (|mut keys: ResMut<ButtonInput<KeyCode>>| keys.press(KeyCode::KeyF))
                            .run_if(|| std::env::var("DPS_RADIAL").is_ok()),
                        // DPS_BLAST=1: emit staged Deaths just before the capture
                        // so the screenshot lands on fresh explosions (FX dev).
                        blast_probe.run_if(|| std::env::var("DPS_BLAST").is_ok()),
                    ),
                );
            }
        }
    }
}

/// Capture at frame 60 (scene warmed up + rendered), then hard-exit at frame
/// 150 to give the async GPU readback + PNG write plenty of time to finish.
fn capture_then_exit(
    mut commands: Commands,
    mut plan: ResMut<ScreenshotPlan>,
    mut frame: Local<u32>,
) {
    *frame += 1;
    if !plan.taken && *frame >= 150 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(plan.path.clone()));
        plan.taken = true;
        info!("DPS_SCREENSHOT: captured frame -> {}", plan.path);
    }
    if *frame >= 280 {
        info!("DPS_SCREENSHOT: exiting");
        std::process::exit(0);
    }
}

/// Screenshot/demo-only: auto-start a run (Title → Playing), skip the pre-wave
/// build window, and drop a ring of demo turrets around the Core — so a capture
/// shows a populated tower-defense scene instead of the title or an empty prep.
/// Env-gated (`DPS_DEMO`), so it never runs in normal play.
fn demo_drive(
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut wave: ResMut<Wave>,
    mut commands: Commands,
    mut placed: Local<bool>,
) {
    match state.get() {
        GameState::Title => next.set(GameState::Playing),
        GameState::Playing if !*placed => {
            *placed = true;
            wave.skip_prep();
            let kinds = [TowerKind::Gun, TowerKind::Frost, TowerKind::Inferno, TowerKind::Flak];
            for (i, &kind) in kinds.iter().enumerate() {
                let a = i as f32 / kinds.len() as f32 * std::f32::consts::TAU;
                let pos = Vec2::new(a.cos(), a.sin()) * 150.0;
                commands.spawn((
                    Tower::new(kind),
                    tower_shape(kind),
                    Transform::from_translation(pos.extend(0.4)),
                ));
            }
        }
        _ => {}
    }
}

/// FX-dev only (`DPS_BLAST=1`): emit a spread of `Death` messages a few frames
/// before the frame-150 capture, each tagged with a different element so the
/// screenshot shows several fresh explosions (incl. the 3D perspective rings) at
/// once. Staggered across frames 138/142/146 so cores + expanding rings overlap.
fn blast_probe(
    mut deaths: MessageWriter<crate::messages::Death>,
    mut reactions: MessageWriter<crate::messages::Reaction>,
    mut frame: Local<u32>,
) {
    use crate::components::EnemyKind;
    use crate::messages::ReactionFx;
    *frame += 1;
    let kinds = [
        (EnemyKind::Tangerine, Vec2::new(-180.0, 90.0)),  // pyro (orange)
        (EnemyKind::FrostLance, Vec2::new(170.0, 70.0)),  // cryo (blue)
        (EnemyKind::TeslaWraith, Vec2::new(-120.0, -110.0)), // volt (violet)
        (EnemyKind::Plaguebearer, Vec2::new(140.0, -120.0)), // toxic (green)
        (EnemyKind::Hunter, Vec2::new(0.0, 150.0)),       // kinetic (pale)
    ];
    let fire_on = matches!(*frame, 138 | 142 | 146);
    if fire_on {
        for (kind, pos) in kinds {
            deaths.write(crate::messages::Death {
                entity: Entity::PLACEHOLDER,
                position: pos,
                kind: Some(kind),
                boss_tier: 0,
                mini_boss: false,
            });
        }
        // Also fire the shared shockwave pop-rings (shatter/flare) so the capture
        // shows they're now thin + 3D-tilted too, not flat discs.
        reactions.write(crate::messages::Reaction { center: Vec2::new(-60.0, 20.0), kind: ReactionFx::Shatter });
        reactions.write(crate::messages::Reaction { center: Vec2::new(70.0, -30.0), kind: ReactionFx::Flare });
    }
}

/// Screenshot-only: keep the player invulnerable so a late capture shows a
/// populated mid-game scene rather than an early GameOver. Env-gated, so it
/// never runs in normal play.
fn keep_player_alive(mut commands: Commands, q: Query<Entity, With<Ship>>) {
    for e in &q {
        commands.entity(e).insert(Invulnerable { seconds: 999.0 });
    }
}

/// Screenshot-only: hold the fire button so the capture shows player bullets,
/// kills, explosions, and dropped orbs. Env-gated.
fn force_fire(mut q: Query<&mut Intent, With<Ship>>) {
    for mut intent in &mut q {
        intent.firing = true;
    }
}
