# Fidelity backlog

Open work toward a faithful single-player recreation. Specs and evidence live in `docs/re/`; this file only tracks status.
Priority (AGENTS.md): in-game UI/controls, then Campaign Mission 1 end to end. Multiplayer is out of scope for now.

**Status:** Missing (nothing usable end to end) · Partial (an important path is missing) · Provisional (works, constants or
behaviour not from original evidence) · Unvalidated (implemented, not compared with retail or play) · Unknown (needs inventory).

**Closing an item:** original evidence → own-words spec in `docs/re/` → shared implementation → regression test → runtime scenario
(plus a reference comparison for visuals). Keep IDs stable; when a row closes, move it to *Done* with its check.

## 1. Mission 1

| ID | Status | Open work | Code; spec |
|---|---|---|---|
| M1-01 | Unvalidated | Owner completed Mission 1 manually. Replay in normal combat after recent changes: tender repair, asteroids, pirates, navy tow, ending; check late-run frame time and particle counts stay bounded. | `scenario.rs`, `docking.rs`, `fx.rs`; [campaign.md](re/campaign.md#next) |
| M1-02 | Unvalidated | Defeat, restart, leave and re-enter: camera, pause, HUD, entities, audio, campaign counters. | `scenario.rs`, `menu.rs`, `campaign.rs` |
| M1-03 | Provisional | Refit and lifeboats implemented per spec (scripted smoke `lifeboat-m1`, `lifeboat-m1-fallback`). Open: normal-combat replay, NIS speech timing of the early exit, lifeboat look/banner, crew-skill scaling of damage control. | `docking.rs`, `lifeboats.rs`; [refit-resupply.md](re/refit-resupply.md), [lifeboats.md](re/lifeboats.md) |

## 2. Controls and HUD

| ID | Status | Open work | Code; spec |
|---|---|---|---|
| UI-01 | Provisional | Selection / command ship / focus implemented. Open: default keys for primary and slot 10, selection fallback on destruction/capture, cross-group script promotion. | `bindings.rs`, `view.rs`, `ship.rs`; [fleet-controls.md](re/fleet-controls.md) |
| UI-02 | Provisional | Ship bar functional. Open: retail slide animation, overflow, status icons and art metrics. | `panels.rs`, `classic/panels.rs`; [fleet-controls.md §2](re/fleet-controls.md) |
| UI-03 | Provisional | All eight formations, hold/release, Fleet Commands panel. Open: formation size, slot assignment ties, follow-controller constants, long-run convergence. | `ship.rs`, `scenario.rs`, `panels.rs`; [fleet-controls.md §3](re/fleet-controls.md) |
| UI-04 | Provisional | Advanced movement (plot, accept/cancel). Open: button placement and keys are our reading of the manual. | `course.rs`, `view.rs`; [hud.md](re/hud.md) |
| UI-05 | Provisional | Hold position and cloak. Open: cloak timings. | `cloak.rs`, `ship.rs`; [cloaking.md](re/cloaking.md) |
| UI-06 | Provisional | All bindings have a consumer (screenshot, focus projectile, attack-only). Open: retail dispatch/click effects of attack-only. | `view.rs` |
| UI-07 | Provisional | Retail comparison of gauge meanings, radar range/contact animation, faction placement, cursors, tooltips, target/nav marker geometry. | `panels.rs`, `classic/world_hud.rs`; [hud.md](re/hud.md), [starmap.md](re/starmap.md) |
| PT-06 | Provisional | Wake and dotted course line need a visual pass against the original (retail: chunky dashed yellow tiles). | `wake.rs`, `course.rs` |
| PT-07 | Missing | Lob aiming cursor: ground target marker, firing-solution feedback, lob accept/cancel/reject sounds (art and sounds exported, unused). | `view.rs`, `classic/world_hud.rs` |
| HUD-01 | Missing | Selection indicator arrow/compass models (exported, unused). | `classic/world_hud.rs` |

## 3. Simulation

| ID | Status | Open work | Code; spec |
|---|---|---|---|
| SIM-01 | Partial | Cutter movement and crew scaling measured on retail; Man-O-War speeds and turn rates match the same formula. Open: Green/Average/Elite crews, weave while steering for a point, no lean at Emergency, retail course/rudder controller. | `ship.rs`, `course.rs`; [movement.md](re/movement.md) |
| SIM-02 | Provisional | Obstacle avoidance/path planning, stance and captain behaviour, target choice, border-zone AI. Steer-away fails on concave coasts. | `ship.rs`, `collision.rs`; [movement.md](re/movement.md) |
| PT-09 | Provisional | AI: retail run-in, circling (per-ship radius), hazard keep-out (black holes, storms), crew speed/rudder/scan factors and map exploration measured and implemented ([movement.md](re/movement.md#computer-captains-measured-2026-10)). AI target choice implemented (nearest in sight, re-chosen every 6 s). AI grapple-and-board implemented ([boarding.md](re/boarding.md) §5; harpoon reach PROVISIONAL). AI repair docking implemented ([docking-and-orders.md](re/docking-and-orders.md#computer-ships-docking-for-repair)). Mine laying implemented ([gunnery.md](re/gunnery.md#mine-laying), thresholds PROVISIONAL). Circle mechanism and helm rule implemented (aim point at the gun banks' mean long range beside the target, rudder = error / 20°; computer rudder × navigator factor²). Open: the run-in/circle throttle switch; ships that head straight for the enemy instead of exploring; Assault Cutter frontal attack and ramming. |
| SIM-03 | Partial | One retail ram measured (contact wider than the physics box, idle heavy ship unmoved, damage on every section of both ships; [collision.md](re/collision.md#measured-on-retail-2026-10-one-glancing-ram)). Open: ram damage formula; restitution, inertia, damping and docking exemptions guessed; rock/rock, rock/static, whale/mine contacts absent; no continuous contact for fast belts. | `collision.rs`, `combat/asteroid.rs`; [collision.md](re/collision.md) |
| SIM-04 | Provisional | Retail dock arrival/path planner, per-section repair jobs, tow approach and cable spring/damping. | `docking.rs`; [docking-and-orders.md](re/docking-and-orders.md) |
| SIM-05 | Partial | Boarding leadership/per-person crews, harpoon flight, winch tension/break, player boarding of bases. | `boarding.rs`; [boarding.md](re/boarding.md) |
| SIM-06 | Partial | Crew & Arms functional. Open: captain/leadership bonuses, riggers' repair share, generic crews, retail unlock/upgrade rules, transfer to replacement hulls, retail layout; owner acceptance. | `crew_arms.rs`, `campaign.rs`; [crew.md](re/crew.md), [scoring-saves.md](re/scoring-saves.md) |
| SIM-07 | Provisional | Lob guns, mines, point defence and shell gravity done. Open: star mortar blast look, lob elevation scatter (44-50° measured), torpedo arming, lob/well/mine constants, ricochet formula. | `combat/`; [gunnery.md](re/gunnery.md) |
| SIM-08 | Provisional | Fire-fighting, critical-hit feedback, section destruction/movement loss; replay Iron War as Procyon for the destroyed-section aim fix; damaged-mesh collision and pick silhouettes. | `combat/damage.rs`; [effects.md](re/effects.md) |
| SIM-09 | Provisional | Storms, currents, meteor showers implemented. Black hole: third class measured, the pull is an acceleration masked by the rest snap (effects.md); remake still moves positions at a constant creep. Open: the acceleration law vs distance and class, destroy distance per class, its look; inside-cloud overlay, exact rates, lightning endpoint sampling/forces. | `environment.rs`, `nebula.rs`, `blackhole.rs`; [nebula-landmark.md](re/nebula-landmark.md), [effects.md](re/effects.md) |
| SIM-10 | Partial | Whale and dragon skeletal animation, whale damage, asteroid fragment/contact fidelity. | `dragons.rs`, `environment.rs`; [dragons.md](re/dragons.md), [asteroids.md](re/asteroids.md) |
| SIM-11 | Partial | Sensing implemented. Open: retail mesh hiding/contact memory, cloak/NIS/range-modifier semantics, scenery UI distance, moving storms/overlapping clouds, dragon/projectile acquisition. | `environment.rs`, `panels.rs`; [visibility.md](re/visibility.md) |
| SIM-12 | Partial | Scenery height uses a first-dock datum workaround until MDB/MSB pivot/pose semantics are known; visual comparison and mission/NIS replay after the orientation fix. | `scenario.rs`, `tools/mdb_to_glb.py`; [scenery-placement.md](re/scenery-placement.md) |
| PT-03 | Partial | Procyon submersible dive/surface look: find which effects/rules the hull uses. | `fx.rs`, `cloak.rs` |
| PT-04 | Unknown | Procyon on an open skirmish map spawned at the Navy base: check whether retail start position follows faction or slot. | `scenario.rs`, `menu.rs` |

## 4. Campaign and front end

| ID | Status | Open work | Code; spec |
|---|---|---|---|
| CAM-01 | Unvalidated | Missions 2-13: play each, including failure/alternate paths, and fix what blocks it. Mission 2 owner playtest underway. | `campaign.rs`, `rules/`; [campaign.md](re/campaign.md#missions-2-13-first-pass-untested-in-play) |
| CAM-02 | Provisional | Rule semantics: vital-section condition, top-speed-as-throttle cap, group/owner matching, event/condition timing, readings of the AUD-05 actions, NIS object visibility (stored only). | `rules/`; [campaign.md](re/campaign.md) |
| CAM-03 | Partial | Mid-mission save/load: saves restart the mission; no snapshot of ships, projectiles, AI, timers, rule progress. | `campaign.rs`; [scoring-saves.md](re/scoring-saves.md) |
| CAM-04 | Partial | Difficulty effects beyond damage, retail stats/outfitting layout, economy calibration. | `stats.rs`, `crew_arms.rs`; [scoring-saves.md](re/scoring-saves.md) |
| FE-01 | Provisional | Profiles; inventory of other retail front-end paths; `fe_*` front-end sounds unwired. | `menu.rs`, `classic/menu.rs` |
| FE-02 | Partial | Skirmish arms: victory-point budget/Auto Assign for weapons, crew/fleet load-save, AI fleet construction; owner acceptance. | `menu.rs`; [skirmish-setup.md](re/skirmish-setup.md) |
| INV-01 | Unknown | Complete inventory of ship/weapon variants, scenery gameplay, alternate campaign branches. An exported asset is not runtime support. | manual, TST/TWT/WOT, exporters |

### Front end against retail screenshots

Reference screenshots are local only (`local/reference/`, never committed).

| ID | Status | Open work |
|---|---|---|
| UIR-01 | Partial | World labels and star map still use vector fonts, not the retail glyph sheets. |
| UIR-05 | Partial | Ships screen: ship-name drop-down, Load/Save Fleet, slot-bar scrolling. |
| UIR-06 | Partial | Crew/Arms: skirmish crew roster, drag and drop, Load/Save, Auto Assign, stat-bar post mapping ([crew.md](re/crew.md)). |
| UIR-08 | Partial | Game Summary: retail VICTORY! matches the layout (one page, green bar per column's best, winner's row first; rows now ordered by ships left). Open: panel art, flag choice, whether campaign summaries add a kills-by-type table. |
| UIR-09 | Unvalidated | Battle HUD vs `battle_hud.png` and the 2026-10-15 references (player list, speech box, talking head, weapon bar). |
| UIR-10 | Partial | Ship selection: "N victory points remaining" dialog (Auto Assign / Cancel / Continue). |

## 5. Audio and visuals

| ID | Status | Open work | Code; spec |
|---|---|---|---|
| AV-01 | Missing | Pre-rendered campaign/ending movies with skip/transition (needs a video dependency: ask first). | `menu.rs`, `campaign.rs`; [movies.md](re/movies.md) |
| AV-02 | Provisional | Retail NIS spline/acceleration/focus transitions, later-mission openings, portrait expressions, speech cadence. | `briefing.rs`, `rules/`; [campaign.md](re/campaign.md) |
| AV-03 | Partial | Scenario-based visual pass: mesh-shifting/skeletal effects, emitters, wake/sail/sky geometry, blend rules. Decals: trigger (per hit vs threshold), placement, per-ship cap. | `fx.rs`, `wake.rs`, `decals.rs`; [effects.md](re/effects.md) |
| AV-04 | Partial | Faction voices (pirate generic crew), spatial mix/falloff; crew callouts for engine/rudder hits, boarding defence, retreat. | `audio.rs`, `crew.rs`; [sound.md](re/sound.md) |
| AV-05 | Provisional | Ship destruction now follows the retail breakup (class explosion, per-section `destructFx` chunks and bursts; measured on one retail Cutter death). Sections destroyed mid-fight drop their `debrisMeshScene` pieces. Open: breakup piece tumbling, retail debris drift/fall/spin numbers, owner visual check. | `combat/mod.rs`; [effects.md](re/effects.md#ship-sinking-and-final-destruction) |
| PT-01 | Unvalidated | Crew lines open the talking head with the speaker's portrait; check whether every callout does in retail. | `audio.rs`, `briefing.rs` |
| PT-02 | Unvalidated | Crew lines use the post's crew species or the faction crew; pirate generic voice, retail check. | `audio.rs`; [sound.md](re/sound.md) |
| PT-05 | Unvalidated | Additive effects sort after alpha meshes; a trail behind a translucent sail shows undimmed (per-emitter sort would fix). Owner visual check. | `fx.rs` |
| PT-11 | Unvalidated | Dock/repair/SOS flares now draw; retail look not compared. | `fx.rs`, `docking.rs` |

## Next retail measurements

1. Which gun reads which gunner slot; Green/Average/Elite crews (SIM-01/06).
2. Black hole: drift vs class (a third class), what starts the Cutter's plunge, destroy radius per class (SIM-09).
3. Star mortar ring timing/push, lob elevation scatter (SIM-07).
4. Victory screen and kills-by-object-type summary (UIR-08); ram/collision damage (SIM-03).
5. More Easy-difficulty hits on the computer.
6. AI: boarding trigger (Barque vs idle Cutter: none; vs Man-O-War: three grapple passes and capture), docking decision at low hull, target choice with two of our ships (PT-09).

## Done

Closed rows, kept for their IDs. Each still carries PROVISIONAL constants noted in its spec.

| ID | What landed | Check |
|---|---|---|
| PT-08 | Command ship's direct-fire guns engage only a clicked target or panic fire: confirmed on retail (idle ship, enemy at 40 u, 0 damage inflicted). | [gunnery.md](re/gunnery.md) measurement |
| PT-10 | Every map object/ship with dock points is dockable (skirmish bases by every side, PROVISIONAL). | `scenario::rotation_tests::skirmish_bases_and_tenders_are_dockable` |
| UIR-02/03/04 | Front-end frame, Skirmish and Game Lobby screens match retail screenshots. | side-by-side screenshots |
| UIR-07 | In-game menu Summary opens the Game Summary page. | runtime |
| AUD-05 | Every rule action/condition used by the 13 missions and tutorials is interpreted (known-warning list in `rules/tests.rs` empty). Re-audit with `tools/audit_assets.py`, `tools/audit_rules.py`; results are leads, not proof. | `tools/check.sh`, mission start-up smoke |
