//! Not retail: the boarding order the mission rule `OP Board Ship` gives a ship (`rules/eval/ext.rs`, docs/modding.md "Mission rule
//! extensions"). It is the sequence a player's orders make by hand, not a second implementation of boarding: the ship takes the Grapple
//! order (steer into harpoon range, fire, the hook bites), and once `can_board` holds the same `BoardMsg::Board` the menu sends starts
//! the fight. Everything else (the winch, the fight, the capture, `Captured`) is the shared code.
use super::*;

/// A pending `OP Board Ship`: the ship is waiting for its grapple on `0` to hold, and then boards it.
#[derive(Component)]
pub(super) struct BoardOrder(Entity);

/// The order arrives: a ship that holds (or has in flight) a grapple on the target waits for the boarding to be possible; one that
/// has a harpoon gets the Grapple order first. No harpoon (or a target it is not hostile to, or not a live ship): nothing happens and
/// the log says so, as the menu would not offer the order either.
pub(super) fn begin(commands: &mut Commands, boarders: &Boarders, boarder: Entity, target: Entity) {
    if boarder == target || !boarders.live_ship(boarder) || !boarders.live_ship(target) || boarders.boarding(boarder, target) {
        return;
    }
    if boarders.grappled(boarder, target) || boarders.grappling(boarder, target) {
        commands.entity(boarder).insert(BoardOrder(target));
    } else if boarders.can_grapple(boarder) && boarders.hostile(boarder, target) {
        commands.entity(boarder).insert((GrappleOrder(target), BoardOrder(target)));
    } else {
        info!("OP Board Ship: {boarder} cannot board {target} (it needs a live harpoon, and the two must be hostile)");
    }
}

/// Sends the boarding once the hook holds. An order ends when either ship is gone, when the fight has started, or when the grapple it
/// waited for was lost (cut, out of range): nothing keeps trying behind the mission author's back. A target that is not Boardable
/// (`Set Group/Unit Boardable`) keeps the order waiting while the grapple holds.
pub(super) fn board_orders(
    mut commands: Commands,
    boarders: Boarders,
    orders: Query<(Entity, &BoardOrder)>,
    grapple_orders: Query<&GrappleOrder>,
    mut msg: MessageWriter<BoardMsg>,
) {
    for (e, BoardOrder(target)) in &orders {
        let target = *target;
        let waiting = grapple_orders.get(e).is_ok_and(|g| g.0 == target) || boarders.grappling(e, target);
        if !boarders.live_ship(e) || !boarders.live_ship(target) || boarders.boarding(e, target) {
            commands.entity(e).remove::<BoardOrder>();
        } else if boarders.can_board(e, target) {
            msg.write(BoardMsg::Board { boarder: Some(e), target });
            commands.entity(e).remove::<BoardOrder>();
        } else if !waiting && !boarders.grappled(e, target) {
            commands.entity(e).remove::<BoardOrder>();
        }
    }
}
