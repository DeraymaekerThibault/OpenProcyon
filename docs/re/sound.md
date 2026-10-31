# Sound

Retail sound is Ogg Vorbis under `Sound/` (`SoundEffects`, `GUI`, `Music`, `Dialogue`, plus `_LQ` low-quality copies).
`tools/build_assets.sh` copies `SoundEffects`, `GUI` and one music track to `local/sound/` with lowercase names, because
WOT names differ in case from the files (`Engine_NAVY_FULL` vs `Engine_Navy_Full.ogg`). Played by `game/src/audio.rs`.

## Data-driven (WOT, exported by `tools/export_combat.py`)
- Gun: `SoundName`, `SoundVolume`, `SoundMaxDistance` (fire sound).
- Bullet: `ImpactSounds<surface>` blocks of 5 slots (empty slots unused) per surface `Hull Wood`, `Hull ReInforced Wood`,
  `Hull Iron`, `Sail Cloth`, `Wall Stone`, `Dragon Scale`; `Impact Sound Volume`, `Impact Sound Max Distance`.
  Also `RicochetSound: <surface>` and `Travel Sound` (ricochet sounds play on a ricochet, `docs/re/gunnery.md`; travel sounds mostly empty).
- Ship: `EngineSoundName Half|Full|Emergency` (looping engine sound per engine order).
- Music: each skirmish map's TWT script plays one track ("Play Music Track", `File Name String`), e.g. Red Rover
  `BTL_NavyBig_Full02`, Ambush `BTL_DeadlyPirate_Full02`, Bayles Stand `BTL_ProcWarDrums_Full`.

## Built into the original (not in the data files)
The original game has built-in sound-name tables that the world setup turns
into random pools. Six material groups in order Wood, Reinforced Wood, Iron, Cloth, Stone, Dragon (empty), each 10 slots:
- damage pool `Damage_Wood01-05` (Wood and Reinforced), `Damage_Metal01-05`, `Damage_Sail01-05`, `Damage_Stone01-05`; float 600 stored with it
- collision pool `Ship_Collision_Wood01-05` (Wood, Reinforced), `Ship_Collision_Metal01-05`; float 200
- `Explosion_Heavy01-08` pool, float 8000; `Explosion_Med01` pool, float 6000.
The float is read as the pool's max distance (same field order as the WOT pools). Interface sounds such as
`HUD_SpeedControl_Up/Down` and `HUD_FireAll` are also original-global names.

## PROVISIONAL in the remake
- Which event uses which original pool. Measured (27-minute retail recording, 2026-10-31): a ship's death plays one file of the
  `Explosion_Heavy` pool, about 2.3 s before its wreck stops moving (4 of 9 deaths matched, each file heard almost nowhere else).
  `Explosion_Med01` plays at the same moment in 4 of 9 deaths and about 70 other times (perhaps section loss, not tested). The
  `Damage_*` pools and `Ship_Destroyed01-03` were never heard, so the remake's damage pool on section loss stays PROVISIONAL.
  Collision pools are unused.
- Falloff (PROVISIONAL): inverse-distance rolloff like DirectSound 3D: full volume within 25 units of the camera, halving per doubling of distance, silent beyond the max distance; no stereo panning. The earlier linear falloff kept nearby collisions near full volume at normal zoom.
- Engine loops: no max distance in the data (300 used), level 0.25 (PROVISIONAL mix); silent at All Stop. Music level 0.35, one fixed track.
- One fire sound per shell of a burst.

## Crew voice lines (from the community Unity remake)
`Sound/Dialogue/<RACE><ROLE><nn>[B].ogg` are retail files; the community remake ("CL ModPack") groups them by event in
`CL_Data/Voicelines/English/<Race>/<event>/`. Events (e.g. `full_ahead`, `slow_ahead`, `full_stop`, `emergency_speed`, `engine_hit25/50/75`,
`sails_damaged`, `rudder_destroyed`, `fire_weapon`, `enemy_ship_spotted_<faction>`, `hull_damage`, `retreat`, `mission_won`) -> file lists are exported by
`tools/export_voices.py` to `content/voice/events.json`, which ships with the code (event and file names only; Procyon, HumanMale, HumanFemale, Robot). The import copies the named retail Ogg files, so neither players nor contributors need the CL ModPack. The grouping is the community's
interpretation, not original-derived: PROVISIONAL. Wired so far: helmsman line on player engine-order change (`audio.rs::helmsman`).
**Who speaks.** The retail dialogue folder holds one line set per species and post: the two-letter prefix is the species
(HM human male, HF human female, PR Procyon, RO war bot, AQ Aquanog, AR Arcturian, CA Canid, CR Cragorian, DE Densadron,
FE Felinid, GE Geeorie, MC Macriki, MN Mantavor, OP Optoc, TU Tuskrus, ZI Zirrelian), the rest the post (gunner, captain,
firstmate, helmsman, engineer, rigger, spotter) and line number; every species that voices a post has the same line count
for it, and not every species voices every post (Aquanogs: engineer, rigger, spotter). The remake picks a line from the
human-male table, then has it spoken by the first named crew member at that post whose species voices it, else by the
ship's faction crew (Navy human male, Procyon PR). PROVISIONAL: that the numbering means the same line across species, the
pirates' generic crew voice (human male until checked; possibly an alien set), and AI-captain (`<xx>AICAPTAIN`) lines unused.
The import copies every species' post lines. The talking head shows the speaker: the named crew member's portrait, else
the faction's default crew face, the same one the Crew screen shows for an unnamed hand (owner observation).
Regenerate the table: `python3 -m tools.export_voices table "$PROCYON_GAME_ROOT" <CL_Data/Voicelines/English> Procyon HumanMale HumanFemale Robot`.
