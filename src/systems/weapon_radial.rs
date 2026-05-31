//! Weapon-select radials (port of rainboids `ui/radial-menu.js`): hold **F** for
//! the primary-weapon radial, **E** for the power-weapon radial. The cursor's
//! angle from screen-centre highlights a slice (one per weapon); releasing the
//! key commits that weapon. Cursor in the centre dead-zone → no change (cancel).

use crate::systems::power_weapon::{PowerWeapon, PowerWeaponKind};
use crate::systems::weapons::{CurrentWeapon, WeaponKind};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Primary,
    Power,
}

/// Which radial is open + the slice the cursor last hovered (committed on release).
#[derive(Resource, Default)]
pub struct WeaponRadial {
    active: Option<Slot>,
    hovered: Option<usize>,
}

/// Outer ring radius as a fraction of min(screen w, h); dead-zone as a fraction
/// of the outer radius.
const OUTER_FRAC: f32 = 0.30;
const INNER_FRAC: f32 = 0.38;

/// The slice index under a cursor `offset` from the radial centre (screen coords,
/// +Y down): 0 at the top, increasing clockwise; `None` inside the `inner`
/// dead-zone (a cancel). Pure, so the angle math is unit-testable.
pub fn hovered_slice(offset: Vec2, inner: f32, n: usize) -> Option<usize> {
    if n == 0 || offset.length() < inner {
        return None;
    }
    let two_pi = std::f32::consts::TAU;
    let ang = offset.x.atan2(-offset.y).rem_euclid(two_pi); // 0 = up, +clockwise
    Some(((ang / two_pi) * n as f32).floor() as usize % n)
}

pub fn weapon_radial_ui(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut radial: ResMut<WeaponRadial>,
    mut primary: ResMut<CurrentWeapon>,
    mut power: ResMut<PowerWeapon>,
) -> Result {
    // Which radial is held this frame? (F = primary, E = power.)
    let slot = if keys.pressed(KeyCode::KeyF) {
        Some(Slot::Primary)
    } else if keys.pressed(KeyCode::KeyE) {
        Some(Slot::Power)
    } else {
        None
    };

    // Release edge: it was open, now it isn't → commit the hovered pick.
    let Some(slot) = slot else {
        if let (Some(active), Some(idx)) = (radial.active, radial.hovered) {
            match active {
                Slot::Primary => primary.0 = WeaponKind::ALL[idx],
                Slot::Power => power.kind = PowerWeaponKind::ALL[idx],
            }
        }
        radial.active = None;
        radial.hovered = None;
        return Ok(());
    };
    radial.active = Some(slot);

    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let center = screen.center();
    let outer = screen.width().min(screen.height()) * OUTER_FRAC;
    let inner = outer * INNER_FRAC;

    let (header, labels): (&str, Vec<&'static str>) = match slot {
        Slot::Primary => ("PRIMARY", WeaponKind::ALL.iter().map(|w| w.name()).collect()),
        Slot::Power => ("POWER", PowerWeaponKind::ALL.iter().map(|w| w.name()).collect()),
    };
    let n = labels.len();
    let two_pi = std::f32::consts::TAU;

    // Cursor angle (0 at top, clockwise) → hovered slice; dead-zone cancels.
    let ptr = ctx.input(|i| i.pointer.latest_pos());
    let hovered =
        ptr.and_then(|p| hovered_slice(Vec2::new(p.x - center.x, p.y - center.y), inner, n));
    radial.hovered = hovered;

    // ── Draw the overlay ──
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("weapon_radial"),
    ));
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(150));

    // A point on the ring at angle `a` (0 = top, clockwise) and radius `r`.
    let ring = |a: f32, r: f32| egui::pos2(center.x + a.sin() * r, center.y - a.cos() * r);

    for i in 0..n {
        let a0 = i as f32 / n as f32 * two_pi;
        let a1 = (i + 1) as f32 / n as f32 * two_pi;
        let steps = 6;
        let mut pts = Vec::with_capacity((steps + 1) * 2);
        for s in 0..=steps {
            pts.push(ring(a0 + (a1 - a0) * s as f32 / steps as f32, outer));
        }
        for s in (0..=steps).rev() {
            pts.push(ring(a0 + (a1 - a0) * s as f32 / steps as f32, inner));
        }
        let hot = hovered == Some(i);
        let fill = if hot {
            egui::Color32::from_rgba_unmultiplied(70, 150, 255, 200)
        } else {
            egui::Color32::from_rgba_unmultiplied(20, 24, 36, 220)
        };
        painter.add(egui::Shape::convex_polygon(
            pts,
            fill,
            egui::Stroke::new(if hot { 2.5 } else { 1.0 }, egui::Color32::WHITE),
        ));
        let mid = (a0 + a1) * 0.5;
        painter.text(
            ring(mid, (inner + outer) * 0.5),
            egui::Align2::CENTER_CENTER,
            labels[i],
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
    }

    // Hub: header + the currently-hovered weapon name.
    painter.circle_filled(center, inner * 0.9, egui::Color32::from_black_alpha(210));
    painter.text(
        center - egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        header,
        egui::FontId::proportional(16.0),
        egui::Color32::from_rgb(150, 200, 255),
    );
    if let Some(idx) = hovered {
        painter.text(
            center + egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            labels[idx],
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hovered_slice;
    use bevy::prelude::Vec2;

    #[test]
    fn slices_go_clockwise_from_the_top() {
        // 4 slices: top, right, bottom, left (screen coords, +Y down).
        assert_eq!(hovered_slice(Vec2::new(0.0, -50.0), 20.0, 4), Some(0)); // up
        assert_eq!(hovered_slice(Vec2::new(50.0, 0.0), 20.0, 4), Some(1)); // right
        assert_eq!(hovered_slice(Vec2::new(0.0, 50.0), 20.0, 4), Some(2)); // down
        assert_eq!(hovered_slice(Vec2::new(-50.0, 0.0), 20.0, 4), Some(3)); // left
    }

    #[test]
    fn dead_zone_cancels() {
        assert_eq!(hovered_slice(Vec2::new(5.0, 0.0), 20.0, 4), None);
        assert_eq!(hovered_slice(Vec2::new(50.0, 0.0), 20.0, 0), None);
    }
}
