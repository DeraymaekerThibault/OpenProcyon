//! Space dragons (missions 6-8, docs/re/dragons.md): `Animal_SpaceDragon` placements from the mission JSON, spawned as space
//! animals (so the rules' paths, owners, visibility and volumes work on them) plus a `Dragon` brain: stance, attack orders,
//! damage threshold (past it the dragon breaks off to its border-zone return point), border zone, fire breath and roars.
//! Data: `environment.dragon` (Animal_SpaceDragon.wot and Gun_DragonBreath.wot via tools/export_environment.py).
//! PROVISIONAL: the original's dragon behaviour is unknown; the chase/stand-off/flee behaviour below is the remake's.
use bevy::prelude::*;
use serde_json::Value;

use crate::campaign::{Mission, MissionShip};
use crate::combat::Health;
use crate::environment::SpaceAnimal;
use crate::rules::Stance;
use crate::{Scenario, Ship, Side, game_to_bevy};

#[derive(Component)]
pub struct Dragon {
    pub stance: Stance,
    /// 'Dragon - Set Damage Threshold': fraction of its strength lost at which it breaks off (0: never).
    pub threshold: f32,
    pub border: Option<String>,
    pub home: Option<Vec3>,
    pub attack: Vec<String>,
    pub hold: bool,
    pub hp: f32,
    max_hp: f32,
    fleeing: bool,
    /// Last ship that hurt it and when (neutral/defensive dragons answer attacks).
    pub provoked: Option<(Entity, f32)>,
    cooldown: f32,
    roar_in: f32,
    roars: usize,
    flap: f32,
}
impl Dragon {
    /// A shell or blast struck it (combat.rs).
    pub fn hurt(&mut self, amount: f32, by: Entity, now: f32) {
        self.hp = (self.hp - amount).max(0.0);
        self.provoked = Some((by, now));
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(FixedUpdate, think.in_set(crate::BattleSet::Orders).run_if(in_state(crate::menu::Screen::Battle)));
}

pub fn spawn(commands: &mut Commands, assets: &AssetServer, data: &Value) {
    let env = &data["environment"];
    let def = &env["dragon"];
    let Some(list) = env["dragons"].as_array().filter(|l| !l.is_empty()) else { return };
    let hp = def["hp"].as_f64().unwrap_or(500.0) as f32;
    let scene = def["scene"].as_str().unwrap_or("SpaceDragon");
    for d in list {
        let group = d["group"].as_str().unwrap_or("Dragons");
        let pos = game_to_bevy(&d["pos"]);
        let forward = crate::scenario::game_rotation(&d["rot"]) * Vec3::NEG_Z;
        let heading = (-forward.x).atan2(-forward.z);
        let id = d["id"].as_i64().unwrap_or(-1);
        let mut animal = SpaceAnimal::new(group, 9.0, Vec3::ZERO, id.max(0) as u64, true);
        animal.speed = def["physics"]["MaxSpeed"].as_f64().unwrap_or(12.0) as f32;
        commands.spawn((
            Scenario,
            animal,
            Dragon {
                stance: Stance::Neutral,
                threshold: 0.0,
                border: None,
                home: None,
                attack: vec![],
                hold: false,
                hp,
                max_hp: hp,
                fleeing: false,
                provoked: None,
                cooldown: 0.0,
                roar_in: 4.0 + (id.rem_euclid(5)) as f32,
                roars: id.max(0) as usize,
                flap: id as f32,
            },
            crate::data::scene(assets, &format!("{scene}.glb")),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(heading)),
            Visibility::Inherited,
        ));
    }
}

/// Gravity-affected breath: the launch direction for muzzle speed `s` to reach offset `d` (low arc), or straight if out of reach.
/// PROVISIONAL gravity on dragon breath (dragons have no gun data to derive it from).
const BREATH_G: f32 = 9.8;

fn breath_dir(d: Vec3, s: f32) -> Vec3 {
    const G: f32 = BREATH_G;
    let (r, h) = (Vec2::new(d.x, d.z).length(), d.y);
    let disc = s.powi(4) - G * (G * r * r + 2.0 * h * s * s);
    if disc < 0.0 || r < 0.01 {
        return d.normalize_or(Vec3::NEG_Z);
    }
    let theta = ((s * s - disc.sqrt()) / (G * r)).atan();
    Vec3::new(d.x, 0.0, d.z).normalize_or_zero() * theta.cos() + Vec3::Y * theta.sin()
}

fn think(
    time: Res<Time>,
    mission: Option<Res<Mission>>,
    skirmish: Option<Res<crate::rules::skirmish::SkirmishScript>>,
    fxl: Option<Res<crate::fx::FxLib>>,
    mut commands: Commands,
    mut dragons: Query<(Entity, &mut Dragon, &mut SpaceAnimal, &mut Transform, &mut Visibility), Without<Ship>>,
    ships: Query<(Entity, &Transform, &Ship, &Health, Option<&MissionShip>, Has<crate::PlayerShip>)>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut shoot: MessageWriter<crate::combat::Shoot>,
) {
    let (sc, def) = match (&mission, &skirmish) {
        (Some(m), _) => (&m.script, &m.data["environment"]["dragon"]),
        (_, Some(s)) => (&s.script, &s.environment["dragon"]),
        _ => return,
    };
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let max_speed = def["physics"]["MaxSpeed"].as_f64().unwrap_or(12.0) as f32;
    let sight = def["sight"].as_f64().unwrap_or(500.0) as f32;
    let gun = &def["gun"];
    let num = |v: &Value, d: f32| v.as_f64().map_or(d, |x| x as f32);
    let (muzzle, reload, range) = (num(&gun["muzzle"], 40.0), num(&gun["reload"], 10.0), num(&gun["range"][2], 60.0));
    let bullet = gun["bullet"].as_str().unwrap_or("Bullet_DragonBreath").to_string();
    for (e, mut d, mut animal, mut tf, mut vis) in &mut dragons {
        if animal.dead || !animal.visible {
            animal.paused = false;
            continue;
        }
        // Death: the strength is gone.
        if d.hp <= 0.0 {
            animal.dead = true;
            animal.visible = false;
            *vis = Visibility::Hidden;
            if let Some(fxl) = fxl.as_deref() {
                let at = Transform::from_translation(tf.translation);
                crate::fx::spawn(&mut commands, fxl, "ShipExplosion_Medium", None, at, at, true);
            }
            sfx.write(crate::audio::Sfx { name: "Animal_SpaceDragon_Impact".into(), at: Some(tf.translation), vol: 1.0, max: 2000.0 });
            continue;
        }
        let lost = 1.0 - d.hp / d.max_hp.max(1.0);
        if d.threshold > 0.0 && lost >= d.threshold {
            d.fleeing = true;
        }
        // Roars (WOT: every 7..13 s, heard within 700).
        d.roar_in -= dt;
        if d.roar_in <= 0.0 {
            if let Some(sounds) = def["sounds"].as_array().filter(|a| !a.is_empty()) {
                sfx.write(crate::audio::Sfx {
                    name: sounds[d.roars % sounds.len()].as_str().unwrap_or_default().into(),
                    at: Some(tf.translation),
                    vol: 0.8,
                    max: num(&def["sound_distance"], 700.0),
                });
            }
            d.roars += 1;
            let (lo, hi) = (num(&def["sound_interval"][0], 7.0), num(&def["sound_interval"][1], 13.0));
            d.roar_in = lo + (hi - lo) * ((d.roars * 37) % 101) as f32 / 100.0;
        }
        d.cooldown -= dt;
        // Target: an ordered group, else (aggressive/persistent) the nearest hostile in sight, else whoever hurt it lately.
        let candidates = || {
            ships
                .iter()
                .filter(|s| !s.3.destroyed && s.4.is_none_or(|m| m.visible) && s.1.translation.distance(tf.translation) < sight * 1.5)
                .map(|s| (s.0, s.1.translation, s.2, s.4, s.5))
        };
        let nearest = |pred: &dyn Fn(Option<&MissionShip>, bool) -> bool| crate::nearest(candidates().filter(|c| pred(c.3, c.4)), tf.translation, |c| c.1);
        let ordered = nearest(&|m, primary| m.is_some_and(|m| d.attack.iter().any(|g| sc.answers(m.id, &m.owner, &m.name, primary, g))));
        let hostile = || nearest(&|m, _| m.is_some_and(|m| sc.hostile(&animal.owner, &m.owner)));
        let target = match (d.fleeing, d.stance) {
            (true, _) | (_, Stance::Dummy) => None,
            _ if ordered.is_some() => ordered,
            (_, Stance::Aggressive | Stance::Persistent) => hostile().filter(|t| t.1.distance(tf.translation) < sight),
            (_, Stance::Defensive) => hostile().filter(|t| t.1.distance(tf.translation) < sight * 0.5),
            _ => d.provoked.filter(|p| now - p.1 < 20.0).and_then(|p| candidates().find(|c| c.0 == p.0)),
        };
        // Steering goal: break off home, return inside the border zone, close on the target, or follow the rules' path.
        let back = d.border.as_deref().and_then(|z| crate::rules::leash(sc, z, d.home, tf.translation));
        let goal = if d.fleeing {
            Some(d.home.unwrap_or(tf.translation + tf.forward() * 100.0))
        } else if let Some(b) = back {
            Some(b)
        } else if let Some(t) = target {
            // Stand off just inside breath range, circling.
            let off = (tf.translation - t.1).with_y(0.0);
            let keep = range * 0.7;
            Some(if off.length() > keep { t.1 + off.normalize_or_zero() * keep } else { t.1 + Quat::from_rotation_y(0.6) * off })
        } else if d.hold {
            Some(tf.translation)
        } else {
            None
        };
        animal.paused = goal.is_some();
        if let Some(g) = goal {
            let delta = (g - tf.translation).with_y(0.0);
            let dist = delta.length();
            let speed = if dist < 2.0 { 0.0 } else { max_speed.min(dist / dt.max(1e-3)) };
            let dir = delta.normalize_or_zero();
            tf.translation += dir * speed * dt;
            animal.velocity = dir * speed;
            let face = target.filter(|_| !d.fleeing).map_or(dir, |t| (t.1 - tf.translation).with_y(0.0).normalize_or(dir));
            if face.length_squared() > 0.5 {
                let heading = (-face.x).atan2(-face.z);
                tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(heading), 1.0 - (-3.0 * dt).exp());
            }
        }
        // Wing beat: a gentle bob (bind pose only; the retail skeletal animation is not decoded).
        d.flap += dt;
        tf.translation.y += (d.flap * 2.0).sin() * 0.6 * dt;
        // Breath: in range and roughly facing.
        if let Some(t) = target
            && d.cooldown <= 0.0
            && t.1.distance(tf.translation) <= range
        {
            let mouth = tf.translation + tf.forward() * 6.0;
            let aim = t.1 + Vec3::Y * 2.0 - mouth;
            if tf.forward().dot(aim.with_y(0.0).normalize_or_zero()) > 0.5 {
                d.cooldown = reload;
                let side = if t.2.side == Side::Player { Side::Enemy } else { Side::Player };
                shoot.write(crate::combat::Shoot {
                    bullet: bullet.clone(),
                    from: mouth,
                    vel: breath_dir(aim, muzzle) * muzzle,
                    side,
                    shooter: e,
                    target: Some(t.0),
                    gravity: BREATH_G,
                });
                if let Some(name) = gun["sound"].as_str() {
                    sfx.write(crate::audio::Sfx { name: name.into(), at: Some(mouth), vol: 1.0, max: 700.0 });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breath_arc_reaches_close_targets_and_falls_back_straight() {
        let d = Vec3::new(30.0, 0.0, -20.0);
        let v = breath_dir(d, 40.0);
        assert!(v.y > 0.0 && (v.length() - 1.0).abs() < 1e-4);
        assert_eq!(breath_dir(Vec3::new(0.0, 0.0, -5000.0), 10.0), Vec3::NEG_Z);
    }
    #[test]
    fn hurt_tracks_strength_and_attacker() {
        let mut d = Dragon {
            stance: Stance::Neutral,
            threshold: 0.75,
            border: None,
            home: None,
            attack: vec![],
            hold: false,
            hp: 500.0,
            max_hp: 500.0,
            fleeing: false,
            provoked: None,
            cooldown: 0.0,
            roar_in: 0.0,
            roars: 0,
            flap: 0.0,
        };
        d.hurt(600.0, Entity::PLACEHOLDER, 3.0);
        assert_eq!(d.hp, 0.0);
        assert_eq!(d.provoked, Some((Entity::PLACEHOLDER, 3.0)));
    }
}
