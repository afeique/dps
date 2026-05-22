//! Game-flow systems: the screens and transitions at the edges of `Playing`.
//!
//! State graph (`states::GameState`): `Title` (boot) → `Playing` → `GameOver`
//! (death) or `GameComplete` (all 30 waves cleared) → back to `Title`. Each
//! non-playing state shows a centered overlay (spawned `OnEnter`, despawned
//! `OnExit`); `Paused` is reserved but not yet wired.
//!
//! A fresh run resets the run-scoped resources via `reset_run` on
//! `OnEnter(Playing)` (Wave + energy reset by their own OnEnter systems).

use crate::components::{Bullet, Enemy};
use crate::resources::{KillStreak, Score};
use crate::states::GameState;
use crate::systems::wave::Wave;
use bevy::prelude::*;

// ── Screen markers ─────────────────────────────────────────────────────────

#[derive(Component)]
pub struct TitleScreen;
#[derive(Component)]
pub struct GameOverScreen;
#[derive(Component)]
pub struct GameCompleteScreen;

// ── Overlay builder ──────────────────────────────────────────────────────────

/// Spawn a full-screen, dimmed, centered overlay tagged with `marker`, holding a
/// big `title` and a smaller `subtitle`.
fn spawn_overlay<M: Component>(
    commands: &mut Commands,
    marker: M,
    title: &str,
    subtitle: &str,
    title_color: Color,
) {
    commands
        .spawn((
            marker,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.6)),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(title),
                TextFont {
                    font_size: 46.0,
                    ..default()
                },
                TextColor(title_color),
            ));
            p.spawn((
                Text::new(subtitle),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.85, 0.95)),
            ));
        });
}

/// Generic OnExit cleanup: despawn every entity carrying screen-marker `M`.
pub fn despawn_screen<M: Component>(mut commands: Commands, q: Query<Entity, With<M>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ── Title ────────────────────────────────────────────────────────────────────

pub fn enter_title(mut commands: Commands) {
    spawn_overlay(
        &mut commands,
        TitleScreen,
        "DARK PRISM SOLID",
        "Press ENTER to launch",
        Color::srgb(0.55, 0.85, 1.0),
    );
}

/// ENTER / Space on the title screen starts a run.
pub fn title_input(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        next.set(GameState::Playing);
    }
}

// ── New-run reset ──────────────────────────────────────────────────────────

/// Reset run-scoped score + kill streak when a fresh run begins
/// (`OnEnter(Playing)`). The `Wave` and `EnergyMeter` are reset by their own
/// OnEnter(Playing) systems.
pub fn reset_run(mut score: ResMut<Score>, mut streak: ResMut<KillStreak>) {
    *score = Score::default();
    streak.break_streak();
}

// ── Game over ────────────────────────────────────────────────────────────────

/// On entering `GameOver`, clear the field (the ship is already despawned by
/// `apply_damage`) and show the results overlay.
pub fn enter_game_over(
    mut commands: Commands,
    score: Res<Score>,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Bullet>)>>,
) {
    for e in &leftovers {
        commands.entity(e).despawn();
    }
    let subtitle = format!(
        "{} points · {} gold · wave reached\nPress ENTER to return to the title",
        score.points, score.gold
    );
    spawn_overlay(
        &mut commands,
        GameOverScreen,
        "GAME OVER",
        &subtitle,
        Color::srgb(1.0, 0.35, 0.3),
    );
    info!("GAME OVER — press ENTER to return to the title");
}

/// In `GameOver`, ENTER / R returns to the title (a new run starts from there).
pub fn game_over_input(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<GameState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::KeyR) {
        next.set(GameState::Title);
    }
}

// ── Game complete (campaign cleared) ──────────────────────────────────────────

/// While `Playing`, flip to `GameComplete` once the wave driver marks the
/// campaign cleared (wave 30 done — `completed` already implies an empty field).
pub fn check_campaign_complete(wave: Res<Wave>, mut next: ResMut<NextState<GameState>>) {
    if wave.completed {
        next.set(GameState::GameComplete);
    }
}

pub fn enter_game_complete(
    mut commands: Commands,
    score: Res<Score>,
    leftovers: Query<Entity, Or<(With<Enemy>, With<Bullet>)>>,
) {
    for e in &leftovers {
        commands.entity(e).despawn();
    }
    let subtitle = format!(
        "All 30 waves cleared — {} points · {} gold\nPress ENTER for the title",
        score.points, score.gold
    );
    spawn_overlay(
        &mut commands,
        GameCompleteScreen,
        "CAMPAIGN COMPLETE",
        &subtitle,
        Color::srgb(0.5, 1.0, 0.65),
    );
}

/// ENTER on the victory screen returns to the title.
pub fn game_complete_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        next.set(GameState::Title);
    }
}
