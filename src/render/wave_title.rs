//! Wave-start title overlay (spec V.6 announcements, simplified): a brief
//! "STAGE N-M" banner each time the wave number changes. The `stage_label`
//! mapping is the unit-tested core; the banner itself is presentation (shown for
//! `TITLE_LIFE` seconds then despawned — no fade, the per-wave subtitle flavor
//! text is deferred).

use crate::systems::wave::Wave;
use bevy::prelude::*;

/// Marks a live wave-title banner (the container; its Text is a child).
#[derive(Component)]
pub struct WaveTitle {
    life: f32,
}

const TITLE_LIFE: f32 = 2.8;

/// Banner opacity over its life (counts down from `total` to 0): ramp up over the
/// first `FADE_IN` s, hold at full, then ramp down over the last `FADE_OUT` s — so
/// the stage banner / toast eases in and out instead of popping. Pure + tested.
pub fn banner_alpha(life: f32, total: f32) -> f32 {
    const FADE_IN: f32 = 0.3;
    const FADE_OUT: f32 = 0.55;
    let elapsed = total - life;
    let fin = (elapsed / FADE_IN).clamp(0.0, 1.0);
    let fout = (life / FADE_OUT).clamp(0.0, 1.0);
    fin.min(fout)
}

/// Map a 1-based wave number to its "stage-substage" label: 3 waves per stage,
/// so W1→"1-1", W3→"1-3", W4→"2-1", W30→"10-3" (spec V — 10 stages × 3).
pub fn stage_label(wave: u64) -> String {
    let w = wave.max(1) - 1;
    format!("{}-{}", w / 3 + 1, w % 3 + 1)
}

/// When the wave number changes (while playing), pop a fresh "STAGE N-M" banner.
pub fn show_wave_title(
    mut commands: Commands,
    wave: Res<Wave>,
    mut last: Local<usize>,
    existing: Query<Entity, With<WaveTitle>>,
) {
    let n = wave.number();
    if n == *last {
        return;
    }
    *last = n;
    for e in &existing {
        commands.entity(e).despawn();
    }
    commands
        .spawn((
            WaveTitle { life: TITLE_LIFE },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(22.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(format!("STAGE {}", stage_label(n as u64))),
                TextFont {
                    font_size: 44.0,
                    ..default()
                },
                // Start transparent; `tick_wave_title` fades it in (no 1-frame pop).
                TextColor(Color::srgb(0.7, 0.85, 1.0).with_alpha(0.0)),
            ));
        });
}

/// Count down each banner, ease its child Text's opacity in/out
/// ([`banner_alpha`]), and despawn it (with its child Text) at end of life.
pub fn tick_wave_title(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut WaveTitle, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    for (e, mut t, children) in &mut q {
        t.life -= time.delta_secs();
        if t.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let a = banner_alpha(t.life, TITLE_LIFE);
        for &child in children {
            if let Ok(mut tc) = texts.get_mut(child) {
                tc.0 = tc.0.with_alpha(a);
            }
        }
    }
}

// ── Pulse-phase toast (spec V.6: "pulse phase toast for P>0, 1600 ms") ──────────

/// Marks a live reinforcement (later-pulse) toast.
#[derive(Component)]
pub struct PulseToast {
    life: f32,
}

const PULSE_TOAST_LIFE: f32 = 1.6;

/// Whether a pulse advance from `prev`→`cur` spawned pulses should toast: a *new*
/// pulse (`cur > prev`) that isn't pulse 0 (`cur >= 2`, since `spawned_pulses` is
/// 1 after pulse 0). The pure core of `show_pulse_toast`.
pub fn should_toast_pulse(prev: usize, cur: usize) -> bool {
    cur > prev && cur >= 2
}

/// Pop a brief "REINFORCEMENTS" toast when a wave's later pulse (P>0) spawns.
pub fn show_pulse_toast(
    mut commands: Commands,
    wave: Res<Wave>,
    mut last_wave: Local<usize>,
    mut last_pulses: Local<usize>,
    existing: Query<Entity, With<PulseToast>>,
) {
    let n = wave.number();
    let p = wave.pulses_spawned();
    // On wave change, sync without toasting (don't announce pulse 0 of a new wave).
    if n != *last_wave {
        *last_wave = n;
        *last_pulses = p;
        return;
    }
    let toast = should_toast_pulse(*last_pulses, p);
    *last_pulses = p;
    if !toast {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    commands
        .spawn((
            PulseToast {
                life: PULSE_TOAST_LIFE,
            },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(34.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("REINFORCEMENTS"),
                TextFont {
                    font_size: 26.0,
                    ..default()
                },
                // Start transparent; faded in by `tick_pulse_toast`.
                TextColor(Color::srgb(1.0, 0.6, 0.25).with_alpha(0.0)),
            ));
        });
}

/// Count down pulse toasts, ease their opacity in/out ([`banner_alpha`]), and
/// despawn at end of life.
pub fn tick_pulse_toast(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut PulseToast, &Children)>,
    mut texts: Query<&mut TextColor>,
) {
    for (e, mut t, children) in &mut q {
        t.life -= time.delta_secs();
        if t.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let a = banner_alpha(t.life, PULSE_TOAST_LIFE);
        for &child in children {
            if let Ok(mut tc) = texts.get_mut(child) {
                tc.0 = tc.0.with_alpha(a);
            }
        }
    }
}
