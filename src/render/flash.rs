//! Screen flash (spec I.2) — the companion to camera shake (`render::shake`).
//!
//! A fading full-screen white overlay (a camera-child sprite, so it covers the
//! view and rides the shake offset) flashes on impactful events — currently
//! boss-rage activation (`Added<Raged>`, spec IV.7 "screen flash 0.42"). Purely
//! additive presentation: `ScreenFlash::add` keeps the stronger trigger and
//! `apply_screen_flash` drives the overlay alpha + decays it. Reusable — other
//! events can call `ScreenFlash::add`. (Spec's separate gold channel deferred.)

use crate::components::Raged;
use bevy::prelude::*;

/// Current flash alpha (0..1). `init_resource` in app.rs.
#[derive(Resource, Default)]
pub struct ScreenFlash {
    pub intensity: f32,
}

impl ScreenFlash {
    /// Trigger a flash, keeping the stronger of current/new (clamped to 1).
    pub fn add(&mut self, alpha: f32) {
        self.intensity = self.intensity.max(alpha).min(1.0);
    }
}

/// Marks the full-screen flash overlay sprite (a camera child).
#[derive(Component)]
pub struct FlashOverlay;

/// Rage-activation flash alpha (spec IV.7 "screen flash 0.42").
pub const RAGE_FLASH: f32 = 0.42;
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
        flash.add(RAGE_FLASH);
    }
}

/// Drive the overlay alpha from the flash intensity, then decay (spec I.2).
pub fn apply_screen_flash(
    time: Res<Time>,
    mut flash: ResMut<ScreenFlash>,
    mut overlay: Query<&mut Sprite, With<FlashOverlay>>,
) {
    if let Ok(mut sprite) = overlay.single_mut() {
        sprite.color.set_alpha(flash.intensity);
    }
    flash.intensity = (flash.intensity - FLASH_DECAY * time.delta_secs()).max(0.0);
}
