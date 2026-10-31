use crate::combat::Shoot;
use crate::{Library, PlayerShip, Side};
use bevy::prelude::*;

/// `--script shells --shot <png> --shot-frame N`: at frame 20 one shell of every bullet type is released in a row ahead of the
/// player's ship, flying slowly along its heading so a screenshot (any `--dist/--yaw/--pitch`) shows all their effects side by
/// side. Visual check only; nothing is asserted. The shells are the player's own and start well clear of any ship.
pub(super) fn shells(cfg: Res<crate::Config>, lib: Res<Library>, player: Query<&Transform, With<PlayerShip>>, mut shoot: MessageWriter<Shoot>) {
    if cfg.script != Some(super::Script::Shells) || cfg.frame != 20 {
        return;
    }
    let Ok(tf) = player.single() else { return };
    let mut bullets: Vec<&String> = lib.bullets.keys().filter(|b| !b.contains("Mine")).collect();
    bullets.sort();
    let (forward, right) = (*tf.forward(), *tf.right());
    // Row 0 then row 1, left to right as the ship sees them.
    info!("shells: {}", bullets.iter().map(|b| b.trim_start_matches("Bullet_")).collect::<Vec<_>>().join(", "));
    for (i, bullet) in bullets.into_iter().enumerate() {
        // Two rows of up to 12 shells, 5 units apart.
        let (col, row) = ((i % 12) as f32 - 5.5, (i / 12) as f32);
        shoot.write(Shoot {
            bullet: bullet.clone(),
            from: tf.translation + forward * (22.0 + 9.0 * row) + right * (col * 5.0),
            vel: forward * 6.0,
            side: Side::Player,
            shooter: Entity::PLACEHOLDER,
            target: None,
            gravity: 0.0,
            aim: None,
            ricochet: 0.0,
        });
    }
}
