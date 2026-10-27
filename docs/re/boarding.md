# Boarding, grappling and capture (reverse engineering)

Source: observed behaviour of the original game and its retail data files, described in our own words.
 **[?]** marks inferences. Implementation: `game/src/boarding.rs`; data: `assets/data/boarding.json`
(`tools/export_boarding.py`). The menu side (items, enable tests) is in [docking-and-orders.md](docking-and-orders.md) §2.

## 1. What a boarding is
Boarding is a *relation* between players: a list with one entry per fight, each naming a **boarder** (first side) and a **target**
(second side). Every fight is updated each frame; the fight itself is a plain stochastic attrition (§4), not a physical simulation. Nothing is boarded without a
**command** (§5): the original has no AI path that starts a fight by itself.

Boardable-ness is two things: whether the object can take part in boarding at all (every ship; bases only when their setup gives
them a combat strength) and the runtime **"Can Be Boarded"** flag (the TWT field `Boardable` of *Setup Ship*, the rule
`Set Group/Unit Boardable`).
Only the *target's* flag matters for starting a fight.

## 2. Strength numbers
Ships and bases both have boarding strengths. Saved field names give the meaning:

| field (ship) | name in the save schema | value |
|---|---|---|
|  | Fighter Combat Strength | `round(10 · Σ combat strength of the Fighters)` |
|  | Crew Combat Strength | the same over every other crew member (gunners, captains, first mates, navigators, engineers, riggers, lookouts) |
|  | Cached **Attacking** Combat Strength | `crew / 2 + fighters` (a boarding party is the fighters and half the crew) |
|  | Cached **Defending** Combat Strength | `crew + fighters` (all hands) |
|  | Initial Defending Combat Strength | defending at the start |
|  | Can Be Boarded | flag |

A base has only `Initial Combat Strength` and `Defending Combat Strength`; its attacking value is always 0. The *Setup Island* field `Combat Strength` seeds both (mission 12's palace: 100, mission 10's merchant island: 50).

Crew roles in the original's order (WOT `Max number of ...`): Gunners, Captains, First Mates, Navigators, Engineers, Riggers, **Fighters** (role 6, the
fighter pool), Lookouts. The per-member value is the member's **Combat** skill (skill 5 of Leadership, Navigation, Spotting, Engineering, Rigging,
Combat, Gunnery; ability 0-10 scaled to 0-1, clamped to 1) plus a small bonus when the captains' leadership average beats two per-ship
thresholds (+0.1 / +0.05; where the thresholds come from is unknown [?]). AI ships get the generic crews `Crew_General_<Race>_<Green|Average|Elite>`
by the *Setup Ship* `Crew Skill Level` (Combat 1 / 3 / 4); `Players` uses the campaign roster. Example: Pirate Schooner (4,1,1,1,0,2,2,1), Green →
fighters 2, crew 10, attack 7, defend 12; Elite → fighters 8, crew 40, attack 28, defend 48.

**Normalise**, run on both sides when a fight ends: the casualties (`fighters + crew - defending`) are taken from the fighter pool first,
then from the crew, and attack/defend are recomputed. Strengths are **not** restored between fights (only a captured ship is crewed anew, §4).

## 3. Start tests (used by the menu, the command and both routines)
Let A = target, B = boarder. A fight may start when: the players are not allied; no entry exists for the pair; A and B can take part in boarding; **A's Can Be Boarded flag is on**; B is not a base; and
- A is a **ship**: a **grapple** holds them together;
- A is a **base**: the boarder is in range (§4).

The Board item is *Stop* (enabled) when a fight exists, else *Start* (enabled by this test). The Grapple item is *Stop* when a
grapple holds, else *Start*, enabled when the boarder has a grapple gun with fewer live grapples than its limit.

Registering: the grapple (ship targets) is marked as boarding, then the fight is created: first side = boarder (its attacking
value), second = target (its defending value), a start time, and the **casualty interval**
`(9 + 1.5·r) / (2 · min(boarder attack, target defend))` seconds (r random 0..1; constants; 0 when the minimum is 0).

## 4. The fight
Per entry and frame:
1. Drop the entry when the players became **allied**, or the **range test** fails. Ship targets: hull surfaces (centre distance minus both
   bounding-sphere radii) **< 10** and the relative velocity (physics + drift) **< 4.5**. Base targets: the distance from the boarder's sphere to the
   nearest of the base's `BoardingEffectPoint` nodes **< 15** and the boarder's speed **< 2**.
2. First frame of a fight: create the fight effects on the *target* (one on each `BoardingEffectPoint` node of the ship / base; ships declare
   `BoardingNode0..n` in the WOT mesh attributes, effect `Effects/Boarding_Fight.eft` [? not confirmed], fade distance 600).
3. Once per interval (timer since the last step): a **coin flip**. Tails: the target loses one defending point and, if it has any, one
   attacking point; heads: the boarder loses one attacking point and one defending point. The fight ends when the *casualty's* side reaches zero
   (target: defending < 1; boarder: attacking < 1). Bases have no attacking value to lose.
4. When over, both sides are normalised (§2) and the result is read from the values **before** normalising: boarder attacking vs target defending.
   Target < boarder → **boarder wins** (outcome 0 to the boarder, 1 to the target); target > boarder → boarder loses (1 / 0); equal → draw (2 / 2).
   Then a win is announced as a capture (rules count it), the grapple between the two is removed, and the boarder's tow links are dropped.
5. Results, per side:
   - **target captured**, owners differ: ship → it stops what it was doing, ownership moves to the boarder's player, and the ship
     is crewed afresh (crew list rebuilt, effects re-initialised); base → ownership moves and its defending strength is reset to the initial value.
   - first-mate speech depends on whether the *human's* primary is the boarder, the target or another own ship: own boarding starts,
     the primary is boarded, own ship / base captured, boarding repelled, we captured a ship / base, our boarding failed. Which line plays
     for each is **[? unknown]**; the remake picks the lines by meaning (`boarding_party`, `landing_party`, `repell_boarders`, `own_ship_boarded`, `enemy_ship_conquered`, `enemy_base_conquered`,
     `own_ship_conquered`, `allied_base_conquered`, `boarding_repelled`, `retreat`) from `content/voice/events.json`.

## 5. Orders and activities
Board start: the ship goes under explicit orders; if the start test passes and the target is a ship, the fight starts at once; for a base
target the ship starts a boarding activity that keeps re-testing (pathing toward the target while the test fails) and starts the fight
once it passes. Board stop removes the fight. Grapple starts a grapple-and-board activity: while the
boarder has a free grapple gun it closes to the gun's range and fires (a 5 % chance per tick to actually fire once aimed [?]), cleans up grapple relations
with ships that stopped being hostile, and once a grapple holds and the start test passes (and a strength check [?] and a 20 % roll) it registers the fight.
The rule actions `Board Ships` / `Grapple Ship` exist but **no mission uses them**; fights only start from the player's orders and these activities.

**Computer captains** run the grapple-and-board activity on their own: every ship the AI steers (all computer ships; not the
human's command ship) runs it every tick against its current target, without any order:
- If the target is a hostile ship, the ship has a harpoon gun with a free grapple, and no friendly ship or scenery lies in the
  line of fire, it fires its harpoons at a 5 % chance per tick (otherwise the harpoons hold fire). The harpoon is an ordinary
  gun, so it also has to be able to aim: the target must be inside a harpoon mount's arc and range.
- Once the boarding start test passes (§3 and §4 step 1: grapple holding and hulls within 10 / slower than 4.5 for a ship; a boarding point in
  reach for a base), it starts the fight at a 20 % chance per tick, but only if its **attacking strength is above 10**.
- Skirmish runs ([movement.md](movement.md#computer-captains-measured-2026-10)): a Carrack's harpoons face forward (±60°), so
  circling with its target on the beam it never fires; a Barque's sit on both beams (±30° around 90°). A Barque circling an idle
  Cutter at 55.7 units for 200 s never grappled; one circling our Man-O-War came within ~47 units with us on its beam twice
  without firing, and only started its grapple passes ~17 s after our first attack click on it (it was first hit 40 s later).
  PROVISIONAL, chosen to fit those runs: the harpoon's reach is its LongRange (45) from the ship's centre, and a computer ship
  only grapples a target that has fired at it. Neither is observed directly.
- Remake: `boarding::ai_boarding` (skirmish enemies their captain's target, `ship::CaptainTargets`; mission ships the rules' target, the
  player's other ships their pilot's target); the line-of-fire skip is not modelled.
**The grapple line.** A harpoon hit links the two ships; the link remembers the centre distance at that moment. The link itself
does not tow or slow either ship. Each frame it is let go (rope-break sound `rope_break_placeholder` at the midpoint) when:
- the ships' centres are more than **2** units further apart than when the hook bit;
- the two players become allied;
- the partner crosses to the other side of the hull: at the hit each end records whether the other ship lay mostly ahead/astern
  or abeam, and on which side, and the link breaks when that side changes.
Once a boarding fight is registered on the pair (the grapple's "boarding" mark, §3), the line is **reeled in**: each AI tick
(1/30 s) its length becomes the current centre distance minus 0.1 (**3 units/s**), while it is longer than **12**. A computer
captain's fight registered out of range is dropped by the range test but its mark keeps reeling **[?]**, which would explain how
grapple passes end with the hulls together (retail run: a Barque closed to 12-18 units on its passes before capturing us).
The line is drawn with the `Rope` effect: a 40-segment chain (each point mass 1, segment rest length = the gap at the hit / 40) whose
two end points are pinned to the ships every frame and that carries a constant force; texture `rope.tga`, segment width 0.1,
texture repeat every 2 units, alpha blended. Tow links use the same effect between the tow points.

Grapples are weapon hits: the harpoon guns (`Gun_Harpoon_Light`, bullet `Bullet_Harpoon`/GrapplingHarpoon, sound `Weapon_GrapHarpoon_*`) of the Navy
cutter, frigate, man-o-war, sloop, tug and most pirate hulls; the link ("Grapple Type": front / right side) holds the two ships together.

## 6. HUD
`ShipStatus_Icon_Boarding_Start / Frame0..5 / Stop` animate in the primary ship's status slot like the docking icon. A world "boarding info" panel
(`BOARDINGSTATUS_HEALTHBAR` 171×40, two colour discs at x = 0 and 131, `ATTACKER` sword and `DEFENDER` shield icons, a relative strength bar inside
(40,13)-(130,29)) is shown while the camera is within 250 units (`Max Boarding Info Visible World Distance`).
The main bar's *Primary Ship Combat Strength* gauge (`MainBar_ShipStatus_CombatStrength`, tooltip text) is taken to be the ship's defending strength over its
initial value **[? inferred from the name "Combat Strength"; unverified]**; the remake used the ready-gun share before.

## Mission usage
m2: lifeboat pickup counts as `PlayerStart` capturing `Pirates` (implemented in `lifeboats.rs`, a separate table from boarding captures; see lifeboats.md); m6: *Capture Pirate Ship* (`Team has captured
a ship from PirateRunSloop,The Little Thief`, `PvP capture count PlayerStart vs PirateRunner` (the lifeboat route of the same objective; boarding feeds only `Team has captured`)); m9: *Board the Derelict StarHammer*; m10: *Board and Capture
the Disabled Ironclad*; m11: 12 `Set Group/Unit Boardable`; m12: the Procyon ironclads sail to the palace's landing boundary, `Parliament` is set Boardable
and `Procs capturing RoyalPalace` (Setup Island, Combat Strength 100) is the defeat. How the AI ironclads start that fight is **[? not found in the original]**.

## Implementation status (`boarding.rs`)
Done: strengths from the exported crew counts and the skill's generic combat ability; casualty/interval/normalise/outcome exactly as above; start tests and
the menu slots (Board / Grapple start and stop); range test; capture (new owner or skirmish side, stop, crewed afresh, base reset), `Captured` → rules'
`Player Vs Player capture count` / `Team has captured a ship from Group/Unit` / `Set Group/Unit Boardable`; Setup Ship `Boardable` + `Crew Skill Level`, Setup
Island `Combat Strength`; fight effect on the target's boarding nodes, fight sound, first-mate lines, the status bar and the status icon.
PROVISIONAL: no leadership bonus and no per-man crews (generic crew values); `Players` = Average; the harpoon is not a projectile (the order closes to
100 units, stops and the hook bites after distance / 45 s) and harpoon guns no longer fire at will; one grapple per ship; the grapple line as
above (no tow, snaps 2 units past its bite distance, reels in at 3 u/s to 12 while a fight is registered; the reel acts on the ships'
velocities at a rate of our choosing, the allied break is not modelled and the side test takes the nearer of the bow-stern and beam axes); a Board order on a grappled ship waits for
the hulls to close instead of failing at once, and the fight effect, crew lines and HUD status start only when the crews meet; the rope is a
sagging ribbon, not a simulated chain (`classic/ropes.rs`);
computer ships grapple and board their target on their own (§5, `ai_boarding`; reach and attacked-first gate PROVISIONAL); bases are boarded only by AI ships (a hostile non-Dummy mission ship in reach of a base whose rules made it Boardable), not from the menu; status bar
placement and colours; effect placement (the original's extra main effect is not created); no grapple-break on tension.

## Skirmish bases (remake, 2026-10-27)
Border Dispute's end rules award the game for 'capturing a ship from' the other side's base (a boarding party taking the base), so skirmish
bases are boardable: a boarding point within 15 units and a speed under 2, like campaign bases (§4). On a won fight the base changes
sides: its guns fight for the captor and the end rules count it as captured (the side the map gave it is remembered). PROVISIONAL: the
retail skirmish base's boarding strength is not measured (the remake uses 50, campaign islands use 50-100); computer ships do not board
skirmish bases (no AI path in the original, §5); whether the capturing team matters to the rule is not checked.
