//! Screen flash (spec I.2) — the companion to camera shake (`render::shake`).
//!
//! A fading full-screen overlay (a camera-child sprite, so it covers the view
//! and rides the shake offset) flashes on impactful events: a **white** flash on
//! boss-rage activation (`Added<Raged>`, spec IV.7 "screen flash 0.42") and a
//! **gold** flash on a Last Stand cheat-death (spec I.2 gold channel). Purely
//! additive presentation: `ScreenFlash::add` keeps the stronger trigger (whose
//! color wins) and `apply_screen_flash` drives the overlay color + alpha + decay.

use crate::components::Raged;
use crate::messages::PlayerHurt;
use crate::resources::LastStandUsed;
use bevy::prelude::*;

/// Current flash alpha (0..1) + the color of the active flash. `init_resource`.
#[derive(Resource)]
pub struct ScreenFlash {
    pub intensity: f32,
    /// RGB of the active flash (alpha comes from `intensity`).
    pub color: Color,
}

impl Default for ScreenFlash {
    fn default() -> Self {
        Self { intensity: 0.0, color: Color::WHITE }
    }
}

impl ScreenFlash {
    /// Trigger a `color` flash at `alpha`, keeping the stronger of current/new
    /// (clamped to 1); the stronger trigger's color wins the overlay tint.
    pub fn add(&mut self, color: Color, alpha: f32) {
        if alpha >= self.intensity {
            self.color = color;
        }
        self.intensity = self.intensity.max(alpha).min(1.0);
    }
}

/// Marks the full-screen flash overlay sprite (a camera child).
#[derive(Component)]
pub struct FlashOverlay;

/// Rage-activation flash alpha (spec IV.7 "screen flash 0.42").
pub const RAGE_FLASH: f32 = 0.42;
/// Last Stand cheat-death flash alpha — a strong gold pop for the dramatic beat.
pub const LAST_STAND_FLASH: f32 = 0.7;
/// Gold tint for the Last Stand flash (HDR-ish warm gold).
pub const FLASH_GOLD: Color = Color::srgb(1.0, 0.82, 0.25);
/// Player-hurt flash alpha — modest so chip damage doesn't strobe.
pub const HURT_FLASH: f32 = 0.22;
/// Red tint for the player-hurt flash.
pub const FLASH_HURT: Color = Color::srgb(1.0, 0.15, 0.15);
/// Flash decay (alpha/sec) — a 0.42 flash fades in ~0.2 s.
const FLASH_DECAY: f32 = 2.2;
/// Overlay edge length (px) — generously larger than any window so it always
/// covers the view even with the shake offset.
const OVERLAY_SIZE: f32 = 6000.0;

/// Spawn the flash overlay as a camera child (covers the view, rides the shake).
/// Registered in Startup `.after(spawn_camera)` so the camera exists.
pub fn setup_screen_flash(mut commands: Commands, camera: Query<Entity, With<Camera2d>>) {
    let Ok(cam) = camera.single() else {
        return;
    };
    commands.entity(cam).with_children(|c| {
        c.spawn((
            FlashOverlay,
            Sprite::from_color(
                Color::linear_rgba(1.0, 1.0, 1.0, 0.0),
                Vec2::splat(OVERLAY_SIZE),
            ),
            // In front of all gameplay (z 0..~100); the HUD UI still draws on top.
            Transform::from_xyz(0.0, 0.0, 100.0),
        ));
    });
}

/// Flash white when a boss enters rage (`Added<Raged>`, spec IV.7).
pub fn trigger_screen_flash(mut flash: ResMut<ScreenFlash>, raged: Query<Entity, Added<Raged>>) {
    if !raged.is_empty() {
        flash.add(Color::WHITE, RAGE_FLASH);
    }
}

/// Flash **gold** when Last Stand cheats death (spec I.2/III.5 gold channel) —
/// detects the `LastStandUsed` false→true transition (it's spent once per run).
pub fn trigger_last_stand_flash(
    last_stand: Res<LastStandUsed>,
    mut flash: ResMut<ScreenFlash>,
    mut prev: Local<bool>,
) {
    if last_stand.0 && !*prev {
        flash.add(FLASH_GOLD, LAST_STAND_FLASH);
    }
    *prev = last_stand.0;
}

/// Flash **red** when the player takes a hit (`PlayerHurt`) — a brief damage
/// vignette (graphical parity). Modest alpha so frequent chip-damage doesn't
/// strobe; the stronger rage/Last-Stand flashes still win via `add`.
pub fn trigger_player_hurt_flash(
    mut flash: ResMut<ScreenFlash>,
    mut hurt: MessageReader<PlayerHurt>,
) {
    if hurt.read().count() > 0 {
        flash.add(FLASH_HURT, HURT_FLASH);
    }
}

/// HP fraction below which the low-HP warning pulse kicks in.
pub const LOW_HP_FRAC: f32 = 0.25;

/// While the player is critically hurt (< [`LOW_HP_FRAC`] HP), pulse a faint red
/// edge each frame (re-added so it doesn't decay away) — a danger heartbeat.
pub fn low_hp_warning(
    time: Res<Time>,
    mut flash: ResMut<ScreenFlash>,
    player: Query<&crate::components::Health, With<crate::components::Ship>>,
) {
    let Ok(hp) = player.single() else {
        return;
    };
    if hp.max > 0.0 && hp.current > 0.0 && hp.current < hp.max * LOW_HP_FRAC {
        // 0.06..0.16 sinusoidal throb (kept below the hit-flash so a real hit
        // still reads stronger).
        let pulse = 0.06 + 0.10 * (time.elapsed_secs() * 7.0).sin().abs();
        flash.add(FLASH_HURT, pulse);
    }
}

/// Drive the overlay color + alpha from the flash, then decay (spec I.2).
pub fn apply_screen_flash(
    time: Res<Time>,
    mut flash: ResMut<ScreenFlash>,
    mut overlay: Query<&mut Sprite, With<FlashOverlay>>,
) {
    if let Ok(mut sprite) = overlay.single_mut() {
        sprite.color = flash.color.with_alpha(flash.intensity);
    }
    flash.intensity = (flash.intensity - FLASH_DECAY * time.delta_secs()).max(0.0);
}
