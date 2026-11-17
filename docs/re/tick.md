# The simulation tick: order and timing

Source: reading of the original's code, checked on the running game where marked; described in our own words. This page is the
order the battle simulation must follow. Individual rules (damage, movement, contact) live in their own pages and only assume this order.

## The clock

- One tick is **0.033 s** (a 32-bit float). The game clock is a 64-bit number that adds that float once per tick, so after n ticks it
  is exactly the sum of n copies of 0.033f in double precision (about 0.0329999998 per tick, not 0.033). *Measured:* 911 consecutive
  ticks of one battle, tick 80 to 686, every logged time equal to that sum with zero difference; no tick skipped.
- A tick counter advances with the clock. Nothing in the simulation uses wall time.
- Speed setting 1x, 2x or 5x means 1, 2 or 5 whole ticks per frame; the tick never changes size. Pause runs no ticks and the clock stands still.
  A single-step request while paused runs one frame (that many ticks).

## The frame

Real time feeds an accumulator: for each 0.033 s of real time one frame runs. At most **10 frames** run per call; if all 10 were
needed the backlog is dropped (the game then simply runs slower than real time). Order in a frame:

1. Queued player and system commands run once, before any tick of the frame.
2. The tick sequence below, repeated for the speed setting.
3. After the last tick of the frame: in single player, the **mission rules are evaluated once** (so at 2x and 5x they run a half
   or a fifth as often as ticks), skipped when the mission is frozen. This also happens while paused. In multiplayer the rules run
   inside the tick instead (step 4), every tick.

## One tick, in order

1. **Pre-tick:** cameras, the end-of-mission countdown (a mission end waits 10 game seconds before the next screen), ambient music.
   No effect on ships.
2. The tick counter and the clock advance. Effect systems update.
3. Commands whose tick has come run (queue order; this is the lock-step order in multiplayer).
4. **New objects join:** everything created since the last tick is added now, in creation order, and may move in this same tick.
   Objects created during steps 6 and 7 wait for the next tick.
5. **Objects queued for removal are destroyed now** (died, left the map, were replaced). Removal is therefore always one tick after
   it was decided; in between, the object still takes part in step 8.
6. **Mission tick** (grapple and boarding upkeep, per-player clocks; multiplayer rules). Skipped when the mission is frozen.
7. **Physics, in three parts:**
   1. Field effects push first, each in its registration order: black hole, ether current, nebula, mortar and gravity wells, timed shells, grappled objects.
   2. Every moving object integrates one step of 0.033 s, in the order objects were registered (creation order until the first removal;
      removal moves the last object into the freed place). A mover is re-placed in the scene and the spatial cells. If it ended
      outside the map box (x, y outside the map's limits, or z below -130) it is queued for removal (step 5 of the next tick).
   3. **One collision pass** over the positions after *all* objects have moved (`docs/re/collision.md`). Each overlapping pair
      responds at most once per tick, a response moves the objects at once and is not integrated again until the next tick.
8. **Per-object logic, in object order (creation order):** the object's decision part, then its own upkeep. For a ship the decision
   is the captain's think, which runs only when it is due (every 16 ticks, see below), then ship upkeep (fire, destruct sequence,
   effects, passive repair, refit timers). Guns think every tick. These run after movement and collision, and what they decide acts in the next tick's integration.
9. Sound.

### The think timer

Each AI object keeps the time of its last think. Every tick: while its start delay is still running the "last think" is held at the
current time; if the time since the last think is at least its period, it thinks and the last think becomes the current time (not
last think plus the period). Ships, islands, space objects, animals, mines, volcanoes and dragons have a period of 0.5 s;
guns have 0 and think every tick. Because 15 ticks make 0.495 s, **a 0.5 s period is 16 ticks (0.528 s)** in practice. Start delay =
0.05 s x (n mod 10) where n counts the objects of that class created so far in the battle (one counter per class for the whole world,
not per player). *Measured:* five captains, every think gap 0.528 s, 85 gaps each.

## What the remake does now (differences, to be built from here)

| Rule | Original | Remake |
|---|---|---|
| Game clock | double sum of 0.033f | fixed 33 ms steps |
| Removal | queued, destroyed at the start of the next tick, still runs per-object logic once | same tick, end of tick |
| New objects | join at the start of the next tick and move in it | appear when spawned |
| Out of map | tested after integration, queued; floor z = -130 | not tested the same way |
| Collision | one pass after all movement | ships integrate and resolve one by one |
| Decisions | after movement and collision | before movement |
| Mission rules (single player) | once per frame, after the last tick | every tick, before decisions |
| Frame catch-up | 10 frames, then drop | about 7 ticks at 1x, 3 above |
| Contact rate constant | 0.033 | 1/30 |

## Unmeasured

- Removal one tick late, the join-at-start rule and the out-of-map test are read from code only.
- Whether a decision made in step 8 differs observably from one made before the step: both see the previous tick's positions and act in the next integration, so the
  difference is only what else the decision sees (commands of the next tick; in single player at 1x, the rules). At most one tick.
- Weather drift and spinning scenery update inside step 6 and are cosmetic.
