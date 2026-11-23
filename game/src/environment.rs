//! Mission environment: retail nebula mesh/point sets and space-whale placements, paths and calls.
//! PROVISIONAL: whale bind-pose swimming, nebula energy rates and sensor occlusion; retail skeletal/mesh-shifting animation is not decoded.
use bevy::prelude::*;
use serde_json::Value;

use crate::{Scenario, Ship, campaign::Mission, game_to_bevy, rules::Unit};

struct SensorObject {
    at: Vec3,
    sphere: (Vec3, f32),
    sight: Option<f32>,
    group: String,
    shown: bool,
    /// The teams that still see this ship while it is fully cloaked (None: not fully cloaked).
    cloak: Option<Vec<crate::Side>>,
    /// The ship's team (bases and scenery: none that any cloak knows).
    side: crate::Side,
    scenery: bool,
}

/// Real seconds between re-tests of what the player sees (docs/re/visibility.md section 5).
pub const SIGHT_RETEST: f64 = 1.0;
/// The fade after a sighting rises (and after a loss falls) by this much per real second, between 0 and 1.
pub const SIGHT_FADE_RATE: f32 = 0.8;

/// What the player's view holds of one object: seen at the last re-test, and the fade that follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Sighting {
    seen: bool,
    fade: f32,
}

/// Frame-boundary snapshot shared by UI and target acquisition; never use camera position as an observer.
#[derive(Resource, Default)]
pub struct Sensors {
    objects: std::collections::HashMap<Entity, SensorObject>,
    primary: Option<Entity>,
    /// The player's view, kept across snapshots: re-tested once a second, faded every frame.
    sightings: std::collections::HashMap<Entity, Sighting>,
    /// Real time of the last re-test (none yet: the next frame re-tests).
    retested: Option<f64>,
    /// Re-test on the next frame whatever the time (the smoke script, after it moves ships).
    pub retest_now: bool,
}
impl Sensors {
    /// The collision sphere (centre, radius) of a ship or base, as the sight test sees it.
    pub fn sphere(&self, e: Entity) -> Option<(Vec3, f32)> {
        self.objects.get(&e).map(|o| o.sphere)
    }
    /// Any live member of the observer's group can supply sight, using collision surface distance.
    pub fn in_sight(&self, observer: Entity, target: Entity, mission: Option<&Mission>) -> bool {
        let (Some(from), Some(to)) = (self.objects.get(&observer), self.objects.get(&target)) else { return false };
        if !to.shown {
            return false;
        }
        // shortcut: cinematics reveal scripted-visible targets; decode native NIS visibility flags before narrowing this exception.
        if crate::briefing::active(mission) || crate::rules::nis(mission) {
            return true;
        }
        self.objects.iter().any(|(e, member)| {
            (*e == observer || (!from.group.is_empty() && member.group == from.group))
                && member.shown
                && member.sight.is_some_and(|range| {
                    crate::lifeboats::surface_distance(member.sphere, to.sphere) < range
                        && to.cloak.as_ref().is_none_or(|knows| knows.contains(&member.side))
                        && can_see(mission, member.at, to.at)
                })
        })
    }
    /// Whether the player's view shows the object: seen at the last once-a-second re-test, or still fading out after it was lost
    /// (and not script-hidden or dead). Interface, labels, radar, picking and the ship's own effects follow this.
    pub fn player_sees(&self, target: Entity, _mission: Option<&Mission>) -> bool {
        self.objects.get(&target).is_some_and(|o| o.shown) && self.sightings.get(&target).is_some_and(|s| s.seen || s.fade > 0.0)
    }
    /// A line between two objects (grapple, tow) is drawn only while the view shows both ends.
    pub fn player_sees_both(&self, a: Entity, b: Entity, mission: Option<&Mission>) -> bool {
        self.player_sees(a, mission) && self.player_sees(b, mission)
    }
    /// The player's sight test as it stands this frame (what the once-a-second re-test asks).
    pub fn sees_now(&self, target: Entity, mission: Option<&Mission>) -> bool {
        let Some(to) = self.objects.get(&target).filter(|o| o.shown) else { return false };
        let Some(primary) = self.primary else { return true }; // Native no-current-ship case.
        if crate::briefing::active(mission) || crate::rules::nis(mission) {
            return true;
        }
        let from = &self.objects[&primary];
        if target == primary || (!from.group.is_empty() && from.group == to.group) {
            return true;
        }
        // Native islands have a separate UI distance policy; do not range-hide scenery on an unverified ship rule.
        if to.scenery {
            return can_see(mission, from.at, to.at);
        }
        self.in_sight(primary, target, mission)
    }
    /// One frame of the player's view, `now` and `dt` in real seconds. An object new to the snapshot that is in sight is shown at
    /// once at full fade. Otherwise every object is re-tested when more than `SIGHT_RETEST` has passed since the last re-test, and
    /// the fade moves toward 1 while seen and toward 0 while not by `SIGHT_FADE_RATE` x dt. On a re-test frame an object that was
    /// shown or is seen again takes no fade step (the original restarts its fade clock then).
    pub fn advance_sightings(&mut self, mission: Option<&Mission>, now: f64, dt: f32) {
        let retest = self.retest_now || self.retested.is_none_or(|t| now - t > SIGHT_RETEST);
        if retest {
            self.retested = Some(now);
            self.retest_now = false;
        }
        self.sightings.retain(|e, _| self.objects.contains_key(e));
        let tested: Vec<(Entity, bool)> = self.objects.keys().filter(|e| retest || !self.sightings.contains_key(e)).map(|&e| (e, self.sees_now(e, mission))).collect();
        let mut fresh = std::collections::HashSet::new();
        for (e, seen) in tested {
            match self.sightings.entry(e) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(Sighting { seen, fade: if seen { 1.0 } else { 0.0 } });
                    fresh.insert(e);
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    let s = o.get_mut();
                    if s.seen || s.fade > 0.0 || seen {
                        fresh.insert(e);
                    }
                    s.seen = seen;
                }
            }
        }
        for (e, s) in &mut self.sightings {
            if fresh.contains(e) {
                continue;
            }
            s.fade = if s.seen { (s.fade + SIGHT_FADE_RATE * dt).min(1.0) } else { (s.fade - SIGHT_FADE_RATE * dt).max(0.0) };
        }
    }
}

/// After each snapshot: the player's view takes one frame (real time, so it runs on while the game is paused or sped up).
fn advance_sightings(mut sensors: ResMut<Sensors>, mission: Option<Res<Mission>>, time: Res<Time<Real>>) {
    sensors.advance_sightings(mission.as_deref(), time.elapsed_secs_f64(), time.delta_secs());
}

/// A ship's crew factor on its sight range. Retail saves store each computer ship's scan range as its WOT sight times its crew
/// factor (a Barque's 500 as 575 / 687.5 / 737.5 with Green / Average / Elite crews); ships without one use the plain sight.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ScanFactor(pub f32);

pub fn refresh_sensors(
    mut sensors: ResMut<Sensors>,
    lib: Res<crate::combat::Library>,
    ships: Query<(
        Entity,
        &Transform,
        &Ship,
        &crate::combat::Health,
        Option<&crate::campaign::MissionShip>,
        Option<&crate::ship::FleetMember>,
        Option<&crate::cloak::Cloak>,
        Has<crate::PlayerShip>,
        Option<&ScanFactor>,
    )>,
    objects: Query<
        (
            Entity,
            &Transform,
            Option<&crate::combat::ArmedBase>,
            Option<&crate::combat::Health>,
            Option<&crate::campaign::MissionBase>,
            Option<&crate::collision::Obstacle>,
            Option<&SpaceAnimal>,
            Option<&crate::combat::Asteroid>,
            Option<&crate::docking::Dockable>,
        ),
        (Without<Ship>, Or<(With<crate::combat::ArmedBase>, With<crate::collision::Obstacle>, With<SpaceAnimal>, With<crate::combat::Asteroid>, With<crate::docking::Dockable>)>),
    >,
) {
    sensors.objects.clear();
    sensors.primary = None;
    for (e, tf, ship, health, ms, fleet, cloak, primary, scan) in &ships {
        let class = &lib.classes[ship.class];
        let (centre, radius) = class.sphere;
        sensors.objects.insert(
            e,
            SensorObject {
                at: tf.translation,
                sphere: (tf.transform_point(centre), radius * tf.scale.abs().max_element()),
                sight: (!class.is_lifeboat).then_some(class.sight * scan.map_or(1.0, |f| f.0)),
                group: fleet.map(|f| f.0.as_str()).or_else(|| ms.map(|m| m.owner.as_str())).unwrap_or("").into(),
                shown: !health.destroyed && ms.is_none_or(|m| m.visible),
                cloak: cloak.filter(|c| c.state == crate::cloak::State::Full).map(|c| c.knows.clone()),
                side: ship.side,
                scenery: false,
            },
        );
        if primary {
            sensors.primary = Some(e);
        }
    }
    for (e, tf, armed, health, base, obstacle, animal, rock, dock) in &objects {
        let sphere = if let Some(base) = armed {
            let (centre, radius) = lib.classes[base.class].sphere;
            (tf.transform_point(centre), radius * tf.scale.abs().max_element())
        } else if let Some(obstacle) = obstacle {
            obstacle.sphere()
        } else {
            // shortcut: collider-free mod docks retain the previous dock-point bound until they supply collision geometry.
            let radius = animal
                .map(|a| a.radius)
                .or_else(|| rock.map(|r| r.radius))
                .unwrap_or_else(|| dock.map_or(0.0, |d| d.points.iter().map(|p| p.pos.length()).fold(0.0, f32::max) + 30.0));
            (tf.translation, radius)
        };
        sensors.objects.insert(
            e,
            SensorObject {
                at: tf.translation,
                sphere,
                sight: armed.map(|b| lib.classes[b.class].sight),
                group: base.map_or("", |b| b.owner.as_str()).into(),
                shown: health.is_none_or(|h| !h.destroyed) && animal.is_none_or(|a| a.visible && !a.dead) && rock.is_none_or(|r| r.hp > 0.0),
                cloak: None,
                side: crate::Side(u8::MAX),
                scenery: animal.is_none(),
            },
        );
    }
}

/// --script sight: exercise the shared contacts against real HUD widgets in either presentation.
fn sensor_smoke(world: &mut World) {
    let cfg = world.resource::<crate::Config>();
    if cfg.script.as_deref() != Some("sight") || ![30, 45, 65, 85, 105, 125].contains(&cfg.frame) {
        return;
    }
    let frame = cfg.frame;
    let primary = world.query_filtered::<Entity, With<crate::PlayerShip>>().single(world).expect("sight smoke primary");
    let ally =
        world.query::<(Entity, &Ship)>().iter(world).find(|(e, s)| *e != primary && s.side == crate::Side::Player).expect("sight smoke requires --allies Ship_Navy_Cutter").0;
    // Sorted: query order changes when the smoke adds components (the cloak), and `enemy` must stay the same ship.
    let mut enemies: Vec<Entity> = world.query::<(Entity, &Ship)>().iter(world).filter(|(_, s)| s.side == crate::Side::Enemy).map(|(e, _)| e).collect();
    enemies.sort();
    let enemy = *enemies.first().expect("sight smoke requires an enemy");
    let group = world.get::<crate::ship::FleetMember>(primary).unwrap().0.clone();
    if frame != 30 {
        let seen = world.resource::<Sensors>().sees_now(enemy, None);
        let radar = world.query_filtered::<&Visibility, With<crate::panels::Contact>>().iter(world).any(|v| *v != Visibility::Hidden);
        let expected = matches!(frame, 65 | 125);
        assert_eq!(seen, expected, "sight smoke contact frame {frame}");
        // The radar only reaches ~153 units (panels.rs): the enemy shared by the far ally at frame 65 is past the rim.
        assert_eq!(radar, frame == 125, "sight smoke radar frame {frame}");
    }
    match frame {
        30 => {
            // Extra enemies (the default --enemies has two) go far out of every sight range, so only `enemy` is ever a contact.
            let parked = enemies.iter().skip(1).enumerate().map(|(i, e)| (*e, 10_000.0 + 1000.0 * i as f32));
            for (e, x) in [(primary, 0.0), (ally, 1000.0), (enemy, 2000.0)].into_iter().chain(parked) {
                world.get_mut::<Transform>(e).unwrap().translation = Vec3::X * x;
                let mut ship = world.get_mut::<Ship>(e).unwrap();
                ship.speed = 0.0;
                ship.halt();
                world.entity_mut(e).insert(crate::ship::HoldPosition).remove::<crate::combat::GunStates>();
            }
        }
        45 => world.get_mut::<Transform>(enemy).unwrap().translation = Vec3::X * 1100.0,
        65 => world.get_mut::<crate::ship::FleetMember>(ally).unwrap().0 = "Unrelated ally".into(),
        85 => {
            world.get_mut::<crate::ship::FleetMember>(ally).unwrap().0 = group;
            // The rules' 'AI Can Override' FALSE: the cloak AI would drop a cloak on a ship whose guns the smoke removed (#418).
            let mut cloak = crate::cloak::Cloak::new(true);
            cloak.ai_modify = false;
            world.entity_mut(enemy).insert(cloak);
        }
        105 => {
            world.entity_mut(enemy).remove::<crate::cloak::Cloak>();
            world.get_mut::<Transform>(enemy).unwrap().translation = Vec3::X * 120.0;
        }
        125 => info!("sight ECS/UI smoke passed: range loss/reveal, group sharing, unrelated ally, cloak and radar"),
        _ => unreachable!(),
    }
    // The steps are a few frames apart, not the once-a-second re-test: show each step's result on the next frame.
    world.resource_mut::<Sensors>().retest_now = true;
}

/// The swim or wing-beat clip baked into an animal's model (tools/skin.py, docs/re/effects.md "Animal skeletons"), or every glTF
/// clip of a mod's scenery model (cranes, flags, beacons), all looped together.
#[derive(Component)]
pub struct Clip {
    pub clips: Vec<Handle<AnimationClip>>,
    /// The entity holding the clips' player and the graph nodes playing them, once the model has spawned.
    player: Option<(Entity, Vec<AnimationNodeIndex>)>,
}
impl Clip {
    pub fn load(assets: &AssetServer, file: &str) -> Self {
        Clip { clips: vec![assets.load(GltfAssetLabel::Animation(0).from_asset(crate::data::model(file)))], player: None }
    }
    /// Every clip in a scenery model, or None when it has none (all retail scenery: its clips are `data/animations.json`).
    pub fn scenery(assets: &AssetServer, file: &str) -> Option<Self> {
        let path = crate::data::model(file);
        let bytes = std::fs::read(crate::assets_dir().join(&path)).ok()?;
        let n = crate::data::glb_json(&bytes)?.0["animations"].as_array().map_or(0, Vec::len);
        (n > 0).then(|| Clip { clips: (0..n).map(|i| assets.load(GltfAssetLabel::Animation(i).from_asset(path.clone()))).collect(), player: None })
    }
}

/// Starts an animal's clip once its model has spawned (the glTF loader puts the player on the skeleton root): looped, from a
/// phase of its own (the original draws a random start in [0, duration)). PROVISIONAL: the phase comes from the entity, not a draw.
fn play_clips(
    mut commands: Commands,
    mut players: Query<(Entity, &mut AnimationPlayer), Added<AnimationPlayer>>,
    parents: Query<&ChildOf>,
    mut clips: Query<&mut Clip>,
    assets: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for (e, mut player) in &mut players {
        let Some(owner) = parents.iter_ancestors(e).find(|a| clips.contains(*a)) else { continue };
        let mut clip = clips.get_mut(owner).unwrap();
        let (graph, nodes) = AnimationGraph::from_clips(clip.clips.iter().cloned());
        let phase = (owner.to_bits().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
        for (node, h) in nodes.iter().zip(&clip.clips) {
            let duration = assets.get(h).map_or(0.0, |c| c.duration());
            player.play(*node).repeat().seek_to(phase * duration);
        }
        commands.entity(e).insert(AnimationGraphHandle(graphs.add(graph)));
        clip.player = Some((e, nodes));
    }
}

/// The clip's rate (the original's rules): a dragon beats its wings at 0.2 x its speed, held between 0.8 and 1.3; a whale swims
/// at 1 while it moves and holds its pose while it does not. PROVISIONAL: the whale's on/off value is read as its speed.
fn clip_rates(animals: Query<(&Clip, &SpaceAnimal, Has<crate::dragons::Dragon>)>, mut players: Query<&mut AnimationPlayer>) {
    for (clip, animal, dragon) in &animals {
        let Some((e, nodes)) = &clip.player else { continue };
        let Ok(mut player) = players.get_mut(*e) else { continue };
        let Some(active) = player.animation_mut(nodes[0]) else { continue };
        let speed = animal.velocity.length();
        let rate = if dragon {
            (0.2 * speed).clamp(0.8, 1.3)
        } else if speed >= 0.01 {
            1.0
        } else {
            0.0
        };
        active.set_speed(rate);
    }
}

#[derive(Component)]
pub struct SpaceAnimal {
    pub home: String,
    pub owner: String,
    pub visible: bool,
    pub radius: f32,
    pub velocity: Vec3,
    pub speed: f32,
    path: Vec<Vec3>,
    next: usize,
    looping: bool,
    offset: Vec3,
    call_in: f32,
    calls: usize,
    /// Another system (dragons.rs) is steering it: no path following or calls here.
    pub paused: bool,
    /// Killed (dragons): gone from the rules' units.
    pub dead: bool,
}
impl SpaceAnimal {
    pub fn new(home: &str, radius: f32, offset: Vec3, id: u64, visible: bool) -> Self {
        SpaceAnimal {
            home: home.into(),
            owner: home.into(),
            visible,
            radius,
            velocity: Vec3::ZERO,
            speed: 0.0,
            path: vec![],
            next: 0,
            looping: false,
            offset,
            call_in: 3.0 + (id % 7) as f32,
            calls: id as usize,
            paused: false,
            dead: false,
        }
    }
    pub fn unit(&self, e: Entity, tf: &Transform) -> Unit {
        Unit { name: self.home.clone(), owner: self.owner.clone(), groups: vec![self.home.clone()], alive: !self.dead, visible: self.visible, ..Unit::new(e, -1, tf.translation) }
    }
    pub fn follow(&mut self, points: &[Vec3], pos: Vec3, closest: bool, looping: bool) {
        self.path = points.iter().map(|p| *p + self.offset).collect();
        self.next = if closest { self.path.iter().enumerate().min_by(|a, b| a.1.distance_squared(pos).total_cmp(&b.1.distance_squared(pos))).map_or(0, |(i, _)| i) } else { 0 };
        self.looping = looping;
    }
    fn advance(&mut self, tf: &mut Transform, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let before = tf.translation;
        let mut remaining = self.speed.max(0.0) * dt;
        // At most one circuit per frame also handles a path made entirely of duplicate points.
        for _ in 0..=self.path.len() {
            let Some(point) = self.path.get(self.next) else { break };
            let delta = *point - tf.translation;
            let distance = delta.length();
            if distance > remaining {
                tf.translation += delta.normalize_or_zero() * remaining;
                break;
            }
            tf.translation = *point;
            remaining -= distance;
            self.next += 1;
            if self.next == self.path.len() && self.looping {
                self.next = 0;
            }
            if remaining <= 0.0 {
                break;
            }
        }
        self.velocity = (tf.translation - before) / dt.max(0.0001);
        if self.velocity.length_squared() > 0.001 {
            let heading = (-self.velocity.x).atan2(-self.velocity.z);
            tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(heading), 1.0 - (-2.0 * dt).exp());
        }
    }
}

/// On a ship's own running effect (and wake) while the local player's group does not see the ship: it keeps running, it is not drawn.
#[derive(Component)]
pub struct Unseen;

/// The original switches a ship's own effects off while the player's group does not see the ship and on again when it does:
/// engine exhausts, wakes, damage smoke, fires and the other effects the ship carries. The hull itself is still drawn, and one-shot
/// effects (hits, muzzle flashes) are not touched (docs/re/visibility.md section 5). A sunk ship keeps its effects (not read).
#[allow(clippy::type_complexity)]
pub fn mark_unseen_ship_effects(
    mut commands: Commands,
    sensors: Res<Sensors>,
    mission: Option<Res<Mission>>,
    ships: Query<&crate::combat::Health, With<Ship>>,
    mut fx: Query<(Entity, &ChildOf, &mut Visibility, Has<Unseen>), (With<crate::fx::Fx>, Without<crate::combat::Transient>, Without<crate::wake::Wake>)>,
    mut wakes: Query<(Entity, &crate::wake::Wake, &mut Visibility, Has<Unseen>), Without<crate::fx::Fx>>,
) {
    let unseen = |ship: Entity| ships.get(ship).is_ok_and(|h| !h.destroyed && !sensors.player_sees(ship, mission.as_deref()));
    let mut set = |e: Entity, vis: &mut Visibility, marked: bool, hide: bool| {
        let want = if hide { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
        match (marked, hide) {
            (false, true) => {
                commands.entity(e).insert(Unseen);
            }
            (true, false) => {
                commands.entity(e).remove::<Unseen>();
            }
            _ => {}
        }
    };
    for (e, parent, mut vis, marked) in &mut fx {
        if ships.contains(parent.parent()) {
            set(e, &mut vis, marked, unseen(parent.parent()));
        }
    }
    for (e, wake, mut vis, marked) in &mut wakes {
        set(e, &mut vis, marked, unseen(wake.ship()));
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Sensors>()
        .add_systems(
            PreUpdate,
            (sensor_smoke, refresh_sensors, advance_sightings)
                .chain()
                .after(crate::smoke::fleet_smoke)
                .before(crate::panels::hud_input)
                .run_if(in_state(crate::menu::Screen::Battle)),
        )
        .add_systems(FixedUpdate, refresh_sensors.in_set(crate::BattleSet::Sense).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(FixedUpdate, update.in_set(crate::BattleSet::Forces).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, (current_fx, flow_currents, mark_unseen_ship_effects).run_if(in_state(crate::menu::Screen::Battle)))
        .add_systems(Update, (play_clips, clip_rates).chain());
}

/// `whales_shown`: campaign missions start their whales hidden (the rules show them); skirmish maps' scripts never do.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, data: &Value, whales_shown: bool) {
    crate::dragons::spawn(commands, assets, data);
    let env = &data["environment"];
    let whale = &env["whale"];
    let animals = env["animals"].as_array().cloned().unwrap_or_default();
    // An import from before skirmish maps exported their whale has none: no whales rather than a panic.
    let animals = if whale["scene"].is_string() { animals } else { vec![] };
    for a in &animals {
        let home = a["group"].as_str().expect("animal group");
        let pos = game_to_bevy(&a["pos"]);
        let group: Vec<_> = animals.iter().filter(|b| b["group"] == a["group"]).map(|b| game_to_bevy(&b["pos"])).collect();
        let centre = group.iter().copied().sum::<Vec3>() / group.len() as f32;
        let id = a["id"].as_u64().unwrap_or(0);
        let forward = crate::scenario::game_rotation(&a["rot"]) * Vec3::NEG_Z;
        let heading = (-forward.x).atan2(-forward.z);
        let min = game_to_bevy(&whale["min"]);
        let max = game_to_bevy(&whale["max"]);
        commands.spawn((
            Scenario,
            SpaceAnimal::new(home, (max - min).length() / 2.0, pos - centre, id, whales_shown),
            crate::data::scene(assets, &format!("{}.glb", whale["scene"].as_str().expect("whale scene"))),
            Clip::load(assets, &format!("{}.glb", whale["scene"].as_str().expect("whale scene"))),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(heading)),
            if whales_shown { Visibility::Inherited } else { Visibility::Hidden },
        ));
    }
}

/// '*State Init* Setup Etherium Current': a stream along a mission path. Constants from `Terrain_Etherium_Current.wot`
/// (custom info Magnitude 25000, Radius 60). PROVISIONAL: the force is the magnitude along the path's direction within the radius
/// of the path (planar), applied as force / ship mass; the original's falloff and its use of the magnitude are unknown.
#[derive(Component)]
pub struct Current {
    path: Vec<Vec3>,
}
pub const CURRENT_FORCE: f32 = 25000.0;
pub const CURRENT_RADIUS: f32 = 60.0;
impl Current {
    /// Flows from the first authored point toward the last: in Mission 4 Nelson's fleet leaves "to the frontier" along the current
    /// on 'Nelson exit route', which runs the same way as 'Mucullough path'.
    pub fn new(path: Vec<Vec3>) -> Self {
        Current { path }
    }
    pub fn force(&self, p: Vec3) -> Vec3 {
        let flat = |v: Vec3| Vec2::new(v.x, v.z);
        let q = flat(p);
        self.path
            .windows(2)
            .filter_map(|w| {
                let (a, b) = (flat(w[0]), flat(w[1]));
                let ab = b - a;
                let t = ((q - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                let d = q.distance(a + ab * t);
                (d < CURRENT_RADIUS).then_some((d, ab.normalize_or_zero()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(Vec3::ZERO, |(_, dir)| Vec3::new(dir.x, 0.0, dir.y) * CURRENT_FORCE)
    }
}

#[derive(Component)]
pub struct CurrentRibbon(pub f32);

/// RiverPrimitive consumes a whole point path and width, not a particle emitter.
fn current_mesh(path: &[Vec3]) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut positions = Vec::new();
    let mut uv = Vec::new();
    let mut indices = Vec::new();
    let mut distance = 0.0;
    for (i, p) in path.iter().enumerate() {
        let before = path[i.saturating_sub(1)];
        let after = path[(i + 1).min(path.len() - 1)];
        let direction = (after - before).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let side = Vec3::new(-direction.z, 0.0, direction.x) * CURRENT_RADIUS;
        distance += p.distance(before);
        positions.extend([(*p - side).to_array(), (*p + side).to_array()]);
        uv.extend([[0.0, distance / (CURRENT_RADIUS * 4.0)], [1.0, distance / (CURRENT_RADIUS * 4.0)]]);
        if i > 0 {
            let k = (2 * i) as u32;
            indices.extend([k - 2, k, k - 1, k - 1, k, k + 1]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; positions.len()]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// EtheriumRiver authors two textured river layers; UV speeds remain provisional.
fn current_fx(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::fx::EffectMaterial>>,
    added: Query<(Entity, &Current), Added<Current>>,
) {
    use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
    for (e, c) in &added {
        if c.path.len() < 2 {
            continue;
        }
        let texture = assets
            .load_builder()
            .with_settings(|s: &mut ImageLoaderSettings| {
                s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_v: ImageAddressMode::Repeat, ..ImageSamplerDescriptor::linear() });
            })
            .load("fx/tex/current.png");
        let mesh = meshes.add(current_mesh(&c.path));
        for (layer, speed) in [0.15, 0.25].into_iter().enumerate() {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(crate::fx::additive_material(texture.clone()))),
                Transform::from_xyz(0.0, layer as f32 * 0.01, 0.0),
                CurrentRibbon(speed),
                bevy::camera::visibility::NoFrustumCulling,
                ChildOf(e),
            ));
        }
        commands.entity(e).insert((Transform::default(), Visibility::default()));
    }
}

fn flow_currents(time: Res<Time>, mut materials: ResMut<Assets<crate::fx::EffectMaterial>>, ribbons: Query<(&CurrentRibbon, &MeshMaterial3d<crate::fx::EffectMaterial>)>) {
    for (ribbon, handle) in &ribbons {
        if let Some(mut material) = materials.get_mut(&handle.0) {
            material.base.uv_transform.translation.y = -(time.elapsed_secs() * ribbon.0).fract();
        }
    }
}

fn update(
    time: Res<Time>,
    mission: Option<Res<Mission>>,
    mut animals: Query<(&mut SpaceAnimal, &mut Transform, Has<crate::dragons::Dragon>), Without<Ship>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    let Some(mission) = mission else { return };
    let dt = time.delta_secs();
    let def = &mission.data["environment"]["whale"];
    for (mut animal, mut tf, dragon) in &mut animals {
        if !animal.visible || animal.paused {
            continue;
        }
        animal.advance(&mut tf, dt);
        if dragon {
            continue; // dragons roar in dragons.rs
        }
        animal.call_in -= dt;
        if animal.call_in <= 0.0 {
            if let Some(sounds) = def["sounds"].as_array().filter(|a| !a.is_empty()) {
                sfx.write(crate::audio::Sfx {
                    name: sounds[animal.calls % sounds.len()].as_str().unwrap().into(),
                    at: Some(tf.translation),
                    vol: 0.6,
                    max: def["sound_distance"].as_f64().unwrap_or(400.0) as f32,
                });
            }
            animal.calls += 1;
            let lo = def["sound_interval"][0].as_f64().unwrap_or(3.0) as f32;
            let hi = def["sound_interval"][1].as_f64().unwrap_or(10.0) as f32;
            animal.call_in = lo + (hi - lo) * ((animal.calls * 17) % 101) as f32 / 100.0;
        }
    }
}

/// Nebula clear-line test, separate from sensing range; no unverified close-range bypass.
pub fn can_see(mission: Option<&Mission>, from: Vec3, to: Vec3) -> bool {
    let Some(mission) = mission else { return true };
    !mission.data["environment"]["nebulae"].as_array().into_iter().flatten().any(|n| {
        crate::data::wot_flag(&n["Nebula Occlusion On/Off"])
            && crosses(&mission.data["volumes"][n["Polygon Name"].as_str().unwrap_or("")], Vec2::new(from.x, -from.z), Vec2::new(to.x, -to.z))
    })
}
fn crosses(polygon: &Value, a: Vec2, b: Vec2) -> bool {
    let Some(points) = polygon.as_array().filter(|p| p.len() >= 3) else { return false };
    let inside_a = crate::rules::in_game_volume(polygon, a.x, a.y);
    let inside_b = crate::rules::in_game_volume(polygon, b.x, b.y);
    if inside_a != inside_b {
        return true;
    }
    let point = |v: &Value| Vec2::new(v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32);
    (0..points.len()).any(|i| {
        let c = point(&points[i]);
        let d = point(&points[(i + 1) % points.len()]);
        let ab = b - a;
        let cd = d - c;
        let denominator = ab.perp_dot(cd);
        if denominator.abs() < 1e-6 {
            return false;
        }
        let t = (c - a).perp_dot(cd) / denominator;
        let u = (c - a).perp_dot(ab) / denominator;
        (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
    })
}

#[cfg(test)]
mod tests;
