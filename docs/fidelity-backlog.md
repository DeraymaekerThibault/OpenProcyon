# Original-game fidelity backlog

This is the working checklist for the **faithful single-player remake**.
It consolidates existing research and a source/retail-rule audit; it is **not a claim that every original feature has been discovered**.
Update this list when a feature lands or new evidence changes its status. Detailed specifications stay in `docs/re/`.

## Status and completion rule

- **Missing:** no usable end-to-end implementation (art, exported data or a rebindable key alone does not count).
- **Partial:** some of the feature works, but an important path is missing.
- **Provisional:** implemented with behaviour/constants not established from original evidence.
- **Unvalidated:** implemented, but the relevant gameplay/retail comparison has not been completed.
- **Unknown:** inventory or semantics still need investigation; do not assume absent or complete.

An item closes only with: original evidence → own-words behaviour specification → shared runtime implementation → regression test → runtime acceptance scenario.
For visual/interaction fidelity, add a reference comparison. A unit test proves our expected behaviour, not that the expectation matches retail.
Keep stable IDs when closing rows; record the check and evidence rather than deleting them. No overall parity percentage: the denominator is not known.

## What this check established

- Mission 1 has been completed manually end-to-end. It is not still an unfinished pirate-fight scaffold.
- Docking, repair scaffold visuals, boarding/capture, torpedo guidance, independent towing, talking heads, checkpoint saves and physical collisions exist.
  They must not be counted as entirely missing because an older milestone paragraph says so.
- Control Options and rebinding exist, but many advertised actions have no input consumer. Audio/Display Options are disabled.
- Mission-rule coverage is not semantic coverage. `rules/tests.rs::later_missions_only_warn_about_known_missing_features` allows known warnings;
  `rules/eval.rs::act` also silently accepts a list of no-op actions. `Player has no lifeboats` now follows the existing lifeboats (`lifeboats.rs`).
- Missions 2–13 load, but have not been validated in play. No-op rules and skipped objects can block them even with no unknown-rule warning.

(The "Control Options" and "Mission-rule coverage" bullets above describe the state before the AUD-05 batch below.)

## 1. Controls and fleet management — first implementation batch

| ID | Status | Gap / acceptance target | Runtime owner and evidence |
|---|---|---|---|
| UI-01 | Provisional | Shared selection / command / focus, own-fleet indexed/wrapped selection, independent captain orders and user/script promotion implemented. P/bracket conflicts resolved; pause/speed retained. Regression + both-view ECS smoke passed; lifecycle, cross-group transfer, key/default and widget parity remain provisional. | `bindings.rs`, `view.rs`, `ship.rs`, `combat/`, `scenario.rs`, `campaign.rs`, `panels.rs`, `rules/`; [fleet-controls.md §1, §4–5](re/fleet-controls.md) |
| UI-02 | Provisional | Functional secondary-only ten-slot roster, toggle/auto-hide, selected/hover/press feedback, aggregate health/status, left release selection and right options menu; lifecycle rebuild tested. Both views smoked at two heights. Native slide/overflow/status/art metrics still unvalidated; immediate transitions and status fallback are explicit. | `panels.rs`, `classic/panels.rs`, remaster HUD; [fleet-controls.md §2](re/fleet-controls.md) |
| UI-03 | Provisional | All eight types, named-fleet rule consumers, six buttons, per-secondary following/detachment, same-type release/partial rejoin and fleet attack/cease/resume implemented. Regression + both-view ECS smoke passed. Native size/assignment/follow-controller and long-run convergence calibration remain open. | `ship.rs`, `scenario.rs`, `rules/`, `panels.rs`; [fleet-controls.md §3, §5](re/fleet-controls.md), rule census below |
| UI-04 | Provisional | Movement-type button (`MainBar_BeginnerMovementTypeButton`) toggles basic/advanced. Advanced: clicks plot pending points (do-nothing stars + preview course), the check mark on the last point or Enter accepts, the X beside it or Backspace cancels. Button placement/keys are the remake's reading of the manual. | `course.rs` (`Plot`), `view.rs`, `panels.rs`; manual in [faithful-remake.md](faithful-remake.md) |
| UI-05 | Provisional | Hold position (`AI_LOCK_POSITION` / `NOT_LOCK`, `ship::HoldPosition`: captains stop moving, still fire) and Cloak start/stop for cloakable hulls (`cloak.rs`: 2 s fade, Cloaking/DeCloaking/FullyCloaked fx, IC_Sub sounds, enemies hidden, guns without 'Can Be Fired While Cloaked' hold, unseen beyond 40 units; the rules' cloak uses the same component with AI override). Timings provisional. | `options_menu.rs`, `cloak.rs`, `ship.rs`, `combat/mod.rs`; [cloaking.md](re/cloaking.md), [docking-and-orders.md](re/docking-and-orders.md) |
| UI-06 | Provisional | All bindings consumed: Scroll Lock screenshot (user `screenshots/`), B focus nearest projectile, Alt attack-only (clicks only target). | `view.rs` |
| UI-07 | Provisional | HUD gauge meanings, radar range/contact animation, faction placement, cursors, targeting/nav marker geometry and tooltips need a retail comparison/inventory. | `panels.rs`, `world_hud.rs`, `options_menu.rs`, `starmap.rs`, `classic/`; [starmap.md](re/starmap.md), [reference-video.md](reference-video.md) |

Do not start UI-01 by simply moving `PlayerShip`: that marker also drives camera, HUD, targeting, docking and mission behaviour.
Trace those callers and establish the original selection/command-ship distinction first.

### UI-01/02/03 investigation outcome

The manual, the retail data and the original game's input/command behaviour are documented in
[fleet-controls.md](re/fleet-controls.md), with controls, eligibility, state changes, feedback, edge cases,
the pre-change caller audit and an ordered implementation/acceptance checklist. The shared runtime batch is now implemented provisionally; [fleet-controls.md §6](re/fleet-controls.md#6-shared-implementation-and-validation-record) records the paths, tests, runtime checks and explicit fallbacks. No original program was run and the Steam installation was not changed. These rows are not retail-parity validated.

Key findings:
- The game keeps a **selected ship** and a separate **primary ship**. Selection alone never promotes a secondary;
  F/focus-only changes the camera. Primary portrait/key selects the primary, not merely camera focus.
- Indexed secondary input has ten registered slots; next/previous wrap across the complete own fleet, including primary.
  Eligibility is own-fleet membership, not allied Side. The old P pause / bracket speed conflicts are resolved: those keys now use fleet bindings; existing HUD/menu pause and speed controls remain (Pause key is a non-retail convenience).
- P sends `ChangePrimaryShipCommand` (`0x1f`) outside campaign; execution moves the candidate to roster index 0,
  changes manual/AI roles, rebuilds primary/secondary feedback and preserves a normalized current speed.
  Campaign `Set Fleet Primary Ship` bypasses the user-input restriction and invokes the same promotion helper.
- Ship bar subscribes to primary/secondary changes and rebuilds from secondaries only. Successful indexed/bar selection
  has its own sound; out-of-range keys do nothing. Bar press/release and right options-menu input are real handlers.
- Formation command (`0x19`) carries player/type/**hold boolean**; hold is per follower, never the primary.
  Clicking an already chosen type releases only when **all** followers hold; with one detached member it rejoins everyone.
  Yellow feedback likewise requires all-held. Individual move detaches that member; attack-in-formation is a separate routine.
- None and Custom are real types; Column spacing is 2R+30, other generated steps R+30. Native layouts store leader-relative
  distance/angle and follow continuously. They are not universal LineAbreast or fixed route offsets.

**Still unknown:** exact native default-key mapping discrepancies (primary/slot 10), modal-gate names, destruction/capture
selection fallback, cross-group script promotion, complete restricted-widget/status/overflow behaviour, native size-field
mapping, slot-assignment tie policy and follow-controller constants. These remain explicit probes; remake fallbacks are separately labelled in §6, not presented as recovered native rules.

Implementation order: own-fleet identity/input conflicts → separate selection/primary/focus → selection and independent orders →
functional ship bar → user/script command changes → all formation types/hold rules → continuous followers → functional Fleet Commands
panel → regression gate, both-view smoke and Mission 1 / synthetic Mission 5/9 acceptance. The gate (149 Rust / 68 Python, 6 skips), both-view fleet smoke with restart/re-entry and two sizes, synthetic actual Mission 5/9 ECS primary dispatch, Mission 1 opening handoff and isolated Pause/restart smoke passed. Full normal-combat Mission 1 replay and retail comparison are **still outstanding**; synthetic dispatch does not validate later missions. See §5–6 of the specification for checks.

## 2. Mission 1 acceptance — finish validation, not another scaffold

| ID | Status | Gap / acceptance target | Runtime owner and evidence |
|---|---|---|---|
| M1-01 | Unvalidated | Normal-combat replay after the latest polish: tender approach/repair, asteroid fight, pirate fight, navy tow and ending. Confirm late-run frame time/particle counts remain bounded. | `scenario.rs`, `docking.rs`, `collision.rs`, `fx.rs`; [campaign.md — Next](re/campaign.md#next), [effects.md](re/effects.md) |
| M1-02 | Unvalidated | Defeat, restart, leave and re-enter the same mission/menu; check camera, pause, HUD, entities/audio and campaign counters. Existing defeat/restart smoke is useful, not all-transition coverage. | `scenario.rs`, `menu.rs`, `campaign.rs`; [campaign.md](re/campaign.md) |
| M1-03 | Provisional | Refit per [refit-resupply.md](re/refit-resupply.md): no ammunition exists in retail; dock heal lands in one step at Repair Time, release at max(dock time, Repair Effect Time), full refit (revive sections, core, re-arm guns) for bases and campaign worlds, partial for skirmish tenders; crew damage control every 12 s (amount provisional). **Lifeboat creation/pickup is implemented** (`lifeboats.rs`): Warrior's two Pirate boats, pickup by the nearest touching ship, `Player has no lifeboats`, the immediate next-level exit and the ~109 s timer fallback pass scripted ECS smoke (`--script lifeboat-m1`, `lifeboat-m1-fallback`). Not yet validated: a normal-combat replay, the real NIS speech timing of the early exit, and the boats' look/banner. | `docking.rs`, `combat/`, `rules/`, `lifeboats.rs`; [campaign.md](re/campaign.md), [docking-and-orders.md](re/docking-and-orders.md), [lifeboats.md](re/lifeboats.md) |

## 3. Simulation fidelity — behaviours that work but are still stand-ins

| ID | Status | Gap / acceptance target | Runtime owner and evidence |
|---|---|---|---|
| SIM-01 | Provisional | Engine-order mapping, acceleration/braking time scale and native course/rudder controller; compare travel/turn/stop at each order and with damage. | `ship.rs`, `course.rs`; [movement.md](re/movement.md) |
| SIM-02 | Provisional | Native obstacle avoidance/path planning, stance/captain behaviour, target choice. Border zones now leash AI ships (and steer the player back); the exe's border AI is not decoded. Reactive steer-away exists but can fail on concave coasts. | `ship.rs`, `rules/systems.rs`, `collision.rs`; [movement.md](re/movement.md), [collision.md](re/collision.md) |
| SIM-03 | Partial / provisional | Collision sound + provisional ram damage (`collision::Ram` above 8000 impulse -> nearest section, `combat/lob.rs`), rules Movable/Collidable (`Pinned`/`Ghost`). Remaining: impact damage constants; restitution, inertia, damping and docking exemptions guessed. Rock/rock, rock/static and whale/mine contacts absent; fast moving belts lack continuous contact detection. | `collision.rs`, `combat/asteroid.rs`, `audio.rs`; [collision.md](re/collision.md) |
| SIM-04 | Provisional | Base approaches now keep a scenery-clear corner route, with collision enabled until the final docking leg; Mission 2 abandoned-island sailing regression passed. Native dock arrival/path planner, per-section repair jobs, tow approach/cable spring and damping remain provisional. Independent hull motion and deploy/hold/retract scaffold already exist. | `docking.rs`, `collision.rs`; [docking-and-orders.md](re/docking-and-orders.md) |
| SIM-05 | Partial / provisional | Boarding leadership/per-person crews, actual harpoon flight, winch/tension break and player boarding of bases. AI base boarding and ship capture exist. | `boarding.rs`, `options_menu.rs`; [boarding.md](re/boarding.md) |
| SIM-06 | Partial | Functional campaign recruitment, eight per-ship posts, reserve/transfers and whole-bank mount-compatible weapon purchases now exist. Assigned gunners/engineers feed that ship's accuracy/repair; named combat skills replace generic hands in boarding strength. Choices and point spending persist through progression/checkpoint saves; both views have a paged screen. Provisional: availability fallback, mean post skills, unique recruits, full-price upgrades/no trade-in and ship identity. Open: captain bonuses, native unlock/upgrade rules, automatic transfer to replacement hulls and retail layout. Compile-only checked; regression/runtime acceptance left to owner. | `crew_arms.rs`, `campaign.rs`, `combat/`, `boarding.rs`; [scoring-saves.md](re/scoring-saves.md), [boarding.md](re/boarding.md) |
| SIM-07 | Provisional | Lob guns fire on their LobAngle arc with a solved speed and burst where they come down (star mortar: growing 60-unit blast; nova mortar: five secondary blasts; gravity charge: 17 s well), mines drift/home on ships within 50, point defence shoots torpedoes and mines, warning-shot mode, NIS gun accuracy, crew-gunnery accuracy (`combat/lob.rs`). Data: the exporter now writes `kind/blast/mine/travel_snd/ricochet_snd/decal` per bullet. Open: torpedo arming (no data found), exact lob/well/mine constants, ricochet rule (factors exported, formula unknown). | `combat/`, `data.rs`; [gunnery.md](re/gunnery.md) |
| SIM-08 | Provisional | Fire-fighting, critical-hit feedback and section destruction/movement-loss semantics need full comparison; avoid replacing already recovered damage rules with guesses. Iron War torpedo-boat follow-up: provisionally stop destroyed non-vital sections absorbing projectiles and redirect clicked aim from a destroyed sail to the hull (bounds-based, preserving overlapping hull targets); destroyed vital sections remain hittable for core damage. Regression tests added, not run; replay Iron War as Procyon to validate. Damaged-mesh collision and mouse-pick silhouettes remain unresolved. | `combat/damage.rs`, `combat/mod.rs`; [movement.md](re/movement.md), [effects.md](re/effects.md) |
| SIM-09 | Provisional | Storms travel their group's route; 'Lock Objects' carries ships; lightning, wind and meteor strikes use separate retail weather damage tables; exe-derived storm-wide pulses, lightning segment intersections and size-based meteor rolls replace guaranteed per-ship hits (endpoint sampling, recharge floors and forces still provisional); Etherium currents push along their path (WOT Magnitude 25000 / Radius 60) and draw two flowing Current-texture ribbons (the prior particle-based EtheriumRiver call drew nothing); nebula fade-out distance. Mission 2 shower direction corrected: target/angle position and velocity contributions now combine independently, producing vertical rain, and the authored `Flat` render mode now lays splash rings horizontally; each streak's start height now matches its fall speed/lifetime to finish on the flight-plane splash surface (provisional); authored-data regressions, full gate and Classic effect smoke passed ([effects.md](re/effects.md#mission-2-meteor-shower-direction-correction)). Open: inside-cloud overlay, exact rates, black-hole force law. | `environment.rs`, `nebula.rs`, `blackhole.rs`, `rules/`; [nebula-landmark.md](re/nebula-landmark.md), [effects.md](re/effects.md) |
| SIM-10 | Partial | Dragons exported and spawned (`dragons.rs`: 500 HP, stances, attack orders, damage-threshold break-off to the return point, border zones, fire breath, roars); wrecks are scenery the rules teleport/ghost. Open: whale skeletal swim animation/damage; dragon skeletal animation; asteroid fragment/contact fidelity. | exporters, `environment.rs`, `dragons.rs`, `scenario.rs`, `combat/asteroid.rs`; [dragons.md](re/dragons.md), [asteroids.md](re/asteroids.md), [campaign.md](re/campaign.md) |

| SIM-11 | Partial / provisional | Shared frame-boundary sensing now uses strict collision-sphere surface distance, authored class sight and group spotting for radar/star map/labels/reticles/picking, Focus/docking, crew spotting and ship/base acquisition. The invented 40-unit nebula bypass is removed; same-cloud interior sight is allowed. Range/group/cloak/script regressions and Classic/Remaster radar ECS smoke pass. Open: native mesh hiding/contact memory, cloak/NIS/range-modifier semantics, scenery UI distance, moving-storm/overlapping-cloud geometry and specialized dragon/projectile acquisition. All inspected scenery polygon counts are zero; no invented island mesh-based sensor blocking. | `environment.rs`, `data.rs`, `docking.rs`, `combat/`, `panels.rs`, `starmap.rs`, `world_hud.rs`, `view.rs`, `options_menu.rs`, `crew.rs`; [visibility.md](re/visibility.md) |
| SIM-12 | Partial / provisional | Native text serialization confirms column-first matrices: corrected scenery yaw/full pitch-roll and campaign ship/animal headings; all scenery now uses the full orientation for meshes/docks/guns/colliders. Authored-axis/catalogue and Mission 2 approach/dock/repair regressions pass; isolated Storm Front rendering smoke completes. Height still uses the first-dock datum workaround: native node pose application subtracts a rotated pivot, so MDB/MSB pivot/pose semantics must be decoded before replacing it. Open: original visual comparison and normal mission/NIS replay after orientation changes. | `scenario.rs`, `campaign.rs`, `environment.rs`, `dragons.rs`, `tools/mdb_to_glb.py`, dock/base/collision exporters; [scenery-placement.md](re/scenery-placement.md) |

## 4. Campaign, front end and presentation

| ID | Status | Gap / acceptance target | Runtime owner and evidence |
|---|---|---|---|
| CAM-01 | Unvalidated / partial | Missions 2–13: validate one at a time, including alternate/failure paths; fix mission prerequisites before claiming playability. All rule types are now interpreted (AUD-05). Lifeboat prerequisites are done: Mission 2's player and Lady J pickup paths, Mission 4's pre-placed boats, Mission 9's combat longboat (not a lifeboat) and the creation flag in 5-7, 10, 12 pass scripted smoke only; owner playtesting Mission 2 is underway. Abandoned-island docking, camp owner-change/first-shot progression, base gun placement, shared dock-height normalization across dockable scenery (including all three Mission 2 bases and Coventry docking/repair at Norville), live tower targeting and Lancer-only tower engagement for the torpedo boat (torpedoes reject bases) are corrected with regressions and Classic focused smoke ([campaign.md](re/campaign.md#mission-2-owner-playtest-fixes-islands-and-gun-towers)); the actual Mission 2 torpedo boat's front Lancer now destroys both towers through real projectile hits, with torpedo fire explicitly forbidden in an isolated invulnerable-player regression, SOS beacon now retains its fixed particles and authored looping glow, with both volume-based pickup rules tested; no complete normal-combat replay of Mission 2-13 yet. | `campaign.rs`, exporters, `rules/`; [campaign.md — Missions 2–13](re/campaign.md#missions-2-13-first-pass-untested-in-play) |
| CAM-02 | Partial / provisional | Rule semantics: approximated vital-section condition, top-speed-as-throttle cap, group/owner matching, native event/condition timing and the provisional readings of the AUD-05 actions. | `rules/eval.rs`, `rules/systems.rs`; [campaign.md](re/campaign.md) |
| CAM-03 | Partial | Mid-mission save/load. Current saves restart the mission with campaign counters; they do not snapshot ships, projectiles, AI, timers and rule progress. | `campaign.rs`, `menu.rs`; [scoring-saves.md](re/scoring-saves.md) |
| CAM-04 | Partial | `stats.rs` Game Summary (ships left/sunk, damage inflicted/taken per player) on the mission end text and the skirmish end; Crew & Arms screen opened by the rules now supports purchases, eight-post assignment and bank equipment (`crew_arms.rs`, both views; SIM-06). Open: difficulty effects, retail stats/outfitting layout and native economy calibration. | `campaign.rs`, `menu.rs`, `rules/`, `stats.rs`, `crew_arms.rs`; [scoring-saves.md](re/scoring-saves.md) |
| FE-01 | Provisional | Sound Options (overall/music/effects/dialogue volume) and Display Options (window mode, effect detail, background, flag waving) in both views, saved in `settings.json`; Tutorial screen (Basic / Full Training) runs the tutorial maps. Open: profiles, sound channels/texture detail (no remake equivalent); inventory other retail front-end paths. | `menu.rs`, `classic/menu.rs`, `remaster/` |
| FE-02 | Partial / provisional | Shared Arms editor now fits each assembled skirmish ship independently and carries compatible bank choices into battle/restart in both views. Cross-family changes use exported mount size/race restrictions; fixed mounts protected. Compile-only checked, owner acceptance pending. Open: victory-point budgets/Auto Assign (weapon fitting currently free, never spends campaign points), crew/fleet-profile load-save and 3D ship preview; AI fleet construction approximate. | `menu.rs`, `classic/menu.rs`; [skirmish-setup.md](re/skirmish-setup.md) |
| AV-01 | Missing | Pre-rendered campaign/ending movies and their skip/transition flow. Use converted assets, not the original playback DLL (needs a video playback dependency). | `menu.rs`, `campaign.rs`; [movies.md](re/movies.md) |
| AV-02 | Provisional | Native NIS spline/acceleration/focus transitions, later-mission opening handling, portrait expressions and speech cadence. In-engine NIS, talking-head device and NIS zoom already exist. | `briefing.rs`, `rules/systems.rs`, `panels.rs`; [campaign.md](re/campaign.md) |
| AV-03 | Partial / provisional | Mesh-shifting/skeletal effects, no-expiry lifetime, emitters, wake/sail/sky geometry and blend rules; do a scenario-based visual pass, not global brightness guesses. Damage decals added (`decals.rs`). | exporters, `fx.rs`, `wake.rs`, `nebula.rs`, `decals.rs`, `classic/`; [effects.md](re/effects.md), [reference-video.md](reference-video.md) |
| AV-04 | Partial / provisional | Done: collision, shell travel loops, warning-shot ricochets, black hole / meteor / abyss beds, pause and NIS-bar cues, crew hazard/weapon/fire/collision/out-of-bounds callouts and faction spotting, crew speech toggles. Open: faction voices, spatial mix/falloff. | `audio.rs`, `crew.rs`; [sound.md](re/sound.md) |
| AV-05 | Provisional | Ship destruction now marks gameplay death immediately, keeps the damaged hull visible for a nose-first descent, then plays the hull's final explosion and removes its child presentation (including damage smoke/engine effects). Dead ships no longer create wakes. Owner reported this sequence from retail; 4 s duration, 65° pitch and radius-scaled descent are provisional. Scripted Destroy retains immediate hiding; its damage smoke is cleaned up. Regression added, not run; runtime/retail acceptance left to owner. | `combat/mod.rs`, `combat/damage.rs`, `wake.rs`; [effects.md — Ship sinking](re/effects.md#ship-sinking-and-final-destruction) |
| INV-01 | Unknown | Tutorial/content entry paths (tutorials now reachable), all ship/weapon variants, scenery gameplay and alternate campaign branches need a complete manual/data/export/runtime inventory. An asset existing does not establish runtime support. | manual, TST/TWT/WOT, exporters, `scenario.rs`, `menu.rs` |

Multiplayer/network/chat parity is **deferred outside the current single-player target**, not implemented or silently counted as complete.

## Rule omissions that warning-only coverage misses

Direct census of exported Mission01–13 `rules` and source review of `rules/eval.rs`. Mission numbers indicate **presence**, not proof of a blocker.
Setup may apply some fields elsewhere (for example island combat strength at spawn); trace the entire path before declaring a no-op a bug.
**Since AUD-05 every row below has a provisional implementation**; the table is kept as the census of where each is used.

| Actions/conditions requiring follow-up | Missions | Current handling / backlog |
|---|---|---|
| Set Lifeboat Creation State; Player has no lifeboats; Player Vs Player capture count | Creation: 1–12; no-lifeboats: 1; capture count: 2, 6, 12 | Implemented (flag on `MissionShip`, owner list, lifeboat-only table). Provisional: scripted `Destroy` leaves no boats; shells pass through boats; mission 12's defeat keeps a base-boarding bridge (below). M1-03 / CAM-01 |
| Set FleetFormation Type; Set Fleet Hold Formation | Type: 1,3,4,5,6,7,9,10,11,12; hold: 3,5,9,11 | Shared named-fleet type / per-member hold implemented; layout/controller calibration provisional. UI-03 |
| Set Fleet Primary Ship; Set Group/Unit AI Captain | Primary: 5,9; captain: 3,5,7,8,9,11 | Primary dispatch implemented (cross-group fallback provisional); Captain_Human = no own captain AI, takes fleet orders. UI-01 / SIM-02/06 |
| Open Crew and Arms Screens | 3,5,7,9,11 | Functional recruitment/post assignment/bank purchases; provisional economy and layout, acceptance pending. SIM-06 / CAM-04 |
| Border Zone / Clear / Return Point | Set: 1–3,5–12; clear: 1,3,9,12; return: 2,6,7,8,10,12 | AI leash + player steer-back. SIM-02 |
| Mission Essential; Towable; Movable; SetCollidable; Radar Active State | Essential: 1–12; towable: 1,2,5,8,11; movable: 4,5,9,11; collidable: 4,6; radar: 1–3 | Essential = primary-alive loss rule; NoTow / Pinned / Ghost; radar blanks contacts. CAM-02 |
| Crew Speech toggles / Helm Off Course; Warning Shot Mode | Toggle: 2,3,6,8–11,13; helm: 2,6,12; warning: 5 | Crew callouts muted/unmuted; out-of-bounds line; harmless shells. AV-04 / SIM-02 |
| Create/Release Event Effect; Play Special Effect | Event: 2,5,6,11; special: 2,3,5,6,9 | Implemented (AUD-04). AV-03 |
| NIS Zoom / gun accuracy / object visibility; nebula fadeout / lock | Zoom: 3,4,9; fadeout/lock: 3; other NIS actions need scenario review | Zoom eases FOV; perfect gunnery; object visibility stored as a flag only; fade/lock implemented. AV-02 / SIM-09 / CAM-02 |
| Setup Etherium Current; Dragon actions; Mission 9 specials; HUD Texture Overlay | Currents: 3,4,10; dragons: 6–8; specials: 9; overlays: 13 | Implemented provisionally. SIM-09/10 / CAM-01 / AV-03 |

`Set Is In Calyan Abyss` plays an ambience bed; `*State Init* Setup Island` is applied at spawn (owner, combat strength).
The existing known-warning test must not be described as evidence that these effects work.

## Automated gap audit (AUD)

Mechanical, repeatable checks (not play-testing). Re-run after changes: `python3 tools/audit_assets.py`, `python3 tools/audit_rules.py`
Results are **leads**: a name missing from our code may be built dynamically or irrelevant. Verify each against the original game before implementing.

### AUD-01 Converted assets nothing references (290 of ~2000)
Stem not found in `game/src` or any `assets/**/*.json`. Groups, most likely feature first:

| Group | Count | Likely missing feature |
|---|---|---|
| `sound/impacts` | 69 | Per-material hit sounds beyond the wired subset: `damage_metal06-10`, `damage_sail*`, `damage_stone*`, EtheriumShock variants. Exe pools only have 01-05 (see sound.md), so some may be unused retail too |
| `sound/weapons` | 51 | Ricochet (`*_richocet*`), travel sounds, multi-fire (`cannon_fire*multi`), Arcturian fire. Ricochet/travel slots exist in bullet data and are unwired |
| `sound/ui` | 47 | Front-end sounds (`fe_*`: add, autoselect, dragndrop, button select) and HUD cues |
| `hud/tex` | 47 | Main-bar art: movement-type, chat, formations, panic, pause buttons, left detail. Cross-check UI-04/05/06, Panic order |
| `sound/ambience` | 32 | Black hole, nebula, solar storm loops, animals (mantaray, space dragon, whales) |
| `flags` | 31 | `flag_civilian`, `flag_navy01-05`: faction/fleet flags other than navy00 (flag.rs hardcodes one) |
| `sound/ships` | 7 | Cloak/uncloak loop (UI-05), ship rebuild loop/scaffold (refit), rope break |
| `models/ui` | 2 | Small selection indicators (arrow/compass) |
| `menu` | 2 | `fe_shipscreen_bg`, `frontendmap` (ship screen, front-end map) |

(Pre-AUD-05 counts; travel/ricochet, hazard ambience, dragon sounds, cloak sounds and the movement-type button are now used.)

### AUD-02 Rule vocabulary used by shipped maps vs `rules/eval.rs`
130 distinct condition/action types appear in the 13 missions + skirmish/tutorial maps. Before AUD-05, 11 were unhandled (warn) and 23 were
accepted no-ops; the census below is that pre-AUD-05 state. All are now interpreted.

```
## UNHANDLED (11 of 130 distinct types used)
act  *State Init* Setup Etherium Current  [M03x1, M04x1, M10x3]
act  Close HUD Texture Overlay  [M13x25, basic_tutorialx3, tutorialx10]
act  Dragon - Set AI Stance  [M06x6, M07x2, M08x1]
act  Dragon - Set Damage Threshold  [M06x4, M07x8, M08x1]
act  Mission 9 - Do Dark Matter Explosion  [M09x1]
act  Mission 9 - Teleport Longboat  [M09x1]
act  Open HUD Texture Overlay  [M13x25, basic_tutorialx6, tutorialx42]
act  Open Weapon Bar  [tutorialx2]
act  TUTORIAL - Pause when starmap opens?  [tutorialx2]
cond Is Starmap Open  [tutorialx1]
cond Mission 9 - If Mortar explodes within Area  [M09x1]
## NO-OP (23 of 130 distinct types used)
act  *State Init* Setup Island  [M01x11, M02x10, M03x3, M04x1, M06x5, M07x1, M08x3, M09x1, M10x8, M11x36, M12x1, M13x5, tutorialx1]
act  *State Init* Setup Nebula  [M01x1, M02x2, M03x2, M04x1, M05x1, M07x6, M08x6, M09x1, M11x2]
act  Clear Group/Unit Border Zone  [M01x2, M03x3, M09x2, M12x5]
act  Create/Release Event Effect  [M02x5, M05x6, M06x10, M11x1]
act  Crew Speech - Helm Off Course  [M02x1, M06x1, M12x1]
act  Crew Speech - Toggle On Off  [M02x4, M03x2, M06x2, M08x1, M09x4, M10x1, M11x2, M13x4, basic_tutorialx1, tutorialx2]
act  NIS Toggle All Objects Visibility  [M05x1, tutorialx1]
act  NIS Toggle NIS mode Gun Accuracy  [M05x2, M11x2]
act  NIS Zoom  [M03x3, M04x1, M09x1]
act  Open Crew and Arms Screens  [M03x1, M05x1, M07x1, M09x1, M11x1]
act  Play Special Effect  [M02x2, M03x6, M05x1, M06x5, M09x2]
act  Set Group/Unit AI Captain  [M03x1, M05x1, M07x1, M08x1, M09x4, M11x1, tutorialx2]
act  Set Group/Unit Border Zone  [M01x3, M02x3, M03x8, M05x2, M06x11, M07x7, M08x1, M09x3, M10x2, M11x10, M12x5]
act  Set Group/Unit Border Zone Return Point  [M02x3, M06x7, M07x2, M08x1, M10x1, M12x2]
act  Set Group/Unit Mission Essential  [M01x1, M02x1, M03x1, M04x1, M05x3, M06x1, M07x3, M08x2, M09x5, M10x1, M11x4, M12x1]
act  Set Group/Unit Movable  [M04x4, M05x1, M09x2, M11x4, basic_tutorialx2, tutorialx2]
act  Set Group/Unit Towable  [M01x5, M02x4, M05x7, M08x2, M11x9, basic_tutorialx2, tutorialx4]
act  Set Group/Unit Warning Shot Mode  [M05x2]
act  Set Is In Calyan Abyss  [M07x2, M08x2]
act  Set Nebula Fadeout Distance  [M03x2]
act  Set Nebula to Lock Objects  [M03x2]
act  Set Radar Active State  [M01x1, M02x1, M03x1]
act  SetCollidable  [M04x1, M06x6]

## exe rule names with no use in shipped maps: 2
```

### AUD-03 Original-game name tables
The names below are enum tables in the original game (captain/order enums, collision and spotter pools), so "who uses it" needs
data (TWT/WOT) evidence. Findings:

| ID | Finding | Evidence | Verdict |
|---|---|---|---|
| AUD-03a | **Damage decals** are a hull/sail damage-mark system. Retail ships 93 TGAs in `BD_DecalTextures/` (`sail_hole1-18`, `sail_burned1-2`, `dent_*`, `hole*_wood*`, `SteelDecal1-18`, `wall dmg01-19`, `burn*`, `big_hole*`, `rudder_dent`, `engine_part`). Names are **hard-coded in the exe**, not in WOT. Texture-name suffix `_Small/_Medium/_Large` picks a size class 0/1/2, and the textures are stored in per-material buckets (5 flag bits: wood, reinforced, iron/steel, sail, stone-wall) so a hit chooses material bucket then size. | exe name table, `BD_DecalTextures` listing | **Implemented provisionally** (AUD-05: `decals.rs`, name-based buckets). Still unknown: trigger (per hit vs. section threshold), projection/placement, cap per ship. Next probe: what triggers the decal manager |
| AUD-03b | **Crew voice callouts**: `content/voice/events.json` has ~100 events per race (spotter, engineer, gunner, rigger, captain, first mate). Spotter ids in the exe map names to event codes (`SPOTTER_*` -> 0x1e/0x1f/0x21/0x22/0x2b, 0..2) | `events.json`, `audio.rs` | Largely implemented (AUD-04/05, `crew.rs`). Grouping/trigger mapping is community-derived (provisional) |
| AUD-03c | **AI captain**: mission rules only ever set `Captain_Human` (11 uses, M3,5,7,8,9,11, tutorial) on a named ship. Brave/Cautious/Opportunistic/Supportive/BodyGuard/GungHo/Neutral exist as the enum but no shipped mission or WOT file uses them | `rg Captain_ assets` | `Captain_Human` implemented (AUD-05). Others: low priority, no data uses them |
| AUD-03d | `ORDER_PANIC`, `ORDER_RAM`, `ORDER_GRAPPLE/DOCK/BOARD/TOW`, `ORDER_CANCEL_ALL_ACTIONS` are the AI/command order enum. `ORDER_PANIC` is the bar "panic" button (art `bar_navy_mainbar_panicbutton`) | enum init, hud art | Panic button exists (`panels.rs`); the ram order flies bow-on (AUD-05) |
| AUD-03e | `Ship_Collision_Wood/Metal01-05` and `Explosion_Light/Small` pools exist and are registered like the damage pools | `collision.rs` vs `audio.rs` | Collision pools wired (AUD-04) |
| AUD-03f | Selection indicator arrow/compass GLBs, waypoint/lob cursor art, `HUD_FireAll`, lob accept/cancel/reject sounds are exported but unreferenced | AUD-01 | Waypoint accept/decline art used by the advanced plot (UI-04); the rest open |

### AUD-04 Implemented from this audit (provisional)
- `game/src/crew.rs`: `Tracker` (pure, tested) turns per-frame snapshots of the player's situation into crew events: `hull_damage` (75/50/25 %), `sails_damaged/destroyed`, `all_sails_destroyed`, `enemy_ship_spotted` (class sight range), `enemy_ship_destroyed`, `ship_lost`, `dock/undock`, `landing_party`, `mission_won/failed`.
  One line at a time (also yields to the helmsman), 15 s per-event cooldown, queue of 3. Fixed Navy human-male crew. AUD-05 added hazards, weapon reports, fires, collision alarm, out of bounds and faction spotting; engine/rudder hits, boarding defence and retreat remain.
- `collision.rs::sail`: `Ship_Collision_Wood|Metal01-05` on ship/rock/wall contact, throttled to one per 0.5 s (hull material from the first section; max distance 200 from the exe pool). Provisional: impulse threshold and pool choice.
- Audit correction: most "unreferenced" sounds in AUD-01 are built from WOT/bullet data names at runtime; only ricochet/travel slots, `sound/ui` `fe_*`, cloak/rebuild and unused hazard ambience remained genuinely unwired (AUD-05 wired travel, ricochet on warning shots, cloak and the hazard beds).
- Mission rules (second batch): `Create/Release Event Effect` (MoonFish, BottleRocket_Light_Impact looping at every point of the set; exported `fx/MoonFish`, `fx/BottleRocket_Light_Impact`), `Play Special Effect` (dialogue-folder sound by name, no subtitle; files copied to `maps/<id>/speech/`), `Open/Close HUD Texture Overlay` (73 stitched tile textures in `assets/hud/overlay`, `tools/export_overlays.py`; mission 13 is the **end credits**, verified; tutorial pages), `Open Weapon Bar`. Overlay drawn by the classic view; centred native size is a guess. Mission Essential needs no change: all rules only toggle it on the player ship, which the primary-alive approximation already covers.
- The earlier failure of `ship::movement_tests::ship_keeps_speed_and_heading_past_last_waypoint` was the test picking class 0 (now an immobile base); fixed in AUD-05.

### AUD-05 Implementation batch (2026-10-13), provisional unless stated
Every rule action/condition used by the 13 missions and both tutorials is now interpreted (the known-warning list in `rules/tests.rs` is empty):
border zones + return points (AI leash, player helm-off-course return), Towable/Movable/Collidable, AI Captain Human, crew speech toggles, radar,
NIS zoom/gun accuracy/object visibility flag, nebula lock/fade, warning shots, Etherium currents, dragon stance/threshold, mission 9 teleport
longboat / mortar-in-area / dark-matter blast, Calyan Abyss bed, tutorial star-map pause and `Is Starmap Open`, Open Crew and Arms Screens.
Also: damage decals (`decals.rs`, `tools/export_decals.py`, name-based material buckets), wrecks, nova mortar / gravity charge mounts
(11 previously skipped), 29 new effects, area weapons (`combat/lob.rs`), dragons (`dragons.rs`), cloak (`cloak.rs`), storms, currents,
refit timing, Game Summary (`stats.rs`), Crew & Arms screen (`crew_arms.rs`), Sound/Display options, Tutorial screen.
Validated only by `tools/check.sh` (171 Rust / 72 Python) and start-up smoke of missions 1, 3, 6, 9, 12, both tutorials, a skirmish
victory and the options/tutorial screens; **no play-through**.

**Note on data drift:** some entries added in this batch were first exported from a modded install. The assets were later regenerated
from a vanilla install; import from a vanilla install when checking retail values.

Not done at AUD-05: pre-rendered movies (needs a video playback dependency), mid-mission save snapshots, whale/dragon skeletal animation,
crew/arms spending, difficulty, native NIS splines, retail calibration of every PROVISIONAL constant above. Crew/arms spending and post assignment have since landed provisionally (SIM-06 / CAM-04).

### Roadmap to "feature complete" (execution order)
1. ~~Cheap audio completeness~~ largely done (AUD-04/05); remaining: `fe_*` front-end sounds, faction voices, spatial mix.
2. ~~Crew voice system~~ largely done; remaining: engine/rudder hits, boarding defence, retreat.
3. **Mission 2-13 playthrough support:** all rule types interpreted (AUD-05); now play each mission and fix what blocks it.
4. ~~Decals~~ provisional (AUD-05); finish the trigger/projection probe.
5. **Remaining UI:** selection indicators, remaining HUD art, retail stats/outfitting layout. Crew/arms spending screens are functional provisionally; see SIM-06.
6. Then retail comparison/calibration passes (movement, AI, weapons), which cannot be proven from static analysis alone.


## Open probes

| Area | What is established / next question |
|---|---|
| ShipBar / OpenShipBar input | Toggle, secondary roster, selection and draw paths known. Overflow/status follow-up in [fleet-controls.md](re/fleet-controls.md). |
| ChangePrimaryShipCommand / SelectSecondaryShip input | Input and command execution known, including campaign restriction and script route. Lifecycle/cross-group unknowns retained. |
| ShipMoveInFormationRoutine / SetShipsFormationCommand | Layout/hold state, UI toggle feedback and named-fleet rule consumers known. Assignment/controller calibration remains; see specification. |
| Lifeboat pickup and sound | Implemented (see [lifeboats.md](re/lifeboats.md)); open questions there (§4) remain, notably scripted-destroy spawning, shell vs boat, and how mission 12's `Procs vs RoyalPalace` can be true in retail. |
| AttackOnlyMode input | Remake binding implemented (clicks only target); dispatch and click effects to confirm. |

Do not invent a coverage percentage from name matches; define explicit evidence-backed obligations and checks first.

## Working approach from now on

1. **Inventory before more polish:** UI-01/02/03 now have [manual/data and input/command specifications](re/fleet-controls.md); resolve their explicit blocking unknowns and follow the ordered checklist. Include ignored rule effects and unexported objects in the inventory, not just executable names.
2. **Implement one complete functional batch:** fleet selection, command-ship switching, ship bar and formation controls have higher value than another isolated decorative tweak. Keep behaviour in the shared modules, not in the view.
3. **Close Mission 1 validation:** M1-01/02 after that batch, with normal combat and no autokill. Record what passed and what did not; don't reopen completed milestones without reproduction.
4. **Remove Mission 2 prerequisites:** lifeboats are done (scripted smoke); refit follows the RE spec; then play Mission 2. Continue mission-by-mission, prioritising mechanics needed by the next mission.
5. **Calibrate then polish:** movement/AI/weapon/crew behaviour from evidence, followed by coherent reference-scene visual/audio comparisons. Keep guesses explicitly provisional.

For each work session, pick an ID, state the acceptance scenario, inspect its owning code/callers, investigate only unresolved original behaviour,
and update this ledger plus the relevant `docs/re/` note. Run `tools/check.sh` for implementation changes and runtime smoke for ECS/UI changes.
Documentation-only inventory work needs link/diff checks, not a game build.
