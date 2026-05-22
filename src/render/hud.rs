//! On-screen HUD (spec VIII.1), built with Bevy UI.
//!
//! A functional core of the web build's HUD: a tier-colored **health bar** with
//! `HP N/max`, a **status** block (shield %, spare tanks, power-weapon energy +
//! active power weapon + readiness, active primary), an **economy** readout
//! (wave / gold / points), and a **kill-streak** banner that appears while the
//! streak buff is live. Deferred (cosmetic / not yet modelled): the triforce
//! tank glyphs, energy sphere, minimap, damage numbers, loot feed, XP bar.
//!
//! It is pure presentation: it reads gameplay resources + the player's
//! components and writes UI `Text` / bar geometry — it never mutates sim state,
//! so it lives in `render` and runs in `Update` (not the FixedUpdate sim).

use crate::components::{Health, Lives, Shield, Ship};
use crate::resources::{EnergyMeter, KillStreak, Score};
use crate::systems::power_weapon::PowerWeapon;
use crate::systems::wave::Wave;
use crate::systems::weapons::CurrentWeapon;
use bevy::prelude::*;

/// Which HUD text line an entity is, so one query can update them all.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HudText {
    Hp,
    Status,
    Econ,
    Streak,
}

/// Marks the health-bar fill node (width % + color updated each frame).
#[derive(Component)]
pub struct HealthBarFill;

const BAR_W: f32 = 220.0;
const BAR_H: f32 = 24.0;

/// Spawn the HUD tree once at startup.
pub fn setup_hud(mut commands: Commands) {
    let font = TextFont {
        font_size: 16.0,
        ..default()
    };

    // ── Health bar (top-left) — dark track + colored fill child ──────────────
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                width: Val::Px(BAR_W),
                height: Val::Px(BAR_H),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.7)),
        ))
        .with_children(|track| {
            track.spawn((
                HealthBarFill,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.2, 0.6, 1.0)),
            ));
        });

    // HP text, overlaid on the bar.
    commands.spawn((
        HudText::Hp,
        Text::new("40/40"),
        font.clone(),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(15.0),
            left: Val::Px(20.0),
            ..default()
        },
    ));

    // Status block, just below the bar.
    commands.spawn((
        HudText::Status,
        Text::new(""),
        font.clone(),
        TextColor(Color::srgb(0.8, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(44.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));

    // Economy readout, top-right.
    commands.spawn((
        HudText::Econ,
        Text::new(""),
        font.clone(),
        TextColor(Color::srgb(1.0, 0.85, 0.3)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(14.0),
            ..default()
        },
    ));

    // Kill-streak banner, bottom-center (full-width container, centered child).
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(36.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                HudText::Streak,
                Text::new(""),
                font.clone(),
                TextColor(Color::srgb(1.0, 0.7, 0.2)),
            ));
        });
}

/// Refresh every HUD element from the current game state each frame.
#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    player: Query<(&Health, Option<&Shield>, Option<&Lives>), With<Ship>>,
    energy: Res<EnergyMeter>,
    pw: Res<PowerWeapon>,
    cur: Res<CurrentWeapon>,
    score: Res<Score>,
    streak: Res<KillStreak>,
    wave: Res<Wave>,
    mut texts: Query<(&mut Text, &HudText)>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), With<HealthBarFill>>,
) {
    let player = player.single().ok();

    // Health fraction + bar.
    let frac = player
        .map(|(hp, _, _)| (hp.current / hp.max).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    if let Ok((mut node, mut color)) = bar.single_mut() {
        node.width = Val::Percent(frac * 100.0);
        color.0 = if frac > 0.6 {
            Color::srgb(0.2, 0.6, 1.0) // blue
        } else if frac > 0.3 {
            Color::srgb(1.0, 0.85, 0.2) // yellow
        } else {
            Color::srgb(1.0, 0.25, 0.2) // red
        };
    }

    let cost = pw.kind.energy_cost();
    let ready = pw.cooldown <= 0.0 && energy.current >= cost;

    for (mut text, kind) in &mut texts {
        let s = match kind {
            HudText::Hp => match player {
                Some((hp, _, _)) => format!("{}/{}", hp.current.round() as i32, hp.max as i32),
                None => "—".to_string(),
            },
            HudText::Status => {
                let shield_pct = player
                    .and_then(|(_, sh, _)| sh)
                    .map(|s| (s.reduction * 100.0).round() as i32)
                    .unwrap_or(0);
                let tanks = player.and_then(|(_, _, l)| l).map(|l| l.count).unwrap_or(0);
                format!(
                    "SHIELD {shield_pct}%   TANKS x{tanks}\n\
                     ENERGY {}/{}  [{}]{}\n\
                     WEAPON {}",
                    energy.current.round() as i32,
                    crate::resources::ENERGY_MAX as i32,
                    pw.kind.name(),
                    if ready { " READY" } else { "" },
                    cur.0.name(),
                )
            }
            HudText::Econ => format!(
                "WAVE {}\nGOLD {}\nPOINTS {}",
                wave.number(),
                score.gold,
                score.points
            ),
            HudText::Streak => {
                if streak.timer > 0.0 && streak.kills >= 10 {
                    let gold_pct = ((streak.gold_multiplier() - 1.0) * 100.0).round() as i32;
                    format!(
                        "{} KILLS   {:.2}x   +{}% GOLD",
                        streak.kills,
                        streak.multiplier(),
                        gold_pct
                    )
                } else {
                    String::new()
                }
            }
        };
        if text.0 != s {
            text.0 = s;
        }
    }
}
