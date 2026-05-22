//! Minimap (spec VIII.1, simplified): a small corner panel with a cyan player
//! dot and red enemy dots. Placed bottom-left (the spec's top-left is taken by
//! the health cluster in this port). Presentation-only.
//!
//! Dots are rebuilt each frame as children of the panel (so their absolute
//! positions are panel-relative). `world_to_minimap` — the world→panel mapping —
//! is the unit-tested core; the dot rendering itself is visual.

use crate::components::{Enemy, Ship};
use crate::resources::PlayBounds;
use bevy::prelude::*;

const MAP_SIZE: f32 = 150.0;
const MAP_MARGIN: f32 = 12.0;
/// Cap on enemy dots drawn (bounds the per-frame rebuild).
const MAX_DOTS: usize = 40;

#[derive(Component)]
pub struct MinimapPanel;
#[derive(Component)]
pub struct MinimapDot;

/// Map a world position to minimap-local pixels in `[0, size]`. UI y is
/// top-down, so world +Y maps to a smaller y. Out-of-arena positions clamp in.
pub fn world_to_minimap(world: Vec2, half: Vec2, size: f32) -> Vec2 {
    let nx = ((world.x + half.x) / (2.0 * half.x)).clamp(0.0, 1.0);
    let ny = ((half.y - world.y) / (2.0 * half.y)).clamp(0.0, 1.0);
    Vec2::new(nx * size, ny * size)
}

/// Spawn the minimap panel once (bottom-left, dim border box).
pub fn setup_minimap(mut commands: Commands) {
    commands.spawn((
        MinimapPanel,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(MAP_MARGIN),
            left: Val::Px(MAP_MARGIN),
            width: Val::Px(MAP_SIZE),
            height: Val::Px(MAP_SIZE),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.1, 0.15, 0.4)),
    ));
}

fn dot_node(m: Vec2, sz: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(m.x - sz * 0.5),
        top: Val::Px(m.y - sz * 0.5),
        width: Val::Px(sz),
        height: Val::Px(sz),
        ..default()
    }
}

/// Rebuild the minimap dots each frame: red for enemies, a brighter cyan for the
/// player.
pub fn update_minimap(
    mut commands: Commands,
    bounds: Res<PlayBounds>,
    panel: Query<Entity, With<MinimapPanel>>,
    dots: Query<Entity, With<MinimapDot>>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<&Transform, With<Enemy>>,
) {
    let Ok(panel) = panel.single() else {
        return;
    };
    for d in &dots {
        commands.entity(d).despawn();
    }
    let half = bounds.half;
    commands.entity(panel).with_children(|p| {
        for etf in enemies.iter().take(MAX_DOTS) {
            let m = world_to_minimap(etf.translation.truncate(), half, MAP_SIZE);
            p.spawn((
                MinimapDot,
                dot_node(m, 3.0),
                BackgroundColor(Color::srgb(1.0, 0.3, 0.3)),
            ));
        }
        if let Ok(ptf) = player.single() {
            let m = world_to_minimap(ptf.translation.truncate(), half, MAP_SIZE);
            p.spawn((
                MinimapDot,
                dot_node(m, 4.0),
                BackgroundColor(Color::srgb(0.3, 0.9, 1.0)),
            ));
        }
    });
}
