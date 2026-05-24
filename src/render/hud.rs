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

/// One of the three triforce spare-tank glyphs (spec VIII.1); `0` is the first
/// spare. Lit gold when `index < lives.count`, else dimmed.
#[derive(Component, Clone, Copy)]
pub struct TankGlyph(pub u8);

const BAR_W: f32 = 220.0;
const BAR_H: f32 = 24.0;

/// Spare-tank glyph color: gold `#FFD700` when that spare is filled, else a dim
/// gold so the empty slots still read as a 3-tank track (spec VIII.1).
pub fn tank_glyph_color(index: u8, tanks: u32) -> Color {
    if (index as u32) < tanks {
        Color::srgb(1.0, 0.843, 0.0) // #FFD700
    } else {
        Color::srgb(0.28, 0.24, 0.08) // dim gold
    }
}

/// Marks the power-weapon **energy sphere** (spec VIII.1) — a circular node that
/// fills (brightens, teal) toward the active power weapon's cost and pulses gold
/// when it's chargeable.
#[derive(Component)]
pub struct EnergyOrb;

/// Energy-sphere color: a gold pulse when `ready` (energy ≥ cost & off cooldown),
/// else teal whose brightness tracks `frac` (energy / cost, 0..1). `t` drives the
/// ready-pulse.
pub fn energy_orb_color(frac: f32, ready: bool, t: f32) -> Color {
    if ready {
        let p = 0.7 + 0.3 * (t * 6.0).sin().abs(); // 0.7..1.0 gold throb
        Color::srgb(1.0 * p, 0.84 * p, 0.2 * p)
    } else {
        let b = 0.18 + 0.62 * frac.clamp(0.0, 1.0); // dim → bright as it charges
        Color::srgb(0.12 * b, 0.62 * b, 0.92 * b) // teal
    }
}

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

    // Triforce spare-tank glyphs — a row of 3 just right of the health bar.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(BAR_W + 24.0),
            column_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..3u8 {
                row.spawn((
                    TankGlyph(i),
                    Text::new("▲"),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(tank_glyph_color(i, 0)),
                ));
            }
        });

    // Power-weapon energy sphere — a circle (border-radius 50%) right of the triforce.
    commands.spawn((
        EnergyOrb,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(BAR_W + 96.0),
            width: Val::Px(28.0),
            height: Val::Px(28.0),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            ..default()
        },
        BackgroundColor(energy_orb_color(0.0, false, 0.0)),
    ));

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
    time: Res<Time>,
    mut texts: Query<(&mut Text, &HudText)>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), With<HealthBarFill>>,
    mut tank_glyphs: Query<(&TankGlyph, &mut TextColor)>,
    mut energy_orb: Query<&mut BackgroundColor, (With<EnergyOrb>, Without<HealthBarFill>)>,
) {
    let player = player.single().ok();

    // Triforce spare-tank glyphs.
    let tanks = player.and_then(|(_, _, l)| l).map(|l| l.count).unwrap_or(0);
    for (glyph, mut color) in &mut tank_glyphs {
        color.0 = tank_glyph_color(glyph.0, tanks);
    }

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

    // Energy sphere: fills (brightens) toward the cost, gold-pulses when ready.
    if let Ok(mut orb) = energy_orb.single_mut() {
        let frac = if cost > 0.0 { energy.current / cost } else { 1.0 };
        orb.0 = energy_orb_color(frac, ready, time.elapsed_secs());
    }

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
                // Spare tanks are now shown by the triforce glyphs (above).
                format!(
                    "SHIELD {shield_pct}%\n\
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
