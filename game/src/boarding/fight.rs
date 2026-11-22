//! The boarding fight: boarders, messages, casualties per tick and capture.
use super::*;

/// What the menu and the AI need to know about boarding; the original's boarding tests in one place.
#[derive(SystemParam)]
pub struct Boarders<'w, 's> {
    bodies: Query<'w, 's, (Body<'static>, &'static Boardable)>,
    owners: Query<'w, 's, (Option<&'static MissionShip>, Option<&'static MissionBase>)>,
    base_sides: Query<'w, 's, &'static crate::combat::BaseSide>,
    fighters: Query<'w, 's, (Entity, &'static Boarding)>,
    grapples: Query<'w, 's, (Entity, &'static Grapple)>,
    ships: Query<'w, 's, (&'static Ship, &'static Health, Option<&'static crate::combat::GunStates>)>,
    player: Query<'w, 's, Entity, With<PlayerShip>>,
    lib: Res<'w, Library>,
    mission: Option<Res<'w, Mission>>,
}
impl Boarders<'_, '_> {
    fn who(&self, e: Entity) -> (Option<&str>, Option<Side>) {
        (self.owners.get(e).ok().and_then(|(ms, mb)| owner_of(ms, mb)), self.ships.get(e).ok().map(|s| s.0.side).or_else(|| self.base_sides.get(e).ok().map(|b| b.0)))
    }
    pub fn hostile(&self, a: Entity, b: Entity) -> bool {
        hostile_pair(self.mission.as_deref(), self.who(a), self.who(b))
    }
    /// Whether `a`'s line of fire to `end` is held for a friendly ship or scenery (line_of_fire.rs); `target` is exempt from the scenery test.
    pub fn line_blocked(
        &self,
        a: Entity,
        end: Vec3,
        target: Entity,
        ships: &Query<(Entity, &Transform, &Ship)>,
        scenery: &Query<(Entity, &crate::collision::Obstacle, Option<&Visibility>, Has<crate::combat::Asteroid>), (Without<Ship>, Without<crate::combat::GunPart>)>,
    ) -> bool {
        let Ok((_, tf, ship)) = ships.get(a) else { return false };
        let class = &self.lib.classes[ship.class];
        let friends = ships.iter().filter(|o| o.0 != a).filter_map(|(e, otf, os)| {
            let oc = &self.lib.classes[os.class];
            let friendly = class.is_lifeboat || oc.is_lifeboat || allied_pair(self.mission.as_deref(), self.who(a), self.who(e));
            friendly.then(|| (otf.transform_point(oc.sphere.0), oc.sphere.1))
        });
        crate::line_of_fire::friend_in_line(tf.translation, end, friends)
            || crate::line_of_fire::scenery_in_line(
                tf.translation,
                end,
                class.radius,
                scenery.iter().filter(|(_, _, v, _)| *v != Some(&Visibility::Hidden)).map(|(e, o, _, rock)| crate::line_of_fire::Scenery {
                    sphere: o.sphere(),
                    mesh: (!rock).then_some(o),
                    target: e == target,
                }),
            )
    }
    pub fn grappled(&self, a: Entity, b: Entity) -> bool {
        self.grapples.iter().any(|(_, g)| g.flight <= 0.0 && ((g.grappler == a && g.target == b) || (g.grappler == b && g.target == a)))
    }
    pub fn boarding(&self, boarder: Entity, target: Entity) -> bool {
        self.fighters.iter().any(|(e, f)| e == boarder && f.target == target)
    }
    /// Hostile, not already fighting, the target's Boardable flag on, and a grapple (ship) / in range (base).
    pub fn can_board(&self, boarder: Entity, target: Entity) -> bool {
        let (Ok((bb, _)), Ok((tb, t))) = (self.bodies.get(boarder), self.bodies.get(target)) else { return false };
        let alive = |e: Entity| self.ships.get(e).is_ok_and(|s| !s.1.destroyed);
        t.on && boarder != target
            && alive(boarder)
            && (tb.1.is_none() || alive(target))
            && !self.boarding(boarder, target)
            && self.hostile(boarder, target)
            && if tb.1.is_some() { self.grappled(boarder, target) } else { in_range(&self.lib, &bb, &tb, &t.points) }
    }
    /// The ship has a harpoon (a live gun section) and fewer live grapples (flying or bitten) than its limit.
    pub fn can_grapple(&self, e: Entity) -> bool {
        self.ships.get(e).is_ok_and(|(s, h, guns)| {
            !h.destroyed && has_harpoon(&self.lib, s, h, guns) && self.grapples.iter().filter(|(_, g)| g.grappler == e).count() < grapple_limit(&self.lib, s, h, guns)
        })
    }
    pub fn grappling(&self, grappler: Entity, target: Entity) -> bool {
        self.grapples.iter().any(|(_, g)| g.grappler == grappler && g.target == target)
    }
    pub fn me(&self) -> Option<Entity> {
        self.player.single().ok()
    }
}

const CREW_LINES: [(&str, &[&str]); 11] = [
    ("boarding_party", &["hmfirstmate02", "hmfirstmate02b"]),
    ("landing_party", &["hmfirstmate16", "hmfirstmate16b"]),
    ("repell_boarders", &["hmfirstmate01", "hmfirstmate01b"]),
    ("own_ship_boarded", &["hmfirstmate23"]),
    ("enemy_ship_conquered", &["hmfirstmate19"]),
    ("enemy_base_conquered", &["hmfirstmate18"]),
    ("own_ship_conquered", &["hmfirstmate22"]),
    ("allied_base_conquered", &["hmfirstmate21"]),
    ("boarding_repelled", &["hmfirstmate27", "hmfirstmate27b"]),
    ("retreat", &["hmfirstmate20", "hmfirstmate20b"]),
    ("grappled", &["hmfirstmate24"]),
];
/// PROVISIONAL: the Navy human-male first mate speaks (as the helmsman does), the event picked by meaning rather than the original's id;
/// `r` (0..1) picks between a line's variants.
pub(super) fn say(commands: &mut Commands, assets: Option<&AssetServer>, event: &str, r: f32) {
    let (Some(assets), Some((_, files))) = (assets, CREW_LINES.iter().find(|l| l.0 == event)) else { return };
    commands.spawn((
        AudioPlayer::new(assets.load(format!("voice/{}.ogg", files[(r * files.len() as f32) as usize % files.len()]))),
        PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
    ));
}

fn end_fight(commands: &mut Commands, e: Entity, fx: &[Entity]) {
    for f in fx {
        if let Ok(mut c) = commands.get_entity(*f) {
            c.despawn();
        }
    }
    commands.entity(e).remove::<Boarding>();
}

pub(super) fn handle_messages(mut msgs: MessageReader<BoardMsg>, mut commands: Commands, assets: Option<Res<AssetServer>>, mut rng: ResMut<Rng>, boarders: Boarders) {
    let me = boarders.me();
    for m in msgs.read() {
        match *m {
            BoardMsg::Board { boarder, target } => {
                let Some(b) = boarder.or(me) else { continue };
                if !boarders.can_board(b, target) {
                    continue;
                }
                let (Ok((_, bt)), Ok((_, tt))) = (boarders.bodies.get(b), boarders.bodies.get(target)) else { continue };
                let tick = interval(&bt.strength, &tt.strength, rng.f());
                info!("boarding: {b} boards {target} (attack {} against defend {}, a casualty every {tick:.2}s)", bt.strength.attack, tt.strength.defend);
                commands.entity(b).insert(Boarding { target, interval: tick, since: 0.0, live: false, fx: vec![] });
            }
            BoardMsg::Unboard { boarder } => {
                let Some(b) = boarder.or(me) else { continue };
                if let Some((_, f)) = boarders.fighters.iter().find(|(e, _)| *e == b) {
                    end_fight(&mut commands, b, &f.fx);
                    if Some(b) == me {
                        say(&mut commands, assets.as_deref(), "retreat", rng.f());
                    }
                }
            }
            BoardMsg::Grapple { grappler, target } => {
                let Some(g) = grappler.or(me) else { continue };
                if g != target && boarders.can_grapple(g) && !boarders.grappling(g, target) && boarders.hostile(g, target) && boarders.ships.contains(target) {
                    commands.entity(g).insert(GrappleOrder(target));
                }
            }
            BoardMsg::Ungrapple { grappler } => {
                let Some(g) = grappler.or(me) else { continue };
                commands.entity(g).remove::<GrappleOrder>();
                for (h, _) in boarders.grapples.iter().filter(|(_, gr)| gr.grappler == g) {
                    commands.entity(h).despawn();
                }
                // A fight needs the grapple: it ends with it.
                for (e, f) in boarders.fighters.iter().filter(|(e, f)| (*e == g || f.target == g) && boarders.bodies.get(f.target).is_ok_and(|b| b.0.1.is_some())) {
                    end_fight(&mut commands, e, &f.fx);
                }
            }
            BoardMsg::Boardable(..) => {}
            BoardMsg::Scripted { boarder, target } => {
                if boarders.boarding(boarder, target) {
                    continue;
                }
                if boarders.can_board(boarder, target) {
                    // The grapple already holds (or the target is a base in reach): the fight starts as from the menu.
                    commands.entity(boarder).insert(ScriptedBoard(target));
                } else if boarders.grappled(boarder, target) || boarders.grappling(boarder, target) {
                    commands.entity(boarder).insert(ScriptedBoard(target)); // waits for the Boardable flag, the range or the hook's flight
                } else if boarder != target && boarders.can_grapple(boarder) && boarders.hostile(boarder, target) && boarders.ships.contains(target) {
                    commands.entity(boarder).insert((GrappleOrder(target), ScriptedBoard(target)));
                } else {
                    warn!("OP Board Ship: {boarder} cannot board {target} (a ship needs a harpoon and a hostile ship to grapple; a base must be hostile and in reach)");
                }
            }
        }
    }
}

/// Carries out `OP Board Ship`: the boarder sails and grapples as ordered, then boards by the same message the menu and the AI send.
/// The order ends when the fight is registered, when either ship is gone or destroyed, or when no grapple is left on the target (a
/// snapped rope or a failed harpoon: fire the rule again).
pub(super) fn scripted_boarding(mut commands: Commands, mut msg: MessageWriter<BoardMsg>, boarders: Boarders, orders: Query<(Entity, &ScriptedBoard, Has<GrappleOrder>)>) {
    for (e, ScriptedBoard(t), ordered) in &orders {
        let alive = |x: Entity| boarders.ships.get(x).is_ok_and(|s| !s.1.destroyed);
        if !alive(e) || (boarders.ships.contains(*t) && !alive(*t)) || boarders.bodies.get(*t).is_err() || boarders.boarding(e, *t) {
            commands.entity(e).remove::<ScriptedBoard>();
        } else if boarders.can_board(e, *t) {
            msg.write(BoardMsg::Board { boarder: Some(e), target: *t });
            commands.entity(e).remove::<ScriptedBoard>();
        } else if !ordered && !boarders.grappling(e, *t) && boarders.ships.contains(*t) {
            info!("OP Board Ship: {e} lost its grapple on {t}; the order is dropped");
            commands.entity(e).remove::<ScriptedBoard>();
        }
    }
}

pub(super) fn set_flags(mut msgs: MessageReader<BoardMsg>, mut flags: Query<&mut Boardable>) {
    for m in msgs.read() {
        if let BoardMsg::Boardable(e, on) = *m
            && let Ok(mut b) = flags.get_mut(e)
        {
            b.on = on;
        }
    }
}

/// A chance per tick (0.033 s) turned into one for a frame of `dt` seconds.
pub(super) fn per_tick(chance: f32, dt: f32) -> f32 {
    1.0 - (1.0 - chance).powf(dt / crate::TICK_SECS)
}

/// Runs the casualties of every fight and ends those whose conditions lapsed.
pub(super) fn fights(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    assets: Option<Res<AssetServer>>,
    fxl: Option<Res<FxLib>>,
    mut sfx: MessageWriter<Sfx>,
    mission: Option<Res<Mission>>,
    mut settled: MessageWriter<Settled>,
    mut fighters: Query<(Entity, &mut Boarding)>,
    grapples: Query<(Entity, &Grapple)>,
    player: Query<Entity, With<PlayerShip>>,
    mut things: Query<(&Transform, Option<&Ship>, Option<&Drift>, &mut Boardable, Option<&Health>, Option<&MissionShip>, Option<&MissionBase>, Option<&crate::combat::BaseSide>)>,
) {
    let (dt, me) = (time.delta_secs(), player.single().ok());
    for (b, mut f) in &mut fighters {
        let t = f.target;
        let Ok([mut bi, mut ti]) = things.get_many_mut([b, t]) else {
            end_fight(&mut commands, b, &f.fx);
            continue;
        };
        let alive = |h: Option<&Health>| h.is_none_or(|h| !h.destroyed);
        let hostile = hostile_pair(
            mission.as_deref(),
            (owner_of(bi.5, bi.6), bi.1.map(|s| s.side).or(bi.7.map(|b| b.0))),
            (owner_of(ti.5, ti.6), ti.1.map(|s| s.side).or(ti.7.map(|b| b.0))),
        );
        let joined = ti.1.is_none() || grapples.iter().any(|(_, g)| g.flight <= 0.0 && ((g.grappler == b && g.target == t) || (g.grappler == t && g.target == b)));
        let near = in_range(&lib, &(bi.0, bi.1, bi.2), &(ti.0, ti.1, ti.2), &ti.3.points);
        if !alive(bi.4) || !alive(ti.4) || !hostile || !joined || (f.live && !near) {
            let (surface, rel) =
                (bi.0.translation.distance(ti.0.translation) - radius(&lib, bi.1) - radius(&lib, ti.1), (velocity(&(bi.0, bi.1, bi.2)) - velocity(&(ti.0, ti.1, ti.2))).length());
            info!("boarding: {b} breaks off {t} (hostile {hostile}, joined {joined}, surface {surface:.1}, relative speed {rel:.1})");
            end_fight(&mut commands, b, &f.fx);
            if Some(b) == me {
                say(&mut commands, assets.as_deref(), "retreat", rng.f());
            }
            continue;
        }
        // PROVISIONAL: the order to board a grappled ship waits for the winch to close the hulls instead of failing at once.
        if !near {
            continue;
        }
        if !f.live {
            // The crews meet: the fight effect plays on every boarding point of the target, and the first mates speak.
            f.live = true;
            f.fx = fxl.as_ref().map_or(vec![], |fxl| {
                ti.3.points
                    .iter()
                    .filter_map(|p| {
                        let local = Transform::from_translation(*p);
                        crate::fx::spawn(&mut commands, fxl, "Boarding_Fight", Some(t), local, *ti.0 * local, false)
                    })
                    .collect()
            });
            sfx.write(Sfx { name: "Ship_Boarding_Party".into(), at: Some(bi.0.translation), vol: 1.0, max: 600.0 });
            let line = if Some(b) == me {
                Some(if ti.1.is_some() { "boarding_party" } else { "landing_party" })
            } else if Some(t) == me {
                Some("repell_boarders")
            } else if ti.1.is_some_and(|s| s.side == Side::Player) {
                Some("own_ship_boarded")
            } else {
                None
            };
            if let Some(line) = line {
                say(&mut commands, assets.as_deref(), line, rng.f());
            }
        }
        f.since += dt;
        if f.since < f.interval {
            continue;
        }
        f.since = 0.0;
        if casualty(&mut bi.3.strength, &mut ti.3.strength, rng.f() >= 0.5) {
            let outcome = settle(&mut bi.3.strength, &mut ti.3.strength);
            info!("boarding: {b} against {t}: {outcome:?} (attack {} / defend {} left)", bi.3.strength.attack, ti.3.strength.defend);
            end_fight(&mut commands, b, &f.fx);
            settled.write(Settled { boarder: b, target: t, outcome });
        }
    }
}

/// The consequences of a finished fight: a won fight captures the target (new owner, stopped, crewed afresh by the captor's skill, a base
/// back at full strength; the grapple is released), the rules get a capture, and the player's crew speaks.
pub(super) fn settled(
    mut msgs: MessageReader<Settled>,
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut captured: MessageWriter<Captured>,
    mut things: Query<(Option<&mut Ship>, Option<&mut MissionShip>, Option<&mut MissionBase>, &mut Boardable, Option<&mut crate::combat::BaseSide>)>,
    members: Query<&crate::ship::FleetMember>,
    grapples: Query<(Entity, &Grapple)>,
    mut mission: Option<ResMut<Mission>>,
) {
    for s in msgs.read() {
        let Ok([(bship, bms, _, bflag, _), (tship, tms, tbase, mut tflag, tside)]) = things.get_many_mut([s.boarder, s.target]) else { continue };
        let side_of = |ship: &Option<Mut<Ship>>| ship.as_ref().map(|x| x.side);
        let (ours, theirs, is_ship) =
            (side_of(&bship) == Some(Side::Player), side_of(&tship) == Some(Side::Player) || (tship.is_none() && side_of(&bship) == Some(Side::Enemy)), tship.is_some());
        let r = rng.f();
        match s.outcome {
            Outcome::Won => {
                let owner = |ms: &Option<Mut<MissionShip>>, mb: &Option<Mut<MissionBase>>, ship: &Option<Mut<Ship>>| {
                    ms.as_ref()
                        .map(|m| m.owner.clone())
                        .or_else(|| mb.as_ref().map(|m| m.owner.clone()))
                        .unwrap_or_else(|| if side_of(ship) == Some(Side::Player) { "Player".into() } else { "Enemy".into() })
                };
                let (by, victim) = (owner(&bms, &None, &bship), owner(&tms, &tbase, &tship));
                info!("boarding: {} captured {}", by, victim);
                captured.write(Captured { by: by.clone(), victim, unit: s.target });
                if let Some(mut m) = tms {
                    m.owner = by.clone();
                    m.target = None;
                    m.attack.clear();
                }
                if let Some(mut b) = tbase {
                    b.owner = by;
                }
                // A skirmish base changes sides with its guns and its place in the end rules.
                if let (Some(mut bs), Some(side)) = (tside, side_of(&bship)) {
                    bs.0 = side;
                }
                if let (Ok(member), true) = (members.get(s.boarder), is_ship) {
                    let mut member = member.clone();
                    member.1 = s.target.to_bits() as usize;
                    commands.entity(s.target).insert((member, crate::ship::FleetPilot::default()));
                }
                if let Some(mut sh) = tship {
                    sh.halt();
                    if let Some(side) = side_of(&bship) {
                        sh.side = side;
                    }
                }
                tflag.strength = if is_ship { Strength::ship(tflag.counts, bflag.skill.member()) } else { Strength::base(tflag.strength.full) };
                for (h, _) in grapples.iter().filter(|(_, g)| g.grappler == s.boarder || g.grappler == s.target) {
                    commands.entity(h).despawn();
                }
                // The captor's tow links go with the grapple.
                if let Some(m) = mission.as_mut() {
                    m.script.drop_tows_of(s.boarder);
                }
                if ours {
                    sfx.write(Sfx::ui("HUD_Options_Dock"));
                    say(&mut commands, assets.as_deref(), if is_ship { "enemy_ship_conquered" } else { "enemy_base_conquered" }, r);
                } else if theirs {
                    say(&mut commands, assets.as_deref(), if is_ship { "own_ship_conquered" } else { "allied_base_conquered" }, r);
                }
            }
            Outcome::Lost if ours => say(&mut commands, assets.as_deref(), "retreat", r),
            Outcome::Lost if theirs => say(&mut commands, assets.as_deref(), "boarding_repelled", r),
            _ => {}
        }
    }
}
