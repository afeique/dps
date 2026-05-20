//! Dev/CI screenshot hook. With `DPS_SCREENSHOT=<path>` set, the app grabs the
//! primary window's framebuffer ~1 s in, writes a PNG, and exits — no OS
//! screen-recording permission needed (it reads the GPU framebuffer, not the
//! display). Doubles as the `docs/port-plan.md` §9 "screenshot diff vs the web
//! build" capture. A no-op when the env var is unset, so normal runs are
//! unaffected.

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
    if !plan.taken && *frame >= 60 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(plan.path.clone()));
        plan.taken = true;
        info!("DPS_SCREENSHOT: captured frame -> {}", plan.path);
    }
    if *frame >= 150 {
        info!("DPS_SCREENSHOT: exiting");
        std::process::exit(0);
    }
}
