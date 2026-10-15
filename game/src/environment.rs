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
    cloaked: bool,
    scenery: bool,
}

/// Frame-boundary snapshot shared by UI and target acquisition; never use camera position as an observer.
#[derive(Resource, Default)]
pub struct Sensors {
    objects: std::collections::HashMap<Entity, SensorObject>,
    primary: Option<Entity>,
}
impl Sensors {
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
                    crate::lifeboats::surface_distance(member.sphere, to.sphere) < range && crate::cloak::seen(to.cloaked, to.at, member.at) && can_see(mission, member.at, to.at)
                })
        })
    }
    pub fn player_sees(&self, target: Entity, mission: Option<&Mission>) -> bool {
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
}

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
    for (e, tf, ship, health, ms, fleet, cloak, primary) in &ships {
        let class = &lib.classes[ship.class];
        let (centre, radius) = class.sphere;
        sensors.objects.insert(
            e,
            SensorObject {
                at: tf.translation,
                sphere: (tf.transform_point(centre), radius * tf.scale.abs().max_element()),
                sight: (!class.is_lifeboat).then_some(class.sight),
                group: fleet.map(|f| f.0.as_str()).or_else(|| ms.map(|m| m.owner.as_str())).unwrap_or("").into(),
                shown: !health.destroyed && ms.is_none_or(|m| m.visible),
                cloaked: cloak.is_some_and(|c| c.hidden()),
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
                group: base.map(|b| b.owner.as_str()).unwrap_or("").into(),
                shown: health.is_none_or(|h| !h.destroyed) && animal.is_none_or(|a| a.visible && !a.dead) && rock.is_none_or(|r| r.hp > 0.0),
                cloaked: false,
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
    let enemy = world.query::<(Entity, &Ship)>().iter(world).find(|(_, s)| s.side == crate::Side::Enemy).expect("sight smoke requires an enemy").0;
    let group = world.get::<crate::ship::FleetMember>(primary).unwrap().0.clone();
    if frame != 30 {
        let seen = world.resource::<Sensors>().player_sees(enemy, None);
        let radar = world.query_filtered::<&Visibility, With<crate::panels::Contact>>().iter(world).any(|v| *v != Visibility::Hidden);
        let expected = matches!(frame, 65 | 125);
        assert_eq!(seen, expected, "sight smoke contact frame {frame}");
        assert_eq!(radar, expected, "sight smoke radar frame {frame}");
    }
    match frame {
        30 => {
            for (e, x) in [(primary, 0.0), (ally, 1000.0), (enemy, 2000.0)] {
                world.get_mut::<Transform>(e).unwrap().translation = Vec3::X * x;
                let mut ship = world.get_mut::<Ship>(e).unwrap();
                ship.speed = 0.0;
                ship.order = 0;
                ship.waypoints.clear();
                ship.goal = None;
                world.entity_mut(e).insert(crate::ship::HoldPosition).remove::<crate::combat::GunStates>();
            }
        }
        45 => world.get_mut::<Transform>(enemy).unwrap().translation = Vec3::X * 1100.0,
        65 => world.get_mut::<crate::ship::FleetMember>(ally).unwrap().0 = "Unrelated ally".into(),
        85 => {
            world.get_mut::<crate::ship::FleetMember>(ally).unwrap().0 = group;
            world.entity_mut(enemy).insert(crate::cloak::Cloak::new(true, false));
        }
        105 => {
            let mut cloak = world.get_mut::<crate::cloak::Cloak>(enemy).unwrap();
            cloak.on = false;
            cloak.level = 0.0;
            world.get_mut::<Transform>(enemy).unwrap().translation = Vec3::X * 120.0;
        }
        125 => info!("sight ECS/UI smoke passed: range loss/reveal, group sharing, unrelated ally, cloak and radar"),
        _ => unreachable!(),
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
    phase: f32,
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
            phase: id as f32,
            call_in: 3.0 + (id % 7) as f32,
            calls: id as usize,
            paused: false,
            dead: false,
        }
    }
    pub fn unit(&self, e: Entity, tf: &Transform) -> Unit {
        Unit {
            e,
            id: -1,
            name: self.home.clone(),
            owner: self.owner.clone(),
            owner_is_named_group: false,
            groups: vec![self.home.clone()],
            pos: tf.translation,
            alive: !self.dead,
            visible: self.visible,
            primary: false,
            damage: 0.0,
            flag: String::new(),
            attack: vec![],
        }
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

pub fn plugin(app: &mut App) {
    app.init_resource::<Sensors>()
        .add_systems(
            PreUpdate,
            (sensor_smoke, refresh_sensors).chain().after(crate::scenario::fleet_smoke).before(crate::panels::hud_input).run_if(in_state(crate::menu::Screen::Battle)),
        )
        .add_systems(Update, (update, current_fx, flow_currents).after(crate::input).before(crate::sail).run_if(in_state(crate::menu::Screen::Battle)));
}

pub fn spawn(commands: &mut Commands, assets: &AssetServer, data: &Value) {
    crate::dragons::spawn(commands, assets, data);
    let env = &data["environment"];
    let whale = &env["whale"];
    let animals = env["animals"].as_array().cloned().unwrap_or_default();
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
            SpaceAnimal::new(home, (max - min).length() / 2.0, pos - centre, id, false),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(&format!("{}.glb", whale["scene"].as_str().expect("whale scene")))))),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(heading)),
            Visibility::Hidden,
        ));
    }
}

/// '*State Init* Setup Etherium Current': a stream along a mission path. Constants from `Terrain_Etherium_Current.wot`
/// (custom info Magnitude 25000, Radius 60). PROVISIONAL: the force is the magnitude along the path's direction within the radius
/// of the path (planar), applied as force / ship mass; the exe's falloff and its use of the magnitude are not decoded.
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
struct CurrentRibbon(f32);

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

/// PROVISIONAL: clouds drain sail energy toward 25%; outside the nebula it recharges. Rates are not yet recovered from the exe.
fn energy_step(energy: f32, inside: bool, dt: f32) -> f32 {
    (energy + if inside { -0.12 * dt } else { 0.08 * dt }).clamp(0.25, 1.0)
}

fn update(
    time: Res<Time>,
    mission: Option<Res<Mission>>,
    mut ships: Query<(&mut Ship, &Transform)>,
    mut animals: Query<(&mut SpaceAnimal, &mut Transform, Has<crate::dragons::Dragon>), Without<Ship>>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
) {
    let Some(mission) = mission else { return };
    let dt = time.delta_secs().min(0.1);
    for (mut ship, tf) in &mut ships {
        let inside = mission.data["environment"]["nebulae"].as_array().into_iter().flatten().any(|n| {
            n["Nebula Cloud Energy Drain On/Off"] == "TRUE"
                && crate::rules::in_game_volume(&mission.data["volumes"][n["Polygon Name"].as_str().unwrap_or("")], tf.translation.x, -tf.translation.z)
        });
        ship.sail_energy = energy_step(ship.sail_energy, inside, dt);
    }
    let def = &mission.data["environment"]["whale"];
    for (mut animal, mut tf, dragon) in &mut animals {
        if !animal.visible || animal.paused {
            continue;
        }
        animal.advance(&mut tf, dt);
        if dragon {
            continue; // dragons roar and flap in dragons.rs
        }
        animal.phase += dt;
        // Bind-pose fallback: gentle body roll, not the retail skeletal fin/tail animation.
        tf.rotation *= Quat::from_rotation_z(0.02 * dt * animal.phase.sin());
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
        n["Nebula Occlusion On/Off"] == "TRUE" && crosses(&mission.data["volumes"][n["Polygon Name"].as_str().unwrap_or("")], Vec2::new(from.x, -from.z), Vec2::new(to.x, -to.z))
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
mod tests {
    use super::*;

    fn sensor_object(position: Vec3, radius: f32, group: &str, sight: f32) -> SensorObject {
        SensorObject { at: position, sphere: (position, radius), sight: Some(sight), group: group.into(), shown: true, cloaked: false, scenery: false }
    }

    #[test]
    fn sight_uses_strict_surface_distance_and_three_dimensions() {
        let (a, b) = (Entity::from_bits(1), Entity::from_bits(2));
        let mut sensors = Sensors { primary: Some(a), ..default() };
        sensors.objects.insert(a, sensor_object(Vec3::ZERO, 10.0, "A", 100.0));
        sensors.objects.insert(b, sensor_object(Vec3::X * 130.0, 20.0, "B", 100.0));
        assert!(!sensors.in_sight(a, b, None), "equal to the range is outside");
        sensors.objects.get_mut(&b).unwrap().sphere.0.x -= 0.01;
        assert!(sensors.player_sees(b, None), "surface, not centre distance");
        sensors.objects.get_mut(&b).unwrap().sphere.0 = Vec3::Y * 140.0;
        assert!(!sensors.player_sees(b, None), "altitude counts");
    }

    #[test]
    fn group_sharing_excludes_unrelated_hidden_or_dead_observers() {
        let (a, b, c) = (Entity::from_bits(1), Entity::from_bits(2), Entity::from_bits(3));
        let mut sensors = Sensors { primary: Some(a), ..default() };
        sensors.objects.insert(a, sensor_object(Vec3::ZERO, 0.0, "A", 100.0));
        sensors.objects.insert(b, sensor_object(Vec3::X * 200.0, 0.0, "B", 100.0));
        sensors.objects.insert(c, sensor_object(Vec3::X * 150.0, 0.0, "Other ally", 100.0));
        assert!(!sensors.player_sees(b, None));
        sensors.objects.get_mut(&c).unwrap().group = "A".into();
        assert!(sensors.player_sees(b, None));
        sensors.objects.get_mut(&c).unwrap().shown = false;
        assert!(!sensors.player_sees(b, None));
        sensors.objects.get_mut(&c).unwrap().shown = true;
        sensors.objects.get_mut(&b).unwrap().shown = false;
        assert!(!sensors.player_sees(b, None));
    }

    #[test]
    fn cloak_script_visibility_no_primary_and_scenery_are_separate() {
        let (a, b) = (Entity::from_bits(1), Entity::from_bits(2));
        let mut sensors = Sensors { primary: Some(a), ..default() };
        sensors.objects.insert(a, sensor_object(Vec3::ZERO, 0.0, "A", 100.0));
        sensors.objects.insert(b, sensor_object(Vec3::X * 80.0, 0.0, "B", 100.0));
        sensors.objects.get_mut(&b).unwrap().cloaked = true;
        assert!(!sensors.player_sees(b, None));
        sensors.objects.get_mut(&b).unwrap().sphere.0.x = 10.0;
        sensors.objects.get_mut(&b).unwrap().at.x = 10.0;
        assert!(sensors.player_sees(b, None));
        sensors.objects.get_mut(&b).unwrap().shown = false;
        sensors.primary = None;
        assert!(!sensors.player_sees(b, None), "no primary never reveals a script-hidden object");
        sensors.objects.get_mut(&b).unwrap().shown = true;
        sensors.objects.get_mut(&b).unwrap().cloaked = false;
        assert!(sensors.player_sees(b, None), "native no-primary exception");
        sensors.primary = Some(a);
        sensors.objects.get_mut(&b).unwrap().sphere.0.x = 1000.0;
        sensors.objects.get_mut(&b).unwrap().scenery = true;
        assert!(sensors.player_sees(b, None), "do not apply ship contact policy to islands");
        assert!(!sensors.in_sight(a, b, None), "docking still needs group sight");
    }

    #[test]
    fn sensor_snapshot_tracks_ship_spheres_script_changes_and_cleanup() {
        use crate::combat::{Health, Library};
        let mut app = App::new();
        let lib = Library::load(&crate::assets_dir());
        let class = lib.index("Ship_Navy_Cutter");
        let health = Health::new(&lib.classes[class]);
        let sphere = lib.classes[class].sphere;
        app.insert_resource(lib).init_resource::<Sensors>().add_systems(Update, refresh_sensors);
        let tf = Transform::from_xyz(10.0, 20.0, 30.0).with_rotation(Quat::from_rotation_y(0.8));
        let ship = Ship {
            class,
            side: crate::Side::Player,
            heading: 0.8,
            speed: 0.0,
            ang_vel: 0.0,
            order: 0,
            waypoints: vec![],
            goal: None,
            cap: 1.0,
            throttle: None,
            sail_energy: 1.0,
            crew_speed: 1.0,
            crew_turn: 1.0,
        };
        let e = app
            .world_mut()
            .spawn((ship, tf, health, crate::PlayerShip, crate::ship::FleetMember("Player".into(), 0), crate::campaign::MissionShip { visible: true, ..default() }))
            .id();
        app.update();
        let sensors = app.world().resource::<Sensors>();
        assert_eq!(sensors.primary, Some(e));
        assert_eq!(sensors.objects[&e].sphere, (tf.transform_point(sphere.0), sphere.1));
        let dock = app
            .world_mut()
            .spawn((Transform::from_xyz(100.0, 0.0, 0.0), crate::docking::Dockable::new(vec![crate::docking::DockPoint { pos: Vec3::Z * 3.0, fwd: Vec3::NEG_Z }], true)))
            .id();
        app.update();
        assert_eq!(app.world().resource::<Sensors>().objects[&dock].sphere, (Vec3::X * 100.0, 33.0));
        assert!(app.world().resource::<Sensors>().in_sight(e, dock, None), "collider-free mod docks remain reachable");
        app.world_mut().get_mut::<crate::campaign::MissionShip>(e).unwrap().visible = false;
        app.update();
        assert!(!app.world().resource::<Sensors>().player_sees(e, None));
        app.world_mut().despawn(e);
        app.world_mut().despawn(dock);
        app.update();
        assert!(app.world().resource::<Sensors>().objects.is_empty());
        assert!(app.world().resource::<Sensors>().primary.is_none());
    }

    #[test]
    fn current_ribbon_follows_the_path_and_matches_the_push_width() {
        use bevy::mesh::VertexAttributeValues;
        let path = [Vec3::ZERO, Vec3::new(0.0, 8.0, -240.0), Vec3::new(240.0, 8.0, -480.0)];
        let current = Current::new(path.to_vec());
        let mesh = current_mesh(&current.path);
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("ribbon vertices");
        };
        assert_eq!(positions.len(), path.len() * 2);
        assert_eq!(mesh.indices().unwrap().len(), (path.len() - 1) * 6);
        for (p, vertices) in current.path.iter().zip(positions.as_chunks::<2>().0) {
            let (a, b) = (Vec3::from_array(vertices[0]), Vec3::from_array(vertices[1]));
            assert!(((a + b) / 2.0).distance(*p) < 0.001);
            assert!((a.distance(b) - 2.0 * CURRENT_RADIUS).abs() < 0.001);
        }
        assert_eq!(current.force(Vec3::new(0.0, 0.0, -100.0)), Vec3::NEG_Z * CURRENT_FORCE);
        assert_eq!(current.force(Vec3::new(CURRENT_RADIUS + 1.0, 0.0, -100.0)), Vec3::ZERO);
        assert_eq!(Current::new(vec![]).force(Vec3::ZERO), Vec3::ZERO);
        assert_eq!(current_mesh(&[]).count_vertices(), 0);
    }
    #[test]
    fn mission4_current_pushes_the_way_nelsons_fleet_sails_out() {
        use bevy::mesh::VertexAttributeValues;
        let mission = crate::campaign::load_id("mission04");
        let path = |name: &str| -> Vec<Vec3> { mission.data["paths"][name].as_array().unwrap().iter().map(game_to_bevy).collect() };
        let current = Current::new(path("Mucullough path"));
        let route = path("Nelson exit route");
        let inside: Vec<_> = route.windows(2).filter(|w| current.force(w[0]) != Vec3::ZERO).collect();
        assert!(inside.len() >= 3, "the exit route runs inside the current");
        for w in inside {
            assert!(current.force(w[0]).dot(w[1] - w[0]) > 0.0, "the current carries the fleet toward the frontier");
        }
        let mesh = current_mesh(&current.path);
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("ribbon UVs") };
        assert!(uv.last().unwrap()[1] > uv[0][1], "negative UV scrolling moves the texture downstream, toward the last point");
    }

    #[test]
    fn nebula_energy_drains_and_recovers_without_permanent_damage() {
        assert_eq!(energy_step(1.0, true, 100.0), 0.25);
        assert_eq!(energy_step(0.25, false, 100.0), 1.0);
        assert!(energy_step(1.0, true, 1.0) < 1.0);
    }
    #[test]
    fn cloud_blocks_crossing_and_interior_not_exterior_segments() {
        let polygon = serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]]);
        assert!(crosses(&polygon, Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0)));
        assert!(crosses(&polygon, Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0)));
        assert!(!crosses(&polygon, Vec2::new(3.0, 5.0), Vec2::new(7.0, 5.0)), "same cloud does not obscure its interior");
        assert!(!crosses(&polygon, Vec2::new(-5.0, 15.0), Vec2::new(15.0, 15.0)));
        let concave = serde_json::json!([[0, 0], [10, 0], [10, 10], [6, 10], [6, 4], [4, 4], [4, 10], [0, 10]]);
        assert!(crosses(&concave, Vec2::new(2.0, 8.0), Vec2::new(8.0, 8.0)), "same cloud endpoints can cross its boundary");
    }
    #[test]
    fn enabled_nebula_gates_short_sight_and_group_members_can_see_around_it() {
        let mut mission = crate::campaign::load_id("mission02");
        mission.opening = None;
        mission.script.nis = false;
        mission.data["volumes"]["Sensor test"] = serde_json::json!([[0, 0], [10, 0], [10, 10], [0, 10]]);
        mission.data["environment"]["nebulae"] = serde_json::json!([{"Polygon Name": "Sensor test", "Nebula Occlusion On/Off": "TRUE"}]);
        assert!(!can_see(Some(&mission), Vec3::new(-1.0, 0.0, -5.0), Vec3::new(1.0, 0.0, -5.0)), "no invented 40-unit bypass");
        assert!(can_see(Some(&mission), Vec3::new(2.0, 0.0, -5.0), Vec3::new(8.0, 0.0, -5.0)));
        let (a, b, c) = (Entity::from_bits(1), Entity::from_bits(2), Entity::from_bits(3));
        let mut sensors = Sensors { primary: Some(a), ..default() };
        sensors.objects.insert(a, sensor_object(Vec3::new(-5.0, 0.0, -5.0), 0.0, "A", 100.0));
        sensors.objects.insert(b, sensor_object(Vec3::new(15.0, 0.0, -5.0), 0.0, "B", 100.0));
        assert!(!sensors.player_sees(b, Some(&mission)));
        sensors.objects.insert(c, sensor_object(Vec3::new(15.0, 0.0, -15.0), 0.0, "A", 100.0));
        assert!(sensors.player_sees(b, Some(&mission)));
        sensors.objects.remove(&c);
        mission.data["environment"]["nebulae"][0]["Nebula Occlusion On/Off"] = Value::from("FALSE");
        assert!(sensors.player_sees(b, Some(&mission)));
    }

    #[test]
    fn whale_paths_loop_and_duplicate_points_do_not_hang() {
        let mut animal = SpaceAnimal { speed: 12.0, phase: 0.0, call_in: 3.0, calls: 0, ..SpaceAnimal::new("Whales", 6.0, Vec3::ZERO, 0, true) };
        let mut tf = Transform::default();
        animal.follow(&[Vec3::ZERO, Vec3::X * 12.0], tf.translation, true, true);
        animal.advance(&mut tf, 0.0);
        assert_eq!(tf.translation, Vec3::ZERO);
        assert_eq!(animal.next, 0);
        animal.advance(&mut tf, 1.0);
        assert_eq!(tf.translation, Vec3::X * 12.0);
        animal.advance(&mut tf, 1.0);
        assert_eq!(tf.translation, Vec3::ZERO);
        animal.follow(&[Vec3::ZERO, Vec3::ZERO], tf.translation, false, true);
        animal.advance(&mut tf, 1.0);
        assert_eq!(tf.translation, Vec3::ZERO);
    }
}
