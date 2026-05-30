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

use crate::components::loadout::{AbilityCooldowns, EquippedAbilities};
use crate::components::{Boss, Core, CoreShielded, Enemy, Health, Lives, Shield, Ship};
use crate::resources::{EnergyMeter, KillStreak, Score};
use crate::systems::missions::Mission;
use crate::systems::power_weapon::PowerWeapon;
use crate::systems::tower::SelectedTower;
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
    Mission,
    /// Tower-defense build readout: the armed turret (or the build menu).
    Build,
}

/// The Core integrity bar root (top-center, always visible during a run).
#[derive(Component)]
pub struct CoreBarRoot;

/// The Core integrity bar fill; width tracks the Core's HP fraction.
#[derive(Component)]
pub struct CoreBarFill;

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

/// The label text of ability-bar slot `0..4` (the 3-char ability tag, or the
/// slot key digit when empty). Updated by `update_ability_bar`.
#[derive(Component)]
pub struct AbilitySlotLabel(pub usize);

/// The cooldown overlay of ability-bar slot `0..4` — a dark node whose height
/// tracks the slot's remaining-cooldown fraction (full when just cast, gone when
/// ready). Updated by `update_ability_bar`.
#[derive(Component)]
pub struct AbilitySlotCooldown(pub usize);

/// The boss healthbar track (top-center); shown only while a boss is alive.
#[derive(Component)]
pub struct BossBarRoot;

/// The boss healthbar fill; its width tracks the active boss's core HP fraction
/// (cyan while `CoreShielded` = invulnerable, red once exposed). `update_boss_bar`.
#[derive(Component)]
pub struct BossBarFill;

/// Pick the boss the player is fighting from `(hp_frac, core_shielded)` pairs:
/// the lowest-HP-fraction boss (so a freshly-spawned pair shows the weaker one).
/// `None` when no boss is present. Pure, for testing the selection.
pub fn pick_active_boss(bosses: &[(f32, bool)]) -> Option<(f32, bool)> {
    bosses
        .iter()
        .copied()
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
}

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

    // ── Boss core healthbar (top-center) — hidden until a boss is alive ──────
    commands
        .spawn((
            BossBarRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(40.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-200.0)), // center the 400px bar
                width: Val::Px(400.0),
                height: Val::Px(12.0),
                display: Display::None, // update_boss_bar reveals it when fighting a boss
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.04, 0.04, 0.8)),
        ))
        .with_children(|track| {
            track.spawn((
                BossBarFill,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.9, 0.2, 0.2)),
            ));
        });

    // ── Core integrity bar (top-center) — the defense objective's health ─────
    commands
        .spawn((
            CoreBarRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-230.0)), // center the 460px bar
                width: Val::Px(460.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.4, 0.8, 1.0, 0.7)),
            BackgroundColor(Color::srgba(0.02, 0.06, 0.09, 0.85)),
        ))
        .with_children(|track| {
            // Fill (absolute so the centered label sits on top of it).
            track.spawn((
                CoreBarFill,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.2, 0.75, 1.0)),
            ));
            track.spawn((
                Text::new("CORE"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.85, 0.95, 1.0)),
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

    // Mission line, below the status block.
    commands.spawn((
        HudText::Mission,
        Text::new(""),
        font.clone(),
        TextColor(Color::srgb(0.7, 0.95, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(108.0),
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

    // Tower-defense build readout, left column under the mission line.
    commands.spawn((
        HudText::Build,
        Text::new(""),
        font.clone(),
        TextColor(Color::srgb(0.65, 0.95, 0.85)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(140.0),
            left: Val::Px(12.0),
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

    // Ability bar (AB) — a row of 4 slots bottom-left. Each slot is a dark
    // square with a centered tag + a top-anchored cooldown overlay that shrinks
    // as the ability comes off cooldown. Filled live by `update_ability_bar`.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            left: Val::Px(14.0),
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            for i in 0..4usize {
                row.spawn((
                    Node {
                        width: Val::Px(34.0),
                        height: Val::Px(34.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.4, 0.6, 0.9, 0.7)),
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.10, 0.7)),
                ))
                .with_children(|slot| {
                    // Cooldown overlay (absolute, top-anchored, starts empty).
                    slot.spawn((
                        AbilitySlotCooldown(i),
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(0.0),
                            left: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(0.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                    ));
                    // Slot tag.
                    slot.spawn((
                        AbilitySlotLabel(i),
                        Text::new(""),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.8, 0.9, 1.0)),
                    ));
                });
            }
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
    mission: Res<Mission>,
    sel: Res<SelectedTower>,
    enemies: Query<(), With<Enemy>>,
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
                    energy.max.round() as i32,
                    pw.kind.name(),
                    if ready { " READY" } else { "" },
                    cur.0.name(),
                )
            }
            HudText::Econ => format!(
                "WAVE {}\nENEMIES {}\nGOLD {}\nPOINTS {}",
                wave.number(),
                enemies.iter().count(),
                score.gold,
                score.points
            ),
            HudText::Build => {
                let prep = if wave.awaiting_first_wave() {
                    "▶ PREP — build your defense, then ENTER to begin\n"
                } else {
                    ""
                };
                match sel.kind {
                    Some(k) => format!(
                        "{prep}BUILDING {} ({}g)\nclick place · U upgrade · X sell · 0 cancel",
                        k.name(),
                        k.cost()
                    ),
                    None => {
                        use crate::systems::tower::TowerKind;
                        let mut s = format!("{prep}BUILD  ");
                        for (i, k) in TowerKind::ALL.iter().enumerate() {
                            s.push_str(&format!("{} {} ({}g)  ", i + 1, k.name(), k.cost()));
                        }
                        s
                    }
                }
            }
            HudText::Mission => {
                let mark = if mission.done { "  ✓" } else { "" };
                format!("MISSION  {}{mark}", mission.kind.label())
            }
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

/// Drive the boss healthbar: reveal it while a boss is alive, set the fill to the
/// active boss's core HP fraction, and colour it cyan while the core is shielded
/// (invulnerable — clear the parts!) or red once exposed.
pub fn update_boss_bar(
    bosses: Query<(&Health, Has<CoreShielded>), With<Boss>>,
    mut root: Query<&mut Node, (With<BossBarRoot>, Without<BossBarFill>)>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), With<BossBarFill>>,
) {
    let data: Vec<(f32, bool)> = bosses
        .iter()
        .map(|(hp, sh)| ((hp.current / hp.max).clamp(0.0, 1.0), sh))
        .collect();
    let Ok(mut root_node) = root.single_mut() else {
        return;
    };
    match pick_active_boss(&data) {
        Some((frac, shielded)) => {
            root_node.display = Display::Flex;
            if let Ok((mut fnode, mut fcol)) = fill.single_mut() {
                fnode.width = Val::Percent(frac * 100.0);
                fcol.0 = if shielded {
                    Color::srgb(0.4, 0.7, 1.0) // cyan — core invulnerable
                } else {
                    Color::srgb(0.9, 0.2, 0.2) // red — exposed
                };
            }
        }
        None => root_node.display = Display::None,
    }
}

/// Drive the Core integrity bar: width tracks the Core's HP fraction, colour
/// shifts blue → amber → red as it falls. Empties (and the bar reads 0) once the
/// Core is gone (game over follows from `tower::core_lose_check`).
pub fn update_core_bar(
    core: Query<&Health, With<Core>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), With<CoreBarFill>>,
) {
    let frac = core
        .single()
        .map(|hp| (hp.current / hp.max).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    if let Ok((mut node, mut color)) = fill.single_mut() {
        node.width = Val::Percent(frac * 100.0);
        color.0 = if frac > 0.5 {
            Color::srgb(0.2, 0.75, 1.0) // healthy cyan
        } else if frac > 0.25 {
            Color::srgb(1.0, 0.7, 0.2) // amber
        } else {
            Color::srgb(1.0, 0.3, 0.25) // critical red
        };
    }
}

/// Refresh the ability-bar slots (AB): the 3-char tag per equipped ability (or
/// the slot key digit when empty), and the cooldown overlay height from the
/// slot's remaining-cooldown fraction (full just after a cast, gone when ready).
pub fn update_ability_bar(
    equipped: Res<EquippedAbilities>,
    cds: Res<AbilityCooldowns>,
    mut labels: Query<(&AbilitySlotLabel, &mut Text, &mut TextColor)>,
    mut overlays: Query<(&AbilitySlotCooldown, &mut Node)>,
) {
    for (slot, mut text, mut color) in &mut labels {
        match equipped.0.get(slot.0).copied().flatten() {
            Some(a) => {
                let s = a.short();
                if text.0 != s {
                    text.0 = s.to_string();
                }
                color.0 = Color::srgb(0.85, 0.93, 1.0);
            }
            None => {
                let s = format!("{}", slot.0 + 1);
                if text.0 != s {
                    text.0 = s;
                }
                color.0 = Color::srgba(0.45, 0.5, 0.6, 0.6);
            }
        }
    }
    for (slot, mut node) in &mut overlays {
        node.height = Val::Percent(cds.fraction(slot.0) * 100.0);
    }
}
