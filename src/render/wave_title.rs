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
                TextColor(Color::srgb(0.7, 0.85, 1.0)),
            ));
        });
}

/// Count down each banner and despawn it (with its child Text) at end of life.
pub fn tick_wave_title(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut WaveTitle)>,
) {
    for (e, mut t) in &mut q {
        t.life -= time.delta_secs();
        if t.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
