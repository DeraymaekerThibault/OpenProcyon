//! AI boarding of boardable bases.
use super::*;

/// Chance per tick (0.033 s) that a computer captain fires its harpoons at its target, and that a grappled one sends its
/// boarders; boarding also needs an attacking strength above `AI_MIN_ATTACK` (docs/re/boarding.md §5).
const AI_HARPOON_CHANCE: f32 = 0.05;
pub(super) const AI_BOARD_CHANCE: f32 = 0.2;
pub(super) const AI_MIN_ATTACK: i32 = 10;
/// The original's grapple-and-board routine, run by every AI-steered ship (not the player's command ship) against its current
/// target: with a free harpoon whose arc covers the target in harpoon range it fires at a 5 % chance per tick; once the grapple holds (or, at a
/// base, a boarding point is in reach) it boards at 20 % per tick if its boarding party is strong enough. It skips the harpoon (no
/// roll) while a friendly ship or scenery is on the line to the target (line_of_fire.rs). PROVISIONAL: only a ship its target has fired
/// at grapples (a retail Barque left an idle target alone and grappled ~17 s after being attacked).
#[allow(clippy::type_complexity)]
pub(super) fn ai_boarding(
    time: Res<Time>,
    lib: Res<Library>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
    mut sfx: MessageWriter<Sfx>,
    boarders: Boarders,
    mut msg: MessageWriter<BoardMsg>,
    ships: Query<(Entity, &Ship, &Transform, &Health, Option<&crate::combat::GunStates>, &Boardable, Option<&MissionShip>, Option<&crate::ship::FleetPilot>), Without<PlayerShip>>,
    bases: Query<Entity, With<MissionBase>>,
    targets: Query<(&Transform, &Ship)>,
    all_ships: Query<(Entity, &Transform, &Ship)>,
    scenery: Query<(Entity, &crate::collision::Obstacle, Option<&Visibility>, Has<crate::combat::Asteroid>), (Without<Ship>, Without<crate::combat::GunPart>)>,
    (mut fired, mut attacked, captains): (MessageReader<crate::combat::Fired>, Local<std::collections::HashSet<(Entity, Entity)>>, Option<Res<crate::ship::CaptainTargets>>),
) {
    attacked.extend(fired.read().filter_map(|f| f.target.map(|t| (f.shooter, t))));
    let dt = time.delta_secs();
    let me = boarders.me();
    for (e, ship, tf, health, guns, own, ms, pilot) in &ships {
        if ms.is_some_and(|m| m.stance == crate::rules::Stance::Dummy || m.hold_fire) || pilot.is_some_and(|p| p.hold_fire) {
            continue;
        }
        // Mission ships: the rules' target. Others: an explicit fleet order, else their captain's target (ship::CaptainTargets),
        // else (skirmish enemies) the player's ship.
        let target = match ms {
            Some(m) => m.target,
            None => pilot.and_then(|p| p.target).or_else(|| captains.as_deref().and_then(|c| c.of(e))).or_else(|| me.filter(|_| ship.side != Side::Player)),
        };
        let strong = own.strength.attack > AI_MIN_ATTACK;
        if let Some(t) = target.filter(|t| boarders.hostile(e, *t))
            && let Ok((ttf, tship)) = targets.get(t)
        {
            let surface = tf.translation.distance(ttf.translation) - lib.classes[ship.class].radius - lib.classes[tship.class].radius;
            if attacked.contains(&(t, e))
                && boarders.can_grapple(e)
                && !boarders.grappling(e, t)
                && harpoon_bears(&lib, ship, health, guns, tf, ttf.translation)
                && !boarders.line_blocked(e, ttf.translation, t, &all_ships, &scenery)
                && rng.f() < per_tick(AI_HARPOON_CHANCE, dt)
            {
                commands.spawn(Grapple::new(e, t, (surface.max(0.0) / HARPOON_SPEED).max(0.1)));
                sfx.write(Sfx { name: "Weapon_GrapHarpoon_Fire".into(), at: Some(tf.translation), vol: 0.7, max: 600.0 });
            }
            if strong && boarders.can_board(e, t) && rng.f() < per_tick(AI_BOARD_CHANCE, dt) {
                msg.write(BoardMsg::Board { boarder: Some(e), target: t });
            }
        }
        // PROVISIONAL: mission ships also board a boardable hostile base in reach without targeting it.
        if ms.is_some()
            && strong
            && let Some(t) = bases.iter().find(|t| boarders.can_board(e, *t))
            && rng.f() < per_tick(AI_BOARD_CHANCE, dt)
        {
            msg.write(BoardMsg::Board { boarder: Some(e), target: t });
        }
    }
}
