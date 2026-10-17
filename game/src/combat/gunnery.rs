//! Guns: mount hierarchy, turret aim and lead, burst/reload timing, ship and base fire, shell launch (docs/re/gunnery.md).
use super::*;

#[derive(Clone)]
pub(super) struct GunState {
    pub(super) gun: String,
    pub(super) root: Entity,
    pub(super) cooldown: f32,
    pub(super) burst_left: u32,
    pub(super) burst_timer: f32,
    pub(super) base: Entity,
    pub(super) barrel: Entity,
    pub(super) muzzles: Vec<Entity>,
    pub(super) next_muzzle: usize,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) aligned: bool,
    pub(super) in_range: bool, // a target is within range and the mount's arc (weapon bar: green outline)
    pub(super) dev: f32,       // this burst's deviation angle, drawn once when the burst starts (retail: once per burst)
}
/// Marks the entities of a gun's visual hierarchy (root, base, barrel, muzzle points).
#[derive(Component)]
pub(super) struct GunPart;
#[derive(Component)]
pub struct GunStates(pub(super) Vec<GunState>);
impl GunStates {
    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|s| s.gun.clone()).collect()
    }

    pub fn gun(&self, index: usize) -> &str {
        &self.0[index].gun
    }

    pub fn replace(&mut self, commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, mount: &crate::data::Mount, index: usize, gun: &str) {
        commands.entity(self.0[index].root).try_despawn();
        self.0[index] = spawn_gun(commands, assets, lib, ship, mount, gun);
    }

    /// A refit re-creates every mount: loaded, no burst in progress.
    pub fn rearm(&mut self) {
        for g in &mut self.0 {
            (g.cooldown, g.burst_left, g.burst_timer) = (0.0, 0, 0.0);
        }
    }
}
/// Spawns the visual hierarchy of every mount as children of `ship` and returns the gun state (PROVISIONAL: guns start loaded).
pub fn spawn_guns(commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, class: usize) -> GunStates {
    GunStates(lib.classes[class].mounts.iter().map(|m| spawn_gun(commands, assets, lib, ship, m, &m.gun)).collect())
}

pub(super) fn spawn_gun(commands: &mut Commands, assets: &AssetServer, lib: &Library, ship: Entity, m: &crate::data::Mount, gun: &str) -> GunState {
    let scene = |glb: &str| WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(glb))));
    let g = &lib.guns[gun];
    let root = commands.spawn((GunPart, Transform { translation: m.pos, rotation: m.rot, ..default() }, Visibility::default(), ChildOf(ship))).id();
    let base = commands.spawn((GunPart, Transform::default(), Visibility::default(), ChildOf(root))).id();
    if let Some(b) = &g.base {
        commands.entity(base).insert(scene(b));
    }
    // The barrel pitches about its pivot, but its mesh and the muzzle points are authored in the base's frame (on 24 of 26 barrels
    // with a pivot the muzzles sit on the mesh only that way): inside the pivot node they are drawn at minus the pivot.
    let barrel = commands.spawn((GunPart, Transform::from_translation(g.pivot), Visibility::default(), ChildOf(base))).id();
    if let Some(b) = &g.barrel {
        commands.spawn((GunPart, scene(b), Transform::from_translation(-g.pivot), Visibility::default(), ChildOf(barrel)));
    }
    let muzzles = g.muzzles.iter().map(|p| commands.spawn((GunPart, Transform::from_translation(*p - g.pivot), Visibility::default(), ChildOf(barrel))).id()).collect();
    GunState {
        gun: gun.into(),
        root,
        cooldown: 0.0,
        burst_left: 0,
        burst_timer: 0.0,
        base,
        barrel,
        muzzles,
        next_muzzle: 0,
        yaw: 0.0,
        pitch: 0.0,
        aligned: false,
        in_range: false,
        dev: 0.0,
    }
}

// Straight projectile intercept; a slow torpedo needs more than the generic one-step lead.
pub(super) fn intercept_time(d: Vec3, relative_velocity: Vec3, speed: f32) -> Option<f32> {
    let a = relative_velocity.length_squared() - speed * speed;
    let b = 2.0 * d.dot(relative_velocity);
    let c = d.length_squared();
    if c < 0.000001 {
        return Some(0.0);
    }
    if a.abs() < 0.000001 {
        let t = -c / b;
        return (t.is_finite() && t >= 0.0).then_some(t);
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    [(-b - discriminant.sqrt()) / (2.0 * a), (-b + discriminant.sqrt()) / (2.0 * a)].into_iter().filter(|t| t.is_finite() && *t >= 0.0).reduce(f32::min)
}

/// Aim point with lead. Shells and torpedoes leave the gun at their own speed in the world frame (retail: launch speed equals
/// MuzzleSpeed whatever the firing ship's motion), so the lead uses the target's velocity only.
pub(super) fn aim_offset(d: Vec3, target_velocity: Vec3, speed: f32, torpedo: bool) -> Vec3 {
    if torpedo {
        let d = d.with_y(0.0);
        let velocity = target_velocity.with_y(0.0);
        // No reachable straight intercept: aim at the target, not an arbitrarily distant extrapolation.
        return intercept_time(d, velocity, speed).map_or(d, |t| d + velocity * t);
    }
    d + target_velocity * (d.length() / speed)
}

/// Launch velocity: MuzzleSpeed along `dir` in the world frame; the firing ship's velocity is not added (measured on retail
/// cannon, carronade and gattling shells). Torpedoes launch level.
pub(super) fn launch_velocity(dir: Vec3, speed: f32, torpedo: bool) -> Vec3 {
    if torpedo { dir.with_y(0.0).normalize_or_zero() * speed } else { dir * speed }
}

/// Launch direction for speed `s` to reach offset `d` under gravity `g`: the low arc, or straight when `g` is 0.
pub(super) fn aim(d: Vec3, s: f32, g: f32) -> Vec3 {
    if g <= 0.0 {
        return d.normalize();
    }
    let (r, h) = (Vec2::new(d.x, d.z).length(), d.y);
    let disc = s.powi(4) - g * (g * r * r + 2.0 * h * s * s);
    let theta = if disc < 0.0 { std::f32::consts::FRAC_PI_4 } else { ((s * s - disc.sqrt()) / (g * r)).atan() };
    let flat = Vec3::new(d.x, 0.0, d.z).normalize_or_zero();
    flat * theta.cos() + Vec3::Y * theta.sin()
}

/// Launch velocity of a lobbing gun (fixed `angle` above the horizon) that lands on offset `d`: the speed is solved for the
/// angle, as measured on retail star mortar shells (speed ~ sqrt(g * range) at ~45 degrees). None when the point is above
/// the arc. PROVISIONAL: retail elevations scattered 44-50 degrees; whether MuzzleSpeed caps the speed is not observed.
pub(super) fn lob_velocity(d: Vec3, angle: f32, g: f32) -> Option<Vec3> {
    let r = Vec2::new(d.x, d.z).length();
    let rise = r * angle.tan() - d.y;
    if r < 0.5 || rise <= 0.1 {
        return None;
    }
    let speed = (g * r * r / (2.0 * angle.cos().powi(2) * rise)).sqrt();
    let flat = Vec3::new(d.x, 0.0, d.z) / r;
    Some((flat * angle.cos() + Vec3::Y * angle.sin()) * speed)
}

/// Panic fire: the nearest visible enemy ship within this gun's maximum range and mount arc.
/// Retail: each gun finds its own target and fires when possible. PROVISIONAL: nearest-first choice, no gun-class
/// category filter (the original skips some gun classes; which is unknown), ships only.
/// A ship a gun may shoot at, from this frame's snapshot.
#[derive(Clone, Copy)]
pub(super) struct Contact {
    pub(super) entity: Entity,
    pub(super) side: Side,
    /// Where shots go: the clicked section or the hull's aim point, in world space.
    pub(super) aim_point: Vec3,
    /// Alive, script-visible and (once filtered per shooter) in sight.
    pub(super) visible: bool,
    pub(super) is_player: bool,
    pub(super) velocity: Vec3,
}
impl Contact {
    pub(super) fn aim(&self) -> AimTarget {
        AimTarget { entity: self.entity, position: self.aim_point, velocity: self.velocity }
    }
}
/// What a gun aims at: a point, its velocity (for lead) and the entity (`Entity::PLACEHOLDER` for a ground point).
#[derive(Clone, Copy)]
pub(super) struct AimTarget {
    pub(super) entity: Entity,
    pub(super) position: Vec3,
    pub(super) velocity: Vec3,
}

pub(super) fn panic_target(contacts: &[Contact], tf: &Transform, mount: &crate::data::Mount, gun: &crate::data::GunDef) -> Option<AimTarget> {
    let origin = tf.transform_point(mount.pos);
    let to_mount = (tf.rotation * mount.rot).inverse();
    contacts
        .iter()
        .filter(|c| c.side == Side::Enemy && c.visible)
        .filter(|c| {
            let offset = c.aim_point - origin;
            let local = to_mount * offset;
            Vec2::new(offset.x, offset.z).length() <= gun.range[2] && (-local.x).atan2(-local.z).abs() <= mount.arc / 2.0
        })
        .min_by(|a, b| a.aim_point.distance(origin).total_cmp(&b.aim_point.distance(origin)))
        .map(Contact::aim)
}

/// Aims every mount at the chosen target (base yaw, barrel pitch at the gun's rotation speeds) and fires aligned, loaded guns
/// from the muzzle points.
pub(super) fn fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    mut target: ResMut<Target>,
    look: Res<Look>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut fired: MessageWriter<Fired>,
    objects: Query<
        (Entity, &Transform, Option<&crate::environment::SpaceAnimal>, Option<(&ArmedBase, &Health, &crate::campaign::MissionBase)>),
        (Or<(With<Asteroid>, With<crate::environment::SpaceAnimal>, With<ArmedBase>)>, Without<GunPart>),
    >,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut shooters: Query<(
        Entity,
        &Transform,
        &Ship,
        &mut GunStates,
        &Health,
        Option<&crate::campaign::MissionShip>,
        Has<PlayerShip>,
        Option<&crate::ship::FleetPilot>,
        Option<&crate::cloak::Cloak>,
        Option<&crate::crew_arms::CrewPosts>,
    )>,
    others: Query<(Entity, &Transform, &Ship, &Health, Has<PlayerShip>, Option<&crate::campaign::MissionShip>, Option<&crate::cloak::Cloak>)>,
    mut parts: Query<&mut Transform, (With<GunPart>, Without<Ship>)>,
    (globals, threats, points, sensors, captains): (
        Query<&GlobalTransform, With<GunPart>>,
        Query<(Entity, &Transform, &Shell), (Without<GunPart>, Without<Ship>)>,
        Option<Res<crate::campaign::CampaignPoints>>,
        Res<crate::environment::Sensors>,
        Option<Res<crate::ship::CaptainTargets>>,
    ),
) {
    // The human fleet's gunners in a campaign: the roster's best gunner (crew_arms.rs); everyone else the plain-AI factor.
    let crewed = crate::crew_arms::accuracy(points.as_deref());
    let dt = time.delta_secs();
    // Point-defence targets: torpedoes and mines in flight, with their side.
    let incoming: Vec<(AimTarget, Side)> = threats
        .iter()
        .filter(|(_, _, shell)| shell.bullet.starts_with(TORPEDO_BULLET) || matches!(shell.payload, Payload::Mine { .. }))
        .map(|(entity, transform, shell)| (AimTarget { entity, position: transform.translation, velocity: shell.vel }, shell.side))
        .collect();
    // 'NIS Toggle NIS mode Gun Accuracy': scripted broadsides fly true.
    let nis_aim = campaign.as_deref().is_some_and(|m| m.script.nis_accuracy);
    // Script visibility stays separate from observer-dependent detection.
    let contacts: Vec<Contact> = others
        .iter()
        .map(|(entity, transform, ship, health, is_player, mission_ship, _)| Contact {
            entity,
            side: ship.side,
            aim_point: transform.transform_point(ship_aim_point(&lib.classes[ship.class], health, if Some(entity) == target.0 { target.1 } else { None })),
            visible: !health.destroyed && mission_ship.is_none_or(|m| m.visible),
            is_player,
            velocity: transform.rotation * Vec3::NEG_Z * ship.speed,
        })
        .collect();
    let rocks: Vec<AimTarget> = objects
        .iter()
        .filter(|(_, _, a, base)| {
            a.is_none_or(|a| a.visible && campaign.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &a.owner)))
                && base.is_none_or(|(_, _, b)| campaign.as_deref().is_some_and(|m| m.script.hostile(m.script.player(), &b.owner)))
        })
        .filter_map(|(entity, transform, animal, base)| {
            let position = match base {
                Some((b, h, _)) => transform.transform_point(base_aim_point(&lib.classes[b.class], h, (target.0 == Some(entity)).then_some(target.1).flatten())?),
                None => transform.translation,
            };
            Some(AimTarget { entity, position, velocity: animal.map_or(Vec3::ZERO, |a| a.velocity) })
        })
        .collect();
    for (me, tf, ship, mut states, health, mission, primary, pilot, cloak, posts) in &mut shooters {
        if health.destroyed {
            continue;
        }
        let reload_cut = gunner_reload_cut(ship, posts, points.as_deref(), &lib);
        let seen: Vec<Contact> = contacts.iter().map(|c| Contact { visible: c.visible && sensors.in_sight(me, c.entity, campaign.as_deref()), ..*c }).collect();
        let cloaked_now = cloak.as_ref().is_some_and(|c| c.level > 0.0);
        let class = &lib.classes[ship.class];
        // Player's command ship: only the clicked target (retail: direct-fire guns need an enemy click or panic fire). Its other
        // ships: the nearest living enemy. Enemies: their captain's target.
        // Mission ships other than the player's fire at the target the rules' AI chose (rules::mission_ai).
        let ship_target = if pilot.is_some_and(|p| p.hold_fire) || mission.is_some_and(|m| m.hold_fire) {
            None
        } else if let Some(t) = pilot.filter(|_| !primary).and_then(|p| p.target) {
            seen.iter().find(|c| c.entity == t && c.side != ship.side && c.visible).map(Contact::aim).or_else(|| rocks.iter().find(|r| r.entity == t).copied())
        } else if let (Some(m), false) = (mission, primary) {
            m.target.and_then(|t| seen.iter().find(|c| c.entity == t && c.visible).map(Contact::aim).or_else(|| rocks.iter().find(|r| r.entity == t).copied()))
        } else if let (Side::Player, Some(r)) = (ship.side, target.0.filter(|_| primary).and_then(|t| rocks.iter().find(|r| r.entity == t))) {
            Some(*r) // selected space object (asteroid or whale)
        } else {
            match ship.side {
                Side::Player => seen.iter().find(|c| primary && Some(c.entity) == target.0 && c.side == Side::Enemy && c.visible).or_else(|| {
                    seen.iter()
                        .filter(|c| !primary && c.side == Side::Enemy && c.visible)
                        .min_by(|a, b| a.aim_point.distance(tf.translation).total_cmp(&b.aim_point.distance(tf.translation)))
                }),
                // Skirmish computer ships fire at their captain's target (ship::CaptainTargets); without one, the player ship.
                Side::Enemy => match captains.as_deref().and_then(|c| c.of(me)) {
                    Some(t) => seen.iter().find(|c| c.entity == t && c.visible),
                    None => seen.iter().find(|c| c.is_player && c.visible),
                },
            }
            .map(Contact::aim)
        };
        let ship_target = ship_target.filter(|t| sensors.in_sight(me, t.entity, campaign.as_deref()));
        let pending = if primary { target.2.shot } else { None };
        let (mut shot_fired, mut shot_engaged) = (false, false);
        for (mount, state) in class.mounts.iter().zip(states.0.iter_mut()) {
            // The player's indirect guns only fire at a clicked ground point.
            let on_shot = pending.is_some_and(|s| s.0 == mount.bank);
            let gun = &lib.guns[&state.gun];
            let gun_target = match (primary && is_indirect(&state.gun), pending) {
                // Point defence: the nearest hostile torpedo or mine within its maximum range (retail: PD guns never fire at ships).
                _ if gun.point_defense => incoming
                    .iter()
                    .filter(|(threat, side)| *side != ship.side && threat.position.distance(tf.transform_point(mount.pos)) <= gun.range[2])
                    .min_by(|a, b| a.0.position.distance(tf.translation).total_cmp(&b.0.position.distance(tf.translation)))
                    .map(|(threat, _)| *threat),
                (false, _) if primary && target.2.panic => panic_target(&seen, tf, mount, gun).or(ship_target),
                (false, _) => ship_target,
                (true, Some((bank, point))) if bank == mount.bank => Some(AimTarget { entity: Entity::PLACEHOLDER, position: point, velocity: Vec3::ZERO }),
                _ => None,
            };
            // PROVISIONAL (owner retail recollection): torpedoes cannot target base towers.
            let gun_target = gun_target.filter(|t| gun.bullet != TORPEDO_BULLET || !objects.get(t.entity).is_ok_and(|o| o.3.is_some()));
            state.cooldown -= dt;
            state.burst_timer -= dt;
            let mount_rot = tf.rotation * mount.rot;
            let origin = tf.transform_point(mount.pos);
            let dead = mount.parent.is_some_and(|p| health.sections[p] <= 0.0);
            // Harpoons are grapples, fired by the Grapple order (boarding.rs) and never at will. PROVISIONAL: the original's
            // grapple-gun exclusion is unknown. A dead mount section rests its gun.
            let aim_at = gun_target.filter(|_| !dead && !state.gun.contains("Harpoon"));
            let engaged = aim_turret(state, gun, mount, mount_rot, origin, aim_at, dt, &mut parts);
            shot_engaged |= on_shot && engaged.is_some();
            let Some((aim_dir, distance)) = engaged else {
                state.burst_left = 0;
                continue;
            };
            // Guns that cannot fire while cloaked hold, unless the rules let this AI drop its cloak to shoot.
            if cloaked_now && !gun.cloak_fire {
                if cloak.is_some_and(|cloak| cloak.ai_override && cloak.on && !primary) && state.cooldown <= 0.0 {
                    commands.queue(move |w: &mut World| {
                        if let Some(mut c) = w.get_mut::<crate::cloak::Cloak>(me) {
                            c.on = false;
                        }
                    });
                }
                state.burst_left = 0;
                continue;
            }
            if state.burst_left == 0 && state.cooldown <= 0.0 && state.aligned {
                state.burst_left = gun.burst;
                shot_fired |= on_shot;
                if !gun.point_defense {
                    let target = gun_target.map(|t| t.entity).filter(|e| *e != Entity::PLACEHOLDER);
                    fired.write(Fired { shooter: me, target, bullet: gun.bullet.clone() });
                }
                state.cooldown = gun.burst_cycle_cut(reload_cut);
                state.burst_timer = 0.0;
                // Accuracy factor: 0.5 for the plain AI; crewed ships use 1 - 0.8 * gunnery skill (the campaign roster's best gunner for
                // the human fleet).
                let human = campaign.as_deref().is_some_and(|c| mission.is_some_and(|ms| ms.owner == c.script.player()));
                let factor = if human { posts.map_or(crewed, |p| p.skill(0, "gunnery").map_or(0.5, |skill| 1.0 - 0.8 * skill)) } else { 0.5 };
                state.dev = if nis_aim { 0.0 } else { burst_deviation(gun, distance, factor, &mut rng) };
            }
            if state.burst_left > 0 && state.burst_timer <= 0.0 {
                state.burst_left -= 1;
                state.burst_timer = gun.burst_reload;
                let turn = burst_turn(aim_dir, state.dev, &mut rng);
                let dir = turn * aim_dir;
                let muzzle = state.muzzles[state.next_muzzle % state.muzzles.len()];
                state.next_muzzle += 1;
                let side = ship.side;
                let goal = gun_target.map(|t| t.position);
                let shot = Launch {
                    vel: gun.lob.and_then(|a| lob_velocity(goal? - origin, a, gun.shell_gravity())).map(|v| turn * v),
                    aim: goal.filter(|_| is_indirect(&state.gun)),
                    pd: gun.point_defense,
                    harmless: mission.is_some_and(|ms| ms.warning_shots),
                };
                launch_shell(
                    &mut commands,
                    &assets,
                    &lib,
                    &fxl,
                    &look,
                    &mut sfx,
                    gun,
                    muzzle,
                    globals.get(muzzle).ok().copied(),
                    origin,
                    dir,
                    side,
                    me,
                    gun_target.map(|t| t.entity),
                    shot,
                );
            }
        }
        // One click, one shot: drop the point once fired, or when the bank cannot reach it.
        if pending.is_some() && (shot_fired || !shot_engaged) {
            target.2.shot = None;
        }
    }
}

/// Per-shot extras of `launch_shell`: a solved lob velocity, the indirect aim point, point-defence and warning-shot flags.
#[derive(Default)]
pub(super) struct Launch {
    pub(super) vel: Option<Vec3>,
    pub(super) aim: Option<Vec3>,
    pub(super) pd: bool,
    pub(super) harmless: bool,
}

/// Spawns one shell of gun `g` from muzzle `mz` (its fire sound, muzzle flash and bullet effect included).
pub(super) fn launch_shell(
    commands: &mut Commands,
    assets: &AssetServer,
    lib: &Library,
    fxl: &FxLib,
    look: &Look,
    sfx: &mut MessageWriter<Sfx>,
    g: &GunDef,
    mz: Entity,
    mgt: Option<GlobalTransform>,
    origin: Vec3,
    dir: Vec3,
    side: Side,
    shooter: Entity,
    target: Option<Entity>,
    shot: Launch,
) {
    let from = mgt.map(|t| t.translation()).unwrap_or(origin);
    let b = &lib.bullets[&g.bullet];
    // PROVISIONAL: one fire sound per shell of a burst.
    if let Some((n, vol, max)) = &g.sound {
        sfx.write(Sfx { name: n.clone(), at: Some(from), vol: *vol, max: *max });
    }
    let mat = if side == Side::Player { look.shell.clone() } else { look.enemy_shell.clone() };
    let vel = shot.vel.unwrap_or_else(|| launch_velocity(dir, g.muzzle, g.bullet == TORPEDO_BULLET));
    let stf = Transform::from_translation(from).looking_to(vel, Vec3::Y);
    let shell = commands
        .spawn((
            Shell {
                vel,
                age: 0.0,
                lifetime: b.lifetime,
                // Mines drift.
                gravity: if matches!(b.payload, Payload::Mine { .. }) { 0.0 } else { g.shell_gravity() },
                bullet: g.bullet.clone(),
                side,
                shooter,
                target,
                angular_velocity: 0.0,
                payload: b.payload,
                aim: shot.aim,
                pd: shot.pd,
                hp: b.hitpoints,
                harmless: shot.harmless,
            },
            Transient,
            stf,
        ))
        .id();
    if let Some((name, vol, max)) = &b.travel_snd {
        commands.entity(shell).insert(crate::audio::Travel { name: name.clone(), vol: *vol, max: *max });
    }
    if let Some(glb) = &b.glb {
        commands.entity(shell).insert(WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(crate::data::model(glb)))));
    } else if b.bullet_fx.is_none() {
        // fx-only shells (Lancer sprite) get no placeholder mesh
        commands.entity(shell).insert((Mesh3d(look.mesh.clone()), MeshMaterial3d(mat)));
    }
    if let Some(n) = &b.bullet_fx {
        fx::spawn(commands, fxl, n, Some(shell), Transform::default(), stf, false);
    }
    if let (Some(n), Some(mg)) = (&g.muzzle_fx, mgt) {
        fx::spawn(commands, fxl, n, Some(mz), Transform::default(), mg.compute_transform(), false);
    }
}

/// Island gun towers: every mount of an armed base shoots the nearest visible ship its owner is hostile to, within the gun's range
/// (turret model as in `fire`). PROVISIONAL: the base's AI stance and 'attack' orders are not consulted; hostility alone arms it.
pub(super) fn base_fire(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    fxl: Res<FxLib>,
    look: Res<Look>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut fired: MessageWriter<Fired>,
    campaign: Option<Res<crate::campaign::Mission>>,
    mut bases: Query<(Entity, &Transform, &crate::campaign::MissionBase, &ArmedBase, &Health, &mut GunStates), Without<GunPart>>,
    ships: Query<(&Transform, &Ship, &Health, Option<&crate::campaign::MissionShip>, Entity)>,
    mut parts: Query<&mut Transform, (With<GunPart>, Without<Ship>)>,
    (globals, sensors): (Query<&GlobalTransform, With<GunPart>>, Res<crate::environment::Sensors>),
) {
    let Some(mission) = campaign else { return };
    let dt = time.delta_secs();
    for (me, tf, base, armed, health, mut states) in &mut bases {
        let class = &lib.classes[armed.class];
        // The nearest visible hostile ship in sight, with its side.
        let target = ships
            .iter()
            .filter(|(_, _, h, m, _)| !h.destroyed && m.is_some_and(|m| m.visible && mission.script.hostile(&base.owner, &m.owner)))
            .map(|(transform, ship, _, _, entity)| {
                let position = transform.transform_point(lib.classes[ship.class].centre);
                (AimTarget { entity, position, velocity: transform.rotation * Vec3::NEG_Z * ship.speed }, ship.side)
            })
            .filter(|(t, _)| sensors.in_sight(me, t.entity, Some(&mission)))
            .min_by(|a, b| a.0.position.distance(tf.translation).total_cmp(&b.0.position.distance(tf.translation)));
        for (mount, state) in class.mounts.iter().zip(states.0.iter_mut()) {
            state.cooldown -= dt;
            state.burst_timer -= dt;
            let gun = &lib.guns[&mount.gun];
            let (mount_rot, origin) = (tf.rotation * mount.rot, tf.transform_point(mount.pos));
            let dead = mount.parent.is_some_and(|p| health.sections[p] <= 0.0);
            // Mortar towers lob at the ship (PROVISIONAL: mines/grav charges on bases are not seen in the data and stay idle).
            let usable = !is_indirect(&mount.gun) || gun.lob.is_some();
            let aim_at = target.map(|t| t.0).filter(|_| !dead && !gun.point_defense && !mount.gun.contains("Harpoon") && usable);
            let engaged = aim_turret(state, gun, mount, mount_rot, origin, aim_at, dt, &mut parts);
            let (Some((aim_dir, distance)), Some((aim_at, target_side))) = (engaged, target) else {
                state.burst_left = 0;
                continue;
            };
            if state.burst_left == 0 && state.cooldown <= 0.0 && state.aligned {
                state.burst_left = gun.burst;
                fired.write(Fired { shooter: me, target: Some(aim_at.entity), bullet: gun.bullet.clone() });
                state.cooldown = gun.burst_cycle();
                state.burst_timer = 0.0;
                state.dev = burst_deviation(gun, distance, 0.5, &mut rng);
            }
            if state.burst_left > 0 && state.burst_timer <= 0.0 {
                state.burst_left -= 1;
                state.burst_timer = gun.burst_reload;
                let turn = burst_turn(aim_dir, state.dev, &mut rng);
                let muzzle = state.muzzles[state.next_muzzle % state.muzzles.len()];
                state.next_muzzle += 1;
                // The shell must hit ships of the target's side; the shooter's own side is the other one.
                let side = if target_side == Side::Player { Side::Enemy } else { Side::Player };
                let shot = Launch {
                    vel: gun.lob.and_then(|a| lob_velocity(aim_at.position - origin, a, gun.shell_gravity())).map(|v| turn * v),
                    aim: is_indirect(&mount.gun).then_some(aim_at.position),
                    ..default()
                };
                let muzzle_transform = globals.get(muzzle).ok().copied();
                launch_shell(&mut commands, &assets, &lib, &fxl, &look, &mut sfx, gun, muzzle, muzzle_transform, origin, turn * aim_dir, side, me, Some(aim_at.entity), shot);
            }
        }
    }
}

/// Turns a turret toward `aim_at` (docs/re/gunnery.md): the base always turns toward the target at the gun's yaw speed, clamped to
/// the mount's half-arc, whatever the range; the barrel only elevates once the base is within 1 degree and the target is within
/// range. Lead as the original does: aim point += target velocity * (distance / muzzle speed), one step, no gravity term (torpedo
/// lead is straight interception, not the original's homing). PROVISIONAL: with no target the gun returns to rest; the original's
/// idle behaviour is unknown. Returns the firing direction and distance while the target is in range and arc.
#[allow(clippy::too_many_arguments)]
fn aim_turret(
    state: &mut GunState,
    gun: &GunDef,
    mount: &crate::data::Mount,
    mount_rot: Quat,
    origin: Vec3,
    aim_at: Option<AimTarget>,
    dt: f32,
    parts: &mut Query<&mut Transform, (With<GunPart>, Without<Ship>)>,
) -> Option<(Vec3, f32)> {
    const TOLERANCE: f32 = std::f32::consts::PI / 180.0;
    let half_arc = mount.arc / 2.0; // already converted to radians when loading the mount
    let (mut want_yaw, mut want_pitch, mut engaged) = (0.0f32, Some(0.0f32), None);
    if let Some(aim_at) = aim_at {
        let to_target = aim_offset(aim_at.position - origin, aim_at.velocity, gun.muzzle, gun.bullet == TORPEDO_BULLET);
        let dir = match gun.lob.and_then(|angle| lob_velocity(to_target, angle, gun.shell_gravity())) {
            Some(velocity) => velocity.normalize(),
            None => aim(to_target, gun.muzzle, gun.shell_gravity()),
        };
        let local = mount_rot.inverse() * dir;
        let yaw = (-local.x).atan2(-local.z);
        want_yaw = yaw.clamp(-half_arc, half_arc);
        let in_range = Vec2::new(to_target.x, to_target.z).length() <= gun.range[2];
        want_pitch = None; // hold elevation until the base is on target
        if (state.yaw - want_yaw).abs() < TOLERANCE && in_range {
            want_pitch = Some(local.y.clamp(-1.0, 1.0).asin().clamp(gun.pitch_limits[0], gun.pitch_limits[1]));
        }
        if in_range && yaw.abs() <= half_arc {
            engaged = Some((dir, to_target.length()));
        }
    }
    let yaw_step = (want_yaw - state.yaw).clamp(-gun.yaw_speed * dt, gun.yaw_speed * dt);
    state.yaw = (state.yaw + yaw_step).clamp(-half_arc.max(0.0), half_arc);
    if let Some(pitch) = want_pitch {
        state.pitch += (pitch - state.pitch).clamp(-gun.pitch_speed * dt, gun.pitch_speed * dt);
    }
    state.in_range = engaged.is_some();
    state.aligned = engaged.is_some() && (state.yaw - want_yaw).abs() < TOLERANCE && want_pitch.is_some_and(|p| (state.pitch - p).abs() < TOLERANCE);
    if let Ok(mut part) = parts.get_mut(state.base) {
        part.rotation = Quat::from_rotation_y(state.yaw);
    }
    if let Ok(mut part) = parts.get_mut(state.barrel) {
        part.rotation = Quat::from_rotation_x(state.pitch);
    }
    engaged
}

/// A burst's deviation angle, drawn once per burst: the range band's deviation * 0.5, uniform in +-that, times `accuracy`.
fn burst_deviation(gun: &GunDef, distance: f32, accuracy: f32, rng: &mut Rng) -> f32 {
    let band = if distance <= gun.range[0] {
        gun.deviation[0]
    } else if distance <= gun.range[1] {
        gun.deviation[1]
    } else {
        gun.deviation[2]
    };
    (rng.f() * 2.0 - 1.0) * band * 0.5 * accuracy
}

/// One shot's turn off the aim: the burst's angle about two muzzle axes, each with a random sign.
fn burst_turn(aim_dir: Vec3, deviation: f32, rng: &mut Rng) -> Quat {
    let sign = |r: f32| if r < 0.5 { -1.0 } else { 1.0 };
    let right = aim_dir.cross(Vec3::Y).normalize_or(Vec3::X);
    Quat::from_axis_angle(Vec3::Y, sign(rng.f()) * deviation) * Quat::from_axis_angle(right, sign(rng.f()) * deviation)
}

/// Damage rules of one shell hit on section `k` (`damage` = [lower, upper, crit, fire chance, fire strength]).
/// Criticals are not modelled: in the original they only feed a tally and crew speech (docs/re/movement.md).
/// Seconds the gunners take off TimeBetweenBursts: 10 x gunnery (0..1), for the player's crewed ships only; computer
/// ships fire every TimeBetweenBursts (measured). Measured on retail: gunners of gunnery 7-9 brought one gun's hits down to
/// every 3.5-3.9 s (heavy cannon, 12 s). PROVISIONAL: each retail gun reads its own gunner slot (mapping unknown); here the
/// mean over the gunner slots (generic hands 0.1), or the campaign roster's best gunner without posts.
pub(super) fn gunner_reload_cut(ship: &Ship, posts: Option<&crate::crew_arms::CrewPosts>, points: Option<&crate::campaign::CampaignPoints>, lib: &Library) -> f32 {
    if ship.side != Side::Player {
        return 0.0;
    }
    let gunnery = match posts {
        Some(p) => p.post_skill(0, "gunnery", crate::boarding::counts(&lib.classes[ship.class].name)[0]),
        None => points.and_then(|p| crate::crew_arms::best(p, "gunnery")).unwrap_or(0.1),
    };
    10.0 * gunnery
}
