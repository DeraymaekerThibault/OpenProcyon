//! Mines a map places before the battle starts (Border Dispute, Red Rover): the same homing mines a minelayer drops, resting where
//! the map put them. The map script hands each field to a player (`Set Group/Unit Owner`), which sets the side they fight for.
use super::*;

/// A mine the map placed (a `Bullet_StarMine_*` world object); `id` is its world-object id, which the script's groups name.
#[derive(Component)]
pub struct MapMine {
    pub id: i64,
    pub bullet: String,
    /// The player the script gave it to; empty until then.
    pub owner: String,
    /// The side that owner fights for, once the script has given it one.
    pub side: Option<Side>,
}

impl MapMine {
    pub fn unit(&self, e: Entity, tf: &Transform) -> crate::rules::Unit {
        crate::rules::Unit {
            e,
            id: self.id,
            name: self.bullet.clone(),
            owner: self.owner.clone(),
            owner_is_named_group: false,
            groups: vec![],
            pos: tf.translation,
            alive: true,
            visible: true,
            primary: false,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        }
    }
}

/// Turns each new map mine into a resting mine shell. Until the script names an owner it is hostile to every ship.
pub(super) fn arm(mut commands: Commands, assets: Res<AssetServer>, lib: Res<Library>, fxl: Res<FxLib>, mines: Query<(Entity, &MapMine, &Transform), Added<MapMine>>) {
    for (e, mine, tf) in &mines {
        let Some(b) = lib.bullets.get(&mine.bullet) else { continue };
        commands.entity(e).insert((
            Shell {
                vel: Vec3::ZERO,
                age: 0.0,
                lifetime: b.lifetime,
                gravity: 0.0,
                bullet: mine.bullet.clone(),
                side: mine.side.unwrap_or(crate::scenario::HOSTILE_TO_ALL),
                shooter: e,
                target: None,
                angular_velocity: 0.0,
                payload: b.payload,
                aim: Some(tf.translation),
                pd: false,
                hp: b.hitpoints,
                harmless: false,
                base: false,
                ricochet: 0.0,
            },
            Transient,
        ));
        if let Some((name, vol, max)) = &b.travel_snd {
            commands.entity(e).insert(crate::audio::Travel { name: name.clone(), vol: *vol, max: *max });
        }
        if let Some(glb) = &b.glb {
            commands.entity(e).insert(crate::data::scene(&assets, glb));
        }
        if let Some(n) = &b.bullet_fx {
            fx::spawn(&mut commands, &fxl, n, Some(e), Transform::default(), *tf, false);
        }
    }
}
