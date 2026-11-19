//! Harpoon range and bearing, the grapple flight and the winch that reels hulls together.
use super::*;

/// Boarder and target must be this close (hull surface to hull surface) and this slow relative to each other.
pub(super) const SHIP_RANGE: f32 = 10.0;
const SHIP_SPEED: f32 = 4.5;
/// Boarder to a base's boarding point, and the boarder's own speed.
const BASE_RANGE: f32 = 15.0;
const BASE_SPEED: f32 = 2.0;
/// The harpoon's longest reach and speed (Gun_Harpoon_Light MaximumRange and MuzzleSpeed).
const HARPOON_RANGE: f32 = 100.0;
pub(super) const HARPOON_SPEED: f32 = 45.0;
/// A grapple snaps when the ships' centres get this much further apart than when the hook bit.
const GRAPPLE_SLACK: f32 = 2.0;
/// While a boarding is registered on the grapple the line is reeled in at this speed, down to this centre distance.
/// Line shortened per tick (0.033 s) while reeling in.
const REEL_PER_TICK: f32 = 0.1;
pub(super) const REEL_MIN: f32 = 12.0;

/// The lines between ships, for views to draw (the original's Rope effect): world end points and how much longer than the gap the
/// line is (its slack).
#[derive(Resource, Default)]
pub struct Ropes(pub Vec<(Vec3, Vec3, f32)>);

/// Bitten grapples run from the grappler's harpoon mount to the target's nearest boarding point (the hull centre without one), slack
/// by how much the centres closed since the hook bit; tows run taut from the tower's stern tow point to the towee's bow.
/// PROVISIONAL: the original's attachment points on the target are not decoded.
pub(super) fn ropes(
    mut out: ResMut<Ropes>,
    lib: Res<Library>,
    mission: Option<Res<Mission>>,
    hooks: Query<&Grapple>,
    ships: Query<(&Ship, &Transform, &Health, Option<&crate::combat::GunStates>, Option<&Boardable>)>,
) {
    out.0.clear();
    for g in &hooks {
        let (Ok((ship, tf, health, guns, _)), Ok((_, ttf, _, _, tb))) = (ships.get(g.grappler), ships.get(g.target)) else { continue };
        if g.flight > 0.0 {
            continue;
        }
        let from = harpoons(&lib, ship, health, guns).next().map_or(tf.translation, |(m, _)| tf.transform_point(m.pos));
        let to = tb.into_iter().flat_map(|b| &b.points).map(|p| ttf.transform_point(*p)).nearest_to(from, |a| *a).unwrap_or(ttf.translation);
        out.0.push((from, to, (g.length - tf.translation.distance(ttf.translation)).max(0.0)));
    }
    for &(tower, towee) in mission.as_deref().map_or(&[][..], |m| m.script.tows()) {
        if let (Ok((a, ta, ..)), Ok((b, tb, ..))) = (ships.get(tower), ships.get(towee)) {
            out.0.push((ta.transform_point(lib.classes[a.class].tow_points[0]), tb.transform_point(lib.classes[b.class].tow_points[1]), 0.0));
        }
    }
}

pub(super) type Body<'a> = (&'a Transform, Option<&'a Ship>, Option<&'a Drift>);
pub(super) fn velocity(b: &Body) -> Vec3 {
    let (tf, ship, drift) = *b;
    ship.map_or(Vec3::ZERO, |s| tf.rotation * Vec3::NEG_Z * s.speed) + drift.map_or(Vec3::ZERO, |d| d.0)
}
pub(super) fn radius(lib: &Library, ship: Option<&Ship>) -> f32 {
    ship.map_or(0.0, |s| lib.classes[s.class].radius)
}
/// Ship targets need the hulls' surfaces close and nearly co-moving; a base needs a boarding point near the boarder's
/// hull and the boarder nearly stopped.
pub(super) fn in_range(lib: &Library, boarder: &Body, target: &Body, target_points: &[Vec3]) -> bool {
    let (rb, vb) = (radius(lib, boarder.1), velocity(boarder));
    if target.1.is_some() {
        let surface = boarder.0.translation.distance(target.0.translation) - rb - radius(lib, target.1);
        surface < SHIP_RANGE && (vb - velocity(target)).length() < SHIP_SPEED
    } else {
        target_points.iter().any(|p| boarder.0.translation.distance(target.0.transform_point(*p)) - rb < BASE_RANGE) && vb.length() < BASE_SPEED
    }
}
/// The ship's live harpoon mounts (a gun whose section still stands) with the gun fitted there.
fn harpoons<'a>(lib: &'a Library, ship: &Ship, health: &'a Health, guns: Option<&'a crate::combat::GunStates>) -> impl Iterator<Item = (&'a crate::data::Mount, &'a str)> {
    lib.classes[ship.class]
        .mounts
        .iter()
        .enumerate()
        .map(move |(i, m)| (m, guns.map_or(m.gun.as_str(), |g| g.gun(i))))
        .filter(|(m, gun)| gun.contains("Harpoon") && m.parent.is_none_or(|p| health.sections[p] > 0.0))
}
/// How many grapples the ship may hold: every gun bank whose first working gun (the first of its mounts whose section still stands)
/// fires the grappling harpoon adds all its guns, working or not.
pub(super) fn grapple_limit(lib: &Library, ship: &Ship, health: &Health, guns: Option<&crate::combat::GunStates>) -> usize {
    let mounts = &lib.classes[ship.class].mounts;
    bank_limit(mounts.iter().enumerate().map(|(i, m)| (m.bank, m.parent.is_none_or(|p| health.sections[p] > 0.0), guns.map_or(m.gun.as_str(), |g| g.gun(i)).contains("Harpoon"))))
}
/// `grapple_limit` over (bank, working, fires the harpoon) per gun, in mount order.
pub(super) fn bank_limit(guns: impl Iterator<Item = (usize, bool, bool)>) -> usize {
    let guns: Vec<_> = guns.collect();
    let mut banks: Vec<usize> = guns.iter().map(|g| g.0).collect();
    banks.sort_unstable();
    banks.dedup();
    banks.into_iter().map(|b| guns.iter().filter(|g| g.0 == b).collect::<Vec<_>>()).filter(|bank| bank.iter().find(|g| g.1).is_some_and(|g| g.2)).map(|bank| bank.len()).sum()
}
pub(super) fn has_harpoon(lib: &Library, ship: &Ship, health: &Health, guns: Option<&crate::combat::GunStates>) -> bool {
    harpoons(lib, ship, health, guns).next().is_some()
}
/// A live harpoon mount whose horizontal arc covers `at` (world point) within the gun's LongRange from the ship's centre.
/// PROVISIONAL range rule, chosen to match the retail runs (docs/re/boarding.md §5).
pub(super) fn harpoon_bears(lib: &Library, ship: &Ship, health: &Health, guns: Option<&crate::combat::GunStates>, tf: &Transform, at: Vec3) -> bool {
    harpoons(lib, ship, health, guns).any(|(m, gun)| {
        let local = (tf.rotation * m.rot).inverse() * (at - tf.transform_point(m.pos));
        (-local.x).atan2(-local.z).abs() <= m.arc / 2.0 && tf.translation.distance(at) <= lib.guns[gun].range[1]
    })
}

pub(super) fn cancel_grapple_order(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands, player: Query<Entity, With<PlayerShip>>) {
    if keys.just_pressed(KeyCode::Backspace) {
        for e in &player {
            commands.entity(e).remove::<GrappleOrder>();
        }
    }
}

/// The Grapple order: close to harpoon range, stop, fire; the hook bites after its flight.
pub(super) fn grapple_orders(
    mut commands: Commands,
    lib: Res<Library>,
    mut sfx: MessageWriter<Sfx>,
    mut orders: Query<(Entity, &GrappleOrder, &mut Ship, &Transform, &Health)>,
    others: Query<(&Transform, &Ship, &Health), Without<GrappleOrder>>,
) {
    for (e, order, mut ship, tf, health) in &mut orders {
        let Ok((ttf, tship, thealth)) = others.get(order.0) else {
            commands.entity(e).remove::<GrappleOrder>();
            continue;
        };
        if health.destroyed || thealth.destroyed {
            commands.entity(e).remove::<GrappleOrder>();
            continue;
        }
        let surface = tf.translation.distance(ttf.translation) - lib.classes[ship.class].radius - lib.classes[tship.class].radius;
        if surface <= HARPOON_RANGE {
            ship.halt();
            commands.entity(e).remove::<GrappleOrder>();
            commands.spawn(Grapple::new(e, order.0, (surface / HARPOON_SPEED).max(0.1)));
            sfx.write(Sfx { name: "Weapon_GrapHarpoon_Fire".into(), at: Some(tf.translation), vol: 0.7, max: 600.0 });
        } else {
            ship.waypoints = vec![ttf.translation];
            ship.order = 2;
        }
    }
}

/// A fired hook's flight runs out in the mission tick; when it ends the hook bites.
pub(super) fn hook_flight(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<AssetServer>>,
    mut rng: ResMut<Rng>,
    mut sfx: MessageWriter<Sfx>,
    mut hooks: Query<&mut Grapple>,
    ships: Query<&Transform>,
    player: Query<Entity, With<PlayerShip>>,
) {
    let dt = time.delta_secs();
    for mut g in &mut hooks {
        if g.flight <= 0.0 {
            continue;
        }
        let e = g.grappler;
        let Ok(tf) = ships.get(e) else {
            g.flight = 0.0;
            continue;
        };
        g.flight -= dt;
        if g.flight <= 0.0 {
            sfx.write(Sfx { name: "Weapon_GrapHarpoon_Impact_Wood".into(), at: Some(tf.translation), vol: 1.0, max: 600.0 });
            if Some(g.target) == player.single().ok() {
                say(&mut commands, assets.as_deref(), "grappled", rng.f());
            }
            info!("grapple: {e} holds {}", g.target);
        }
    }
}

/// The line length after reeling in for `dt` seconds: 0.1 less per tick of 0.033 s, not below `REEL_MIN`.
pub(super) fn reeled(distance: f32, dt: f32) -> f32 {
    (distance - REEL_PER_TICK * dt / crate::TICK_SECS).max(REEL_MIN)
}

/// A bitten grapple is a rope of the length the ships were apart when the hook bit (read from the original's code and checked in play).
/// Each tick, if the centres are further apart than that, both hulls are moved toward each other along the line by the excess, split
/// by mass (the lighter one moves more), positions only. The line snaps when the centres were already `GRAPPLE_SLACK` further apart
/// than at the hit before that pull, which takes a relative speed of about 60 units/s. While a boarding is registered on the pair the
/// length is set to the centre distance less `REEL_PER_TICK`, down to `REEL_MIN`. A grapple on a destroyed or vanished ship is let go.
/// It also snaps when either ship's partner crosses to the other side of the hull it was grappled on.
/// It is also let go when the two players become allied (here: no longer hostile, as the boarding fight's own test reads it).
/// The break test and the reel-in are the mission tick (`winch_upkeep`); the pull itself is the link keeper's (`winch_pull`).
pub(super) fn winch_upkeep(
    mut commands: Commands,
    time: Res<Time>,
    mut sfx: MessageWriter<Sfx>,
    mut hooks: Query<(Entity, &mut Grapple)>,
    fighters: Query<(Entity, &Boarding)>,
    ships: Query<(&Transform, &Health), With<Ship>>,
    mission: Option<Res<Mission>>,
    sides: Query<(&Ship, Option<&MissionShip>)>,
) {
    let dt = time.delta_secs();
    for (hook, mut g) in &mut hooks {
        let e = g.grappler;
        if g.flight > 0.0 {
            continue;
        }
        let Ok([(atf, ha), (btf, hb)]) = ships.get_many([e, g.target]) else {
            commands.entity(hook).despawn();
            continue;
        };
        if ha.destroyed || hb.destroyed {
            commands.entity(hook).despawn();
            continue;
        }
        let who = |x: Entity| sides.get(x).ok().map(|(sh, ms)| (ms.map(|m| m.owner.as_str()), Some(sh.side)));
        if let (Some(a), Some(b)) = (who(e), who(g.target))
            && !hostile_pair(mission.as_deref(), a, b)
        {
            info!("grapple: {e} lets go of {} (the players became allied)", g.target);
            commands.entity(hook).despawn();
            continue;
        }
        let axis = (btf.translation - atf.translation).with_y(0.0);
        let distance = axis.length();
        if g.span <= 0.0 {
            g.span = distance;
            g.length = distance;
            g.sides = [side_of(atf, btf.translation), side_of(btf, atf.translation)];
        }
        // PROVISIONAL: the axis is the nearer of bow-stern and beam; the original picks it from the hull's proportions.
        let crossed = |tf: &Transform, other: Vec3, side: Vec3| (tf.rotation * side).dot(other - tf.translation) <= 0.0;
        if distance > g.span + GRAPPLE_SLACK || crossed(atf, btf.translation, g.sides[0]) || crossed(btf, atf.translation, g.sides[1]) {
            info!("grapple: {e} loses {} (centres {distance:.1} apart, bit at {:.1})", g.target, g.span);
            sfx.write(Sfx { name: "rope_break_placeholder".into(), at: Some(atf.translation.lerp(btf.translation, 0.5)), vol: 1.0, max: 600.0 });
            commands.entity(hook).despawn();
            continue;
        }
        let boarding = fighters.iter().any(|(f, fight)| (f == e && fight.target == g.target) || (f == g.target && fight.target == e));
        if boarding && distance > REEL_MIN {
            g.length = reeled(distance, dt);
        }
    }
}

/// The link keeper's pull (step 7.1): a bitten grapple's hulls are moved toward each other by the excess over the rope's length, split by mass.
pub(super) fn winch_pull(lib: Res<Library>, hooks: Query<&Grapple>, mut ships: Query<(&Ship, &mut Transform)>) {
    for g in &hooks {
        if g.flight > 0.0 || g.span <= 0.0 {
            continue;
        }
        let Ok([(a, mut atf), (b, mut btf)]) = ships.get_many_mut([g.grappler, g.target]) else { continue };
        let axis = (btf.translation - atf.translation).with_y(0.0);
        let distance = axis.length();
        if distance > g.length && distance > 1e-3 {
            let (ma, mb) = (lib.classes[a.class].phys.mass, lib.classes[b.class].phys.mass);
            let pull = axis / distance * (distance - g.length);
            atf.translation += pull * (mb / (ma + mb));
            btf.translation -= pull * (ma / (ma + mb));
        }
    }
}
