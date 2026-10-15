# Campaign mission 1: retail data and first playable scaffold

Source is **World/SinglePlayer/Mission01.twt**, not the owner's SavedGames snapshots.
The installation is read only; no original executable was run or modified.
`tools/export_campaign.py` exports converted data to `assets/maps/mission01/mission.json`
and scenery to `assets/maps/mission01.json`. The campaign map is deliberately not
in the skirmish index.

## Confirmed from mission data (not executable-measured semantics)

- Primary ship: object 231, **RLS Comet**, `Ship_Navy_Torpedo`, owned by
  `Player0 Group`. The opening NIS moves Jim from his placed position to the
  player start point. The scaffold places Jim at that point during the newly implemented
  in-engine briefing; the actual arrival animation is still absent.
- 11 ship placements. Red fleet: **RLS Lyonesse** (Amelia's heavy escort),
  **Explorer** and **Crusader** (Navy sloops), **Swift** (Jim-variant torpedo boat).
  Blue fleet: **Sovereign**, **Vanguard**, **Lightning** (fast frigates).
  Tenders: **Bounty** and **Providence** (Feather variant). Pirate schooner:
  **Warrior**.
- 21 waypoint paths, 23 polygon volumes, 11 objective points and 10 objective tasks.
- The rule-list count field says 136, but there are **135 explicit Rule Name
  records**. The exporter preserves their order, repeated condition/action
  blocks and source line numbers. It does not invent a missing rule or collapse
  duplicate names. This count discrepancy is unresolved.
- Actual sky is `skybox_22_Layer1`. Initial music is `UNDRSCORE_Welcome`.
- All three exercise fleets start with invulnerability 1.0. The first mock battle
  is ended by a 95-second timer, not by destroying the blue fleet.
- The original route goes west through the archipelago, to Clarke, west around
  the nebula, through another mock engagement, to the weapons tender, then the
  asteroid belt and finally the captured tender.
- The second exercise uses black-flag surrender logic and a 122-second fallback;
  the task-completion rule also refers to 120 seconds.
- Warrior initially tows Providence. A damage threshold of 45% causes a break-tow
  order and aggression against Jim. Amelia can intervene; defeating Warrior is
  followed by victory dialogue and pirate lifeboat collection, not simply the
  skirmish victory screen.

The converted rule records are **evidence**, not a claim that every condition and
command has been implemented. Their strings describe designer intent; exact
native condition timing, group matching, damage fractions and NIS/speech queues
still require executable investigation.

## Implemented: rule interpreter (`game/src/rules/`)

Run `cd game && cargo run --release -- --mission 1`, or Single Player → Start New Campaign. `R` restarts, `F10` returns to the menu.
The old hard-coded stage machine is gone: all mission flow comes from the 135 exported rules, evaluated every frame
(`Script::step`, pure and unit-tested). All 19 condition types and 52 action types of the mission are either implemented or
explicitly listed below; unsupported ones are logged once (`unsupported ...`) and a test fails if a new unknown type appears.

- **Conditions** (all of a rule's conditions must hold): flags, named timers (`Start Timer` resets to 0; Greater/Less Than need a started timer),
  speech played / not yet played, Is Group In Volume / Enter Volume (polygon test), Is Group A attacking Group B, hit counts (by anyone or by a
  player), group member counts, damage fraction, mission-essential ships, unit flag texture (original group membership), group distance, docked.
- **Actions**: paths (with Find Closest Point, per-ship 35-unit lateral offsets as a stand-in for formations), max throttle (`Ship.cap`), AI stance,
  owner changes, visibility (hidden ships cannot be targeted or hit), vulnerability (`Invulnerable Percent` scales damage; 1.0 also blocks fires),
  flag texture and banner tint, attack orders, Clear all AI commands, Damage by percent, hold position/fire, reset hit counts, alliances (a
  hostility set between players; default: only the pirate schooner is hostile), towing (Providence trails Warrior) / Break Tow, objective points and
  tasks (HUD text), speech queue with subtitles (one line at a time, sequential), music with fade in/out, NIS bars + camera
  (attach / path flight / focus), Focus Camera On Group, victory points, End Game / Goto Next Level (end overlay).
- **Hostility drives combat**: each frame `Ship.side` = Enemy iff the unit's owner is hostile to the player's owner; non-player units pick targets
  by stance (attack orders > hostile in range) and hold fire per owner. Shell hits and gun bursts are messages (`Hit`, `Fired`) feeding the rules.
- **Opening**: `briefing.rs` still plays the opening NIS camera/speech queue (rules named `Opening NIS*`/`START Opening NIS` are skipped); it marks its four
  speech events as played, which fires the retail rules that follow (`END Opening NIS`, music, fleets' start, Jim/Onyx lines). Jim now drives the
  retail `Jim Arriving @ Start of Mission` path from his placed position (rule 1), which is the arrival animation.
- **Asteroids**: retail group membership selects the 24 belt rocks and 20 Montressor rocks. Ordinary asteroids use WOT HP/radii, tumble at rule-specified speeds, and recursively split into smaller rocks with flash/smoke/sparks. Red ships' belt attack order also selects the fragments. Sphere shooting and Stone damage rolls remain; fragment velocity/group inheritance and collision response are partly provisional. See [asteroids.md](asteroids.md).
- **Docking**: left-click RLS Bounty or use the right-click menu (`D` is a dev shortcut) once it is in sight (docs/re/docking-and-orders.md); no refit/torpedo resupply yet.
- Smoke test: `--script mission` autopilots the player along the objectives (follows the point, shoots hostiles/asteroids, docks). `PROCYON_AUTOKILL=1`
  also sinks the pirate when close, to reach the ending in a test.

PROVISIONAL in the interpreter: condition combination (AND only; no OR seen in this mission), Enter Volume is level-triggered for run-once rules
(edge only for repeating ones), 'attacking' = attack order or fired at the group in the last 6 s, damage fraction = max(core loss, mean section loss),
camera smoothing/path speeds, music volume scale (0.5 x script volume), speech without audio shows its text for max(2 s, chars/14), and `Set Max Throttle Percent` multiplies the ordered speed.

## Clarke and space whales

Mission 1 now exports `environment` alongside its ships/rules: animal IDs, their named groups, world point sets, nebula setup and whale sound/collision settings.
`game/src/environment.rs` renders the retail `OvalNebula` mesh at Clarke's exported point (magnitude/radius ~381), using Nebula.eft's additive blend and initial tint,
and plays the retail `Ambience_Nebula` loop with provisional distance gain. Lightning, meteors and rotational winds are off in this mission and are not added.
Nebula energy drain is separate from permanent sail damage: PROVISIONAL 0.12/s down to 25% sail energy, recovering 0.08/s outside the polygon.
Movement and the sail-power gauge use that value. PROVISIONAL sensor occlusion blocks lines crossing Clarke's polygon, except within 40 units;
selection, ship targeting/AI, radar contacts, labels and reticles share this check. The cloud currently has no native mesh shifting/edge-fade shader.

The four whales (205–207 in `Whales`, 217 in `Whales 2`) use the retail `SpaceWhale_Layer1` mesh and placements. Their groups stay separate even after both
receive the `Whales` owner. The existing rule list controls hiding/reveal, the looping `Space Whale Path` and velocity 12.
Animal positions are included in the interpreter snapshot, enabling the spotting lines and the attack/reprimand timer. Visible whales are selectable,
guns lead their motion, and impacts consume shells. The reprimand's alliance change stops further selected attacks.
The WOT's six calls, 3–10 second call interval and 400-unit sound distance are exported; call ordering, mix and flock offsets are provisional.
The animated MSB is unsupported: swimming uses the bind-pose mesh with gentle roll, not skeletal fin/tail animation. No whale HP/death model is invented.
Initial whale attackability is provisional because the default animal/player relationship has not been recovered.

Validation includes synthetic environment-export tests, sound-source precedence, whale rule paths/visibility/reprimand, looping paths, energy limits and polygon
occlusion. A local-only preview mod revealed/started the whales at initialization and moved the player near Clarke for screenshot checks; this is not a full
normal-timeline Mission 1 playthrough.

## Remaining fidelity gaps

Current consolidated checklist: [fidelity-backlog.md](../fidelity-backlog.md). The earlier scaffold gap list is superseded:
boarding/capture, talking-head portraits, checkpoint saves, independent towing, physical collisions and provisional AI steer-away now exist.

Lifeboat creation, drift and pickup now exist (`docs/re/lifeboats.md`, `lifeboats.rs`). Still incomplete: border zones, player boarding of bases, tender refit/torpedo resupply,
mid-mission snapshots, stats/crew/arms flow, pre-rendered movies and Mission 2–13 gameplay validation.
Nebula/whale behaviour, native tow/collision/path planning, NIS camera, AI stance/captain semantics and condition timing remain partial/provisional.
See “Next” below for the latest Mission 1 remediation and what has actually been revalidated.

## Checks performed

- Rust tests (`cargo test --release`, 12): every condition/action type of the mission is handled; world-init rules (flags, objective point, timers, Jim's
  path, music); Blue leaves at world timer 46 and Red at 51 only while the start flag is false; alliance set/clear; volumes/timers/speech conditions;
  speech without audio completes; opening cue boundaries and path endpoints; menu, HUD and movement tests.
- End-to-end smoke run (`--script mission --speed 10`, `PROCYON_AUTOKILL=1`): the autopilot followed the objectives through both fleet exercises, the black-flag
  logic, Clarke NIS cameras, docking at the tender, the asteroid belt, the pirate fight and the ending overlay ("Mission 1 complete", victory points 3).
  Screenshots: `local/nis_a.png` (script NIS: bars, attached camera, subtitles), `local/end.png` (end overlay). This automated run did not validate
  normal combat or continuous manual play; at `--speed 10` the headless audio backend can flood ALSA errors (run with a small log limit).
- Owner manual playtest: Mission 1 completed end-to-end and reported to work nicely. This validates the normal mission path,
  not all defeat/restart/menu transitions. Fidelity/performance observations and follow-up priorities are below.
- Dev hooks: `PROCYON_NIS_SHOT=<png>` (screenshot in the third script NIS), `PROCYON_END_SHOT=<png>` (screenshot after the mission ends).
- Earlier checks (opening briefing, speed dial, reference-video comparison) are described in docs/reference-video.md.

## Next

Prioritise the owner-observed Mission 1 polish issues before new mission support:

1. **Late-mission FPS drop — reproduced and addressed:** the non-fading, continuously emitted docking-marker flare was given
   infinite particle lifetime. Near the ending it accumulated ~261k particles, with ~45 ms real frames and ~25 ms FX mesh-building
   CPU time. A provisional authored-duration fallback now keeps the equivalent scene near ~8k particles, ~16.7 ms frames and
   ~0.42 ms FX CPU time on the same machine. The earlier corrected autopilot run reached the ending; it used autokill, not normal combat.
   The earlier aggregate particle test was too permissive and has been tightened, alongside a five-minute marker regression.
   See `effects.md`, "Mission 1 particle-growth profiling". Confirm in owner play; native missing-expiry semantics remain unresolved.
2. **Torpedoes moving away from targets — guidance implemented:** after the native 0.5-second launch coast, torpedoes steer
   toward the authored homing point, lead inside 100 units, and adjust altitude. Authored bullet physics drives propulsion/turning;
   missing/destroyed targets stop tracking. A crossing-target test intersects the actual schooner hull, and the GPU Mission 1
   run logs moving training-fleet torpedo impacts. Launch/flame alignment fixes are retained. Exact native integrator, tracking
   invalidation timer, arming and point defence remain provisional/unimplemented (`gunnery.md`).
3. **Repair scaffold animation — implemented:** six converted mesh endpoints feed the EFT deploy/hold/retract curves, including
   ironclad variants. GPU docking smoke visibly deploys the scaffold (`local/feedback-scaffold.png`). Geometry/curve regressions
   cover all five available morph effects; one effect at ship centre remains an approximation of per-section native repair jobs.
4. **Towing — independent physics implemented:** rigid teleport/heading copying removed. Tower stern and towee bow nodes attach
   a mass-weighted slack cable in movement substeps, transferring linear force and off-axis torque to both ships. Tow approach
   uses the nodes; script/menu break-tow relationships are preserved. Tests cover pulling, independent turning and release.
   Cable length is the registration distance and the constraint is inelastic, not recovered native spring physics.
5. **Dim world nebula — blend corrected:** native ONE/ONE blending no longer multiplies the cloud RGB by texture/common alpha;
   edge fading is applied to RGB. Star-map art is unchanged. Exact edge fade and native cloud morphing still require comparison.
6. **Dim low-angle engine trail — blend/motion corrected:** additive particle RGB no longer gets Bevy's additional texture-alpha
   multiplication. Wakes draw a live head between distance samples and no longer apply a guessed 0.7 brightness factor.
   Existing world-axis flame alignment is retained. Flare textures keep the prior mask pending native docking-flare pooling/scaling;
   a global blend switch produced an unacceptable overbright docking marker during smoke testing and was narrowed accordingly.
7. **Collision weight — angular response implemented:** finite-mass asteroid and unequal-mass ship response now include
   contact-point velocity, lever-arm torque and angular effective mass; centred hits retain the existing mass-ratio regressions.
   Off-centre tests verify spin, momentum and non-increasing energy. Geometry is authored, but box inertia, contact manifold,
   zero restitution and lateral damping remain approximations; retail/manual feel still needs comparison (`collision.md`).

This is one remediation pass over the owner's feedback, not a claim of exact retail equivalence. The earlier docking UI/audio,
FX interpolation and bounded-particle fixes remain included. Acceptance still requires the owner's normal-combat replay.

Latest validation: `tools/check.sh` passes 98 Rust and 61 Python tests (six Python skips). GPU smoke shows the repair scaffold,
corrected world nebula, oblique torpedo effects, moving training-fleet torpedo hits, defeat and restart without recorded runtime
panics. The long run (`local/feedback-complete.log`) stayed around 16.7 ms / 7–8k particles through mission time ~1259 s, but the
window closed before mission end while the player remained on the tender approach. **The final pirate/navy tow and ending are
not revalidated by this run.** The separate dock smoke completes docking/repair; do not present the interrupted full run as a win.

First polish batch completed: primary-ship docking animation moved to the bottom-bar portrait; docking route/waypoint art and
automatic speed/movement acknowledgements suppressed. Particle rendering now samples between 30 Hz ticks and uses current
propagated transforms; catch-up emission is spread over the emitter's motion. The owner reports that FX look better in a partial
replay; smoothness across all camera angles/effects remains unverified. See [effects.md](effects.md) and [docking-and-orders.md](docking-and-orders.md).

After these: nebula/whale fidelity, tender refit/torpedo resupply, remaining asteroid fragment/collision
fidelity, native NIS camera and AI stance behaviour, then validate Mission 2. Missions 2–13 already export/load but remain untested in play.

## Mission 2 owner-playtest fixes: islands and gun towers

Expected retail-rule sequence: entering `SmugglerCamp` starts the discovery NIS and M02ONYX04; entering the smaller
`SmugglersCampAttack` volume changes the island owner to `SmugglerCampEnemy`. The first tower shot starts OnyxFirePause;
M02ONYX05/JIM03 then activate Destroy Gun Towers. Destroying both `vitalToMission` tower sections advances to surrender and docking.
Discovery alone does not start combat: the ship must also enter the attack volume.

Fixed from owner feedback:
- Base docking no longer disables island contact for the whole approach. It plans and keeps a scenery-clear detour to a fixed
  approach point; only the final short docking leg allows contact with the dockee. The planner uses conservative swept hull
  boxes and scenery-bound corners, not recovered native navigation. No route means stop, not pass through land.
- `Set Group/Unit Owner` now updates `MissionBase` as well as ships/animals. The previous ship-only dispatcher left the camp
  neutral, so it never fired and the subsequent first-shot dialogue condition never became true.
- Base gun-node positions are already in the MDB mesh frame. `export_combat.py` now uses those positions instead of adding
  a tower offset and subtracting a different section's offset. Corrected gun positions for the eight affected base classes;
  retained all committed HP, weapon/economy definitions and other combat values rather than importing the modded install's changes.
- Hostile live mission sections can now be clicked with an attack cursor and targeted by ship guns. Aiming selects the clicked
  live tower, then another surviving mission section if it is destroyed; neutral/surrendered bases remain non-hostile.

Validation: `tools/check.sh` passed (186 Rust / 73 Python, seven skips); the local base-mount exporter regression also passed separately with the install available. Regression sails the actual Mission 2 torpedo ship from
behind the abandoned island around its collision geometry and into the dock; additional tests cover blocked/unreachable routes,
actual attack-volume owner dispatch and first-shot timers, mount placement within tower geometry, live/dead tower picking and
player return fire. Classic GPU smoke, using an ignored scratch mod to put the player inside the attack volume, logged discovery,
attack and first-shot rules. `local/m2-towers-final.png` shows the discovery NIS; `local/m2-tower-placement.png` shows guns on both
towers (a second isolated view with NIS rules removed). These focused runs were stopped after screenshot capture, not played to victory.
Full Mission 2 completion, all approach directions, other bases' visuals and native path-planner parity remain unvalidated.

Follow-up owner feedback: island heights and ineffective return fire. The first fix subtracted two primary `_Layer1` pivots
(AbandonedCamp Z=-17.2, SmugglersCamp Z=27). That matched those scenes' dock heights but was not a general assembly rule.
The subsequent Norville/Safetytown report exposed the difference: its main island mesh pivot is Z=39, but both converted docks
are Z=9. Subtracting 39 would put docks 30 units below flight height. The primary mesh pivot is not a reliable flight-plane datum.

The shared `scenario::scenery_transform` now anchors **the first converted dock's local height** to the map object's authored
altitude, for all dockable scenery on every map. It moves meshes, guns, docks and colliders together; horizontal positions,
relative dock heights and non-zero map altitudes are preserved. Scenery without docks is unchanged. Norville moves down 9 units;
the previous two island corrections remain +17.2 / -27. The two-scene whitelist/exported overrides and their obsolete parser
regression were removed. No converted models, dock data, authored placements or installation/mod combat values were regenerated.
Explicit `scene_origin` remains an optional mod override, taking precedence over the automatic dock datum.

This is still **PROVISIONAL**, not executable-confirmed native assembly: aligning the functional dock plane fixes the reported
placement fault but does not establish every scene's retail assembly, hierarchy or animated-prop placement. The catalogue test
checks the shared placement invariant, not the correctness of existing converted dock coordinates; those still include the
provisional parent-chain/common-offset interpretation and must be checked against their local meshes when playtesting. Do not flatten all
island geometry or use the first dock's horizontal coordinates as a pivot.

Validation of the generalization: 194 Rust / 74 Python tests, seven skips. Regressions cover all three Mission 2 bases' docks,
the first-dock datum across the shipped map/base catalogue, preserved map altitude/horizontal coordinates/relative dock heights,
explicit overrides, and actual Coventry-class docking and repair at Norville through the collision/movement systems.
Classic isolated screenshot `local/norville-height.png` shows the corrected island and Coventry approach after issuing the
mission's real dock order; the scratch mod skips the startup NIS and positions ships near Norville. It is not a full SOS/tow replay.

Owner clarification: in retail, only the torpedo boat's front Lancer could target these towers, not its torpedo launchers.
The attempted base-homing addition has therefore been removed. Torpedo guns now reject base targets before aiming/firing;
ship/asteroid/animal guidance remains unchanged. This restriction follows owner recollection, not executable-confirmed target masks.
The earlier launch-only Cutter regression has been replaced with the actual Mission 2 torpedo boat, propagated muzzle transforms
and real shell/section-damage systems: from 100 units in front of each tower, the front Lancer alone must destroy both 100-HP
mission sections, and every player projectile must be a Lancer shot (no torpedoes). The player is invulnerable in this isolated
test to keep the target exercise independent of enemy damage; no gun damage, accuracy, range or tower HP is boosted.
The Lancer's maximum range is 135. This is an attack/hit/damage regression, not normal-combat survival or mission completion.

Follow-up gate: 188 Rust / 74 Python tests, seven skips. The abandoned-island docking regression still passes at the corrected
height. Classic isolated screenshots: `local/m2-island-height.png` (smuggler island and guns at corrected height),
`local/meteor-flight-plane.png` (rain above the flight-plane splashes; see effects.md). Runs stopped after capture.

### Mission 2 SOS rocket pickup

After the camps, the first SOS beacon at `Schooner1SOS` is `BottleRocket_Light_Impact`, not a clickable ship/object.
Entering `BottleRocketOnePickedUp` with `PlayerHasDockedWithCamp` set picks it up: Onyx speaks, the effect is released,
the task/objective clear, Coventry becomes visible and damaged, and the pickup sound plays. The second rocket uses
`Schooner2SOS` / `BottleRocketPickedUp2`, gated by `NotTowed`, followed by Lady J/whale story reveals.
The pickup volumes contain their effect point; sailing to the objective is enough, with no mouse interaction required.

The invisible beacon was expiring after one 2.5-second animation cycle, not missing its textures. Loop metadata and the
fixed start-particle pool are now preserved ([effects.md](effects.md#mission-2-sos-bottle-rocket-beacon)). Both original
pickup rules have isolated regressions for volume entry, effect-release action, sound and progression flags.
Validation: 191 Rust / 75 Python tests, seven skips, and Classic isolated beacon screenshot `local/m2-sos-visible.png`.
Normal-timeline pickup/rescue/towing continuation still needs owner playtesting.

## Missions 2-13 (first pass, untested in play)
`--mission N` loads `maps/missionNN/mission.json` through the same rule interpreter; only mission 1 has the scripted opening NIS (`briefing.rs`).
Rules call the human group "Player0 Group"; `Script::new` rewrites it to the TWT's player name. A mission with no primary ship (9, 13)
gets the player group's first ship, else a stand-in cutter. Hulls without a class (`*_Wrecked`) are skipped.

Primary-ship handoffs use both `Set Fleet Primary Ship` and `Set Group/Unit AI Captain` with `Captain_Human`.
The latter previously only set the human-control flag, leaving mission 3 on the torpedo boat instead of RLS Endurance.
Both now use the shared fleet promotion path (selection, HUD/control identity, camera and fleet leader).
Regression coverage uses the actual captain actions from missions 3, 5 and 9 (longboat and return to RLS St Roch);
`--mission 3 --script fleet-primary` isolates mission 3's real setup rule and asserts exactly one primary, RLS Endurance.
This scripted ECS smoke passes; later normal-timeline handoffs still need playtesting.

Mission 3 storm/hunt sequencing: the invisible Exile is placed inside `Sloop Trigger Hunt`.
Counting hidden ships in spatial conditions fired that rule at world initialization and sent the hunters out before the storm ride.
Volume and distance conditions now ignore hidden/destroyed units, and visibility actions update the interpreter's snapshot immediately
so later rules in the same tick see the world-init hiding. Nebula-entry conditions now use the moving storm's polygon offset,
matching the existing storm hazard/carry code rather than testing the original stationary polygon.
A second bug remained: revealing Exile still counted its initial placement inside the hunt volume as entry. `Enter Volume` now
tracks physical membership even while hidden and arms a run-once trigger after observing the group outside (or initially visible inside);
reveal alone cannot launch the hunt. The real rules order Exile to patrol after NIS 2, retreat on spotting, then launch the fleet when it
returns through the hunt volume. Pirate Silver holds fire until that return. Persistent routes now take precedence over combat steering,
including the small ships' scripted swoop before their aggressive-stance switch.
NIS motion is **provisional**: ordinary helm/combat motion halts (including lateral drift), with orders retained for resumption;
authored ship routes and external storm carriage continue. This is not a global world pause: the spotting NIS explicitly orders Exile's
retreat, and Mission 4's opening explicitly routes the player's ships. Mission 1's separately driven opening remains unchanged.
`--mission 3 --script hunt-m3 --speed 16 --shot-frame 140 --late-frame 0` isolates the four post-storm rules, checks reveal without hunt,
spotting/retreat without engagement and NIS helm halt, then teleports Exile into the return volume to check fleet launch.
This is endpoint/wiring coverage, not validation of the complete retreat through scenery or the subsequent battle.
`--mission 3 --script storm-m3 --speed 16 --shot-frame 1050 --late-frame 0` isolates the three retail storm rules,
places an invulnerable primary at the trigger, and asserts storm entry, northward carriage and release at `End of Nebula Ride`.
This ECS smoke passes (NIS 2 starts at ~21 s, release at ~36 s); it does not validate combat damage or a full mission playthrough.
Weather hit selection was subsequently corrected using executable evidence: storm-wide random lightning bolts must intersect a ship,
meteors roll ship diameter / 80 and select a section, and wind excludes the storm centre. The damage-enabled smoke now leaves the sloop
with 70.75/80 sail HP versus the previous 31.64/80; full normal-play validation and remaining timing/force parity are still open
([nebula-landmark.md](nebula-landmark.md#weather-hit-selection-follow-up)).

Retail data now used (static review of all 13 rule lists, `rules/tests.rs` `later_missions_only_warn_about_known_missing_features`):
- **Named groups.** Rules mostly address TWT `Group` blocks (world object ids), not players: 'Pirate Barque & Sloop', 'SpaceWhales',
  'Asteroids1', 'Weapons Tender'... Exported as `groups`; a unit answers to its current owner (player) or any TWT group it was placed in
  (`eval::matches`). An explicit TWT group takes precedence over a same-named player; otherwise selectors follow current ownership
  (including transfers into 'Ships Left Behind' in 7/8 and 'Players Ships' in 11). Mission 3's 'Pirate Silver' group contains only Argentum,
  although all pirates belong to that player. The former owner/group union sent Wylie and RockLobster out with the main hunt and big-ship
  attack orders. They now remain dummy until 'Pirates On Intercept Course' fires at 'Player Triggers Chase Group'. Tests cover the real
  hunt/big-ship orders and intercept trigger, including no owner fallback when a named group's ship is absent. The third reserve group,
  'Punishers' (422–424), is also excluded from those orders and stays hidden/dummy during normal progression. Its real 'Player Needs
  Punishment' rule intentionally reveals and launches it on entering the chase volume with 'Player Obeying Mission Flow' false
  (before 'Start NIS 4' sets that flag). Regression coverage checks both branches; the hunt ECS smoke checks normal-route inactivity.
  This replaced mission 1's hard-coded 'Weapons Tender' / 'Navy Tender in Distress' / 'Whales' lookups.
- **Hostility.** The TWT `PlayerAllianceInfoVector` lists allied player pairs; every other pair of players starts hostile, owners that are
  not players ('NO PLAYER', unset) never are. Replaces the old pirate-hull guess (mission 1's table gives the same result, incl. whales).
- **Speech list.** Rules name an event of the TWT `Speech Event List`; its sound file, subtitle string id and FaceTexture may differ from
  the event name (mission 3+ use names like 'Opening Monologue'; mission 1's M01AMELIA13A plays M01AMELIA13b, so it has audio after all).
  Exported per event as media `file`/`face`; the talking head uses `face`.
- **Setup Ship** stance (137 placements are Stance_Dummy) and placement path (mission 13) are applied at world init.
- **NIS Focus camera on Point** names a point set (its first point), not a path.

Added: Exit Volume, Group Destroyed/Under Attack, Fired X Shots, vital-section (as overall damage), Does Group Contain Unit, Is Ship In Tow,
Set Group Throttle Percent, Stop Timer, Destroy, Teleport, Transfer, Ram (as attack), Task Failed, All Units entered volume once,
Set Current Objective Point On Ship, NIS Focus on Main Ship, NIS Position Camera Relative to Object (PROVISIONAL: as Attach Camera),
Cloak State (PROVISIONAL: hidden and untargetable).
Also: Unit is Within any Nebula (PROVISIONAL: any 'Setup Nebula' polygon counts, nebula.rs `polygons`), Set Ship Name (the
retail string from the install's strings, exported as `ship_names` in the media JSON; sets the banner label only), Set Ship Top
Speed Percentage (PROVISIONAL: same cap as max throttle), and the 'Goto Next Level' victory line ("Mission N complete.", N = the
mission just won, from its World name). Silently ignored presentation/leash actions: NIS Zoom, NIS gun accuracy, NIS object visibility,
nebula fade-out/lock, warning-shot mode, border-zone return points.

Still missing (warned at runtime, ignored), with the missions they block:
- (done) lifeboats: `Set Lifeboat Creation State`, `Player has no lifeboats` and the lifeboat-only `Player Vs Player capture count` are implemented
  (`lifeboats.rs`, `docs/re/lifeboats.md` §9); boarding is separate (`docs/re/boarding.md`: missions 6, 9-12 capture/defence), with AI boarding
  limited to boardable bases. Mission 1 and 2 pickup/fallback paths are exercised by scripted smoke only, not yet by a normal-combat replay.
- mission 9 specials (`If Mortar explodes within Area`, Dark Matter explosion, Teleport Longboat): mission 9's main objective.
- dragons (`Dragon - Set AI Stance/Damage Threshold`; dragons are not spawned): missions 6-8 dragon encounters.
- (done, provisional) Etherium currents (`*State Init* Setup Etherium Current`, missions 3, 4, 10): planar push plus two flowing textured river ribbons
  on each available path; one mission 10 setup references missing `ebend`. Weather damage now uses the separate retail nebula tables instead of weapon bullets
  ([nebula-landmark.md](nebula-landmark.md#etherium-currents)); recharge/force/visual tuning remains provisional.
- HUD texture overlays (mission 13 credits), formations, ship AI captain, wrecks as scenery, crew speech toggles.

## Talking head (bottom-right device)
Implemented in `panels.rs::talking_head` (`HeadState`, `Talk`, `Helmsman`; retail art in `classic/panels.rs::head_art`); hidden with the HUD during cutscenes/NIS.
Findings (original game behaviour + `hud.json`):
- Two retail windows: the docked one (`TalkingHead_Portal_*`, wooden housing `TalkingHead_Solid_Background` 126x122 with a transparent
  80x80 ring) and a floating one (`TalkingHead_FloatingPortal_*`, 151 px; also the cutscene banner). The `TalkingHeadLocation` option picks
  the side; offsets `Left_/Right_TalkingHead_Character_/PortalOverlay_From_TopLeft` place portrait and iris (Navy right: 7,11).
- Layers: character sprite `TalkingHead_Character_<name>` behind, iris overlay (`Portal_Closed`, `ClosedToOpen0..9`, `Portal_Open`), housing on
  top, `Portal_Light_On` over the lamp, `Portal_VoiceBox_*` (55x9) in the name plate (Off when silent, Start..Stop frames cycled while speaking;
  the exe returns `Off` unless its speaking flag is set). `Animation Speed TalkingHeadWindow` = 2.0.
- Sounds on open/close: `HUD_TalkingHead_Iris_Open/Close` (docked), `HUD_TalkingHead_Floating_Open/Close` (floating).
- Helmsman uses `TalkingHead_Character_1stMate`; campaign lines use the speech event's `face`.
- Sprite ids are indices into the exe's name table (8-byte stride); the animators are (first, last, current) triples.
PROVISIONAL: bottom-right corner placement and always-visible closed housing; open/close rate (2.0/s), voice-box cadence (8 fps),
light and voice-box positions (measured on the art), which side the real game defaults to. Not done: the floating variant, the
`TalkingHeadLocation` option, per-line portrait expression beyond the data's `face`, moving the gameplay subtitle onto the speech-event bar.

## Island gun towers
Bases whose WOT places guns (`Base_SmugglersCamp`, `Base_Grant`, ...) export as `assets/ships/Base_*/` (`tools/export_bases.py`, no hull model; the scenery GLB is drawn) and become `combat::ArmedBase` + `Health` + guns when a rule sets them up. `combat::base_fire` shoots the nearest visible ship the base owner is hostile to (neutral owners such as mission 2's "NO PLAYER" camp stay quiet until a rule changes the owner); shells damage the base sections, and the rules' "Vital Section damage" reads the mean damage of the `vitalToMission` sections (PROVISIONAL). Not done: AI stance / attack orders for bases, tower collapse visuals, fires on bases.
