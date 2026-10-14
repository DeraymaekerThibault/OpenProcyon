# Boarding, grappling and capture (reverse engineering)

Source: observed behaviour of the original game and its retail data files, described in our own words.
 **[?]** marks inferences. Implementation: `game/src/boarding.rs`; data: `assets/data/boarding.json`
(`tools/export_boarding.py`). The menu side (items, enable tests) is in [docking-and-orders.md](docking-and-orders.md) §2.

## 1. What a boarding is
Boarding is a *relation* kept by the `BoardingActionsKeeper` (RTTI `TP::BoardingActionsKeeper`, owned by the player-relations object, ctor):
a list of 0x60-byte entries, one per fight, each naming a **boarder** (first side) and a **target** (second side). The keeper's per-frame update
 runs every fight; the fight itself is a plain stochastic attrition (§4), not a physical simulation. Nothing is boarded without a
**command** (§5): the exe has no AI path that starts a fight by itself.

Boardable-ness is two things: an object flag ( bit 3, true for every ship; bases only via their custom info) and the runtime
**"Can Be Boarded"** flag (`IBoardable` slot 1/2, ship; the TWT field `Boardable` of *Setup Ship*, the rule `Set Group/Unit Boardable`).
Only the *target's* flag matters for starting a fight.

## 2. Strength numbers
Both ships and bases implement `IBoardable` (ship custom info, island). Saved field names give the meaning:

| field (ship) | name in the save schema | value |
|---|---|---|
|  | Fighter Combat Strength | `round(10 · Σ combat strength of the Fighters)` |
|  | Crew Combat Strength | the same over every other crew member (gunners, captains, first mates, navigators, engineers, riggers, lookouts) |
|  | Cached **Attacking** Combat Strength (slot 0xC) | `crew / 2 + fighters` (a boarding party is the fighters and half the crew) |
|  | Cached **Defending** Combat Strength (slot 0x14) | `crew + fighters` (all hands) |
|  | Initial Defending Combat Strength (slot 0x28) | defending at the start |
|  | Can Be Boarded | flag |

A base (island custom info) has only `Initial Combat Strength` and `Defending Combat Strength` (slots 0x14/0x18); its attacking
value (slot 0xC) is constant 0. The *Setup Island* field `Combat Strength` seeds both (mission 12's palace: 100, mission 10's merchant island: 50).

Crew roles in the exe's order (WOT `Max number of ...`): Gunners, Captains, First Mates, Navigators, Engineers, Riggers, **Fighters** (role 6, the
fighter pool), Lookouts. The per-member value is the member's **Combat** skill (skill 5 of Leadership, Navigation, Spotting, Engineering, Rigging,
Combat, Gunnery; ability 0-10 scaled to 0-1, clamped to 1) plus a small bonus when the captains' leadership average beats two per-ship
thresholds (+0.1 / +0.05, thresholds from the ship's crew keeper [? not traced]). AI ships get the generic crews `Crew_General_<Race>_<Green|Average|Elite>`
by the *Setup Ship* `Crew Skill Level` (Combat 1 / 3 / 4); `Players` uses the campaign roster. Example: Pirate Schooner (4,1,1,1,0,2,2,1), Green →
fighters 2, crew 10, attack 7, defend 12; Elite → fighters 8, crew 40, attack 28, defend 48.

**Normalise** (slot 0x1C), run on both sides when a fight ends: the casualties (`fighters + crew - defending`) are taken from the fighter pool first,
then from the crew, and attack/defend are recomputed. Strengths are **not** restored between fights (only a captured ship is crewed anew, §4).

## 3. Start tests (used by the menu, the command and both routines)
Let A = target, B = boarder. A fight may start when: the players are not allied; no entry exists for the pair; A and B carry the
boardable object flag; **A's Can Be Boarded flag is on**; B is not a base; and
- A is a **ship**: a **grapple** relation (type 1) exists between them;
- A is a **base**: the boarder is in range (§4).

The Board item is *Stop* (kind 9, enabled) when an entry exists, else *Start* (kind 8, enabled by this test). The Grapple item is *Stop* (kind 4) when a
grapple relation exists, else *Start* (kind 3), enabled when the boarder has a grapple gun with fewer live grapples than its limit.

Registering: for ship targets the grapple relation's "boarding" byte is set, then the entry is created: first side =
boarder (type 0, attacking value), second = target (type 1, defending value), a start time, and the **casualty interval**
`(9 + 1.5·r) / (2 · min(boarder attack, target defend))` seconds (r random 0..1; constants; 0 when the minimum is 0).

## 4. The fight (keeper update, step)
Per entry and frame:
1. Drop the entry when the players became **allied**, or the **range test** fails. Ship targets: hull surfaces (centre distance minus both
   bounding-sphere radii) **< 10** and the relative velocity (physics + drift) **< 4.5**. Base targets: the distance from the boarder's sphere to the
   nearest of the base's `BoardingEffectPoint` nodes **< 15** and the boarder's speed **< 2**.
2. First frame of a fight: create the fight effects on the *target* (one on each `BoardingEffectPoint` node of the ship / base; ships declare
   `BoardingNode0..n` in the WOT mesh attributes, effect `Effects/Boarding_Fight.eft` [? name taken from the registered effect string], fade distance 600).
3. Once per interval (timer since the last step): a **coin flip** (LCG bit). Tails: the target loses one defending point and, if it has any, one
   attacking point; heads: the boarder loses one attacking point and one defending point. The fight ends when the *casualty's* side reaches zero
   (target: defending < 1; boarder: attacking < 1). Bases have no attacking value to lose.
4. When over, both sides are normalised (§2) and the result is read from the values **before** normalising: boarder attacking vs target defending.
   Target < boarder → **boarder wins** (outcome 0 to the boarder, 1 to the target); target > boarder → boarder loses (1 / 0); equal → draw (2 / 2).
   Then the `ShipCapturedMsg` is broadcast on a win, the grapple (relation type 1) between the two is removed, and the boarder's tow links are dropped.
5. Result handlers (slot 0x24 of `IBoardable`, `type` 0 = boarder side, 1 = target side):
   - **target, outcome 1 (captured)**, owners differ: ship → the AI task "stop" (0xB) is pushed, ownership moves to the boarder's player, and the ship
     is crewed afresh (crew list rebuilt, effects re-initialised); base → ownership moves and its defending strength is reset to the initial value (slot 8).
   - sounds (HUD speech ids passed to) depend on whether the *human's* primary is the boarder, the target or another own ship: ids
     0x32 / 0x33 / 0x36 (own boarding starts), 0x39 / 0x3a (the primary is boarded), 0x34 / 0x38 (own ship / base captured), 0x37 (boarding repelled),
     0x3b / 0x3c (we captured a ship / base), 0x3d / 0x3e (our boarding failed). The id → first-mate line table is **[? not traced]**; the remake picks the
     lines by meaning (`boarding_party`, `landing_party`, `repell_boarders`, `own_ship_boarded`, `enemy_ship_conquered`, `enemy_base_conquered`,
     `own_ship_conquered`, `allied_base_conquered`, `boarding_repelled`, `retreat`) from `content/voice/events.json`.

## 5. Commands and routines
`BoardShipCommand` (id 0x1B, execute) mode 0: set the ship's movement state to 4, then if the start test passes: ship target → register the
fight at once; base target → push **task 7** (`ShipBoardRoutine`, update), which re-tests, registers and ends (or paths toward the target while the
test fails). Mode 1 removes the fight. `GrappleShipCommand` (id 0x1C) pushes **task 5** (`ShipGrappleAndBoardRoutine`): while the
boarder has a free grapple gun it closes to the gun's range and fires (a 5 % chance per tick to actually fire once aimed [?]), cleans up grapple relations
with ships that stopped being hostile, and once a grapple holds and the start test passes (and a strength check [?] and a 20 % roll) it registers the fight.
The rule actions `Board Ships` / `Grapple Ship` exist but **no mission uses them**; the only callers of the start code are the HUD commands and these routines.
Grapples are weapon hits: the harpoon guns (`Gun_Harpoon_Light`, bullet `Bullet_Harpoon`/GrapplingHarpoon, sound `Weapon_GrapHarpoon_*`) of the Navy
cutter, frigate, man-o-war, sloop, tug and most pirate hulls; the keeper (`GrappledObjectsKeeper`, "Grapple Type": front / right side) holds the link.

## 6. HUD
`ShipStatus_Icon_Boarding_Start / Frame0..5 / Stop` animate in the primary ship's status slot like the docking icon. A world "boarding info" panel
(`BOARDINGSTATUS_HEALTHBAR` 171×40, two colour discs at x = 0 and 131, `ATTACKER` sword and `DEFENDER` shield icons, a relative strength bar inside
(40,13)-(130,29)) is shown while the camera is within 250 units (`Max Boarding Info Visible World Distance`).
The main bar's *Primary Ship Combat Strength* gauge (`MainBar_ShipStatus_CombatStrength`, tooltip text) is taken to be the ship's defending strength over its
initial value **[? inferred from the exe's use of the name "Combat Strength"; the gauge code was not traced]**; the remake used the ready-gun share before.

## Mission usage
m2: lifeboat pickup counts as `PlayerStart` capturing `Pirates` (implemented in `lifeboats.rs`, a separate table from boarding captures; see lifeboats.md); m6: *Capture Pirate Ship* (`Team has captured
a ship from PirateRunSloop,The Little Thief`, `PvP capture count PlayerStart vs PirateRunner` (the lifeboat route of the same objective; boarding feeds only `Team has captured`)); m9: *Board the Derelict StarHammer*; m10: *Board and Capture
the Disabled Ironclad*; m11: 12 `Set Group/Unit Boardable`; m12: the Procyon ironclads sail to the palace's landing boundary, `Parliament` is set Boardable
and `Procs capturing RoyalPalace` (Setup Island, Combat Strength 100) is the defeat. How the AI ironclads start that fight is **[? not found in the exe]**.

## Implementation status (`boarding.rs`)
Done: strengths from the exported crew counts and the skill's generic combat ability; casualty/interval/normalise/outcome exactly as above; start tests and
the menu slots (Board / Grapple start and stop); range test; capture (new owner or skirmish side, stop, crewed afresh, base reset), `Captured` → rules'
`Player Vs Player capture count` / `Team has captured a ship from Group/Unit` / `Set Group/Unit Boardable`; Setup Ship `Boardable` + `Crew Skill Level`, Setup
Island `Combat Strength`; fight effect on the target's boarding nodes, fight sound, first-mate lines, the status bar and the status icon.
PROVISIONAL: no leadership bonus and no per-man crews (generic crew values); `Players` = Average; the harpoon is not a projectile (the order closes to
100 units, stops and the hook bites after distance / 45 s) and harpoon guns no longer fire at will; one grapple per ship; the winch (drags grappled
hulls together at ≤ 5 u/s and evens out their velocities); a Board order on a grappled ship waits for the hulls to close instead of failing at once;
bases are boarded only by AI ships (a hostile non-Dummy mission ship in reach of a base whose rules made it Boardable), not from the menu; status bar
placement and colours; effect placement (the exe's extra main effect is not created); no grapple-break on tension.
