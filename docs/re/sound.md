# Sound

Retail sound is Ogg Vorbis under `Sound/` (`SoundEffects`, `GUI`, `Music`, `Dialogue`, plus `_LQ` low-quality copies).
`tools/build_assets.sh` copies `SoundEffects`, `GUI` and one music track to `local/sound/` with lowercase names, because
WOT names differ in case from the files (`Engine_NAVY_FULL` vs `Engine_Navy_Full.ogg`). Played by `game/src/audio.rs`.

## Data-driven (WOT, exported by `tools/export_combat.py`)
- Gun: `SoundName`, `SoundVolume`, `SoundMaxDistance` (fire sound).
- Bullet: `ImpactSounds<surface>` blocks of 5 slots (empty slots unused) per surface `Hull Wood`, `Hull ReInforced Wood`,
  `Hull Iron`, `Sail Cloth`, `Wall Stone`, `Dragon Scale`; `Impact Sound Volume`, `Impact Sound Max Distance`.
  Also `RicochetSound: <surface>` and `Travel Sound` (not used yet: no ricochets, travel sounds mostly empty).
- Ship: `EngineSoundName Half|Full|Emergency` (looping engine sound per engine order).
- Music: each skirmish map's TWT script plays one track ("Play Music Track", `File Name String`), e.g. Red Rover
  `BTL_NavyBig_Full02`, Ambush `BTL_DeadlyPirate_Full02`, Bayles Stand `BTL_ProcWarDrums_Full`.

## Built into the exe (not in the data files)
The original game has built-in sound-name tables that the world setup turns
into random pools. Six material groups in order Wood, Reinforced Wood, Iron, Cloth, Stone, Dragon (empty), each 10 slots:
- damage pool `Damage_Wood01-05` (Wood and Reinforced), `Damage_Metal01-05`, `Damage_Sail01-05`, `Damage_Stone01-05`; float 600 stored with it
- collision pool `Ship_Collision_Wood01-05` (Wood, Reinforced), `Ship_Collision_Metal01-05`; float 200
- `Explosion_Heavy01-08` pool, float 8000; `Explosion_Med01` pool, float 6000.
The float is read as the pool's max distance (same field order as the WOT pools). Interface sounds such as
`HUD_SpeedControl_Up/Down` and `HUD_FireAll` are also exe-global names.

## PROVISIONAL in the remake
- Which event uses which exe pool: damage pool on section loss, heavy explosion on ship destruction. Collision and
  `Explosion_Med01` pools are unused (no collisions yet).
- Falloff (PROVISIONAL): inverse-distance rolloff like DirectSound 3D: full volume within 25 units of the camera, halving per doubling of distance, silent beyond the max distance; no stereo panning. The earlier linear falloff kept nearby collisions near full volume at normal zoom.
- Engine loops: no max distance in the data (300 used), level 0.6; silent at All Stop. Music level 0.35, one fixed track.
- One fire sound per shell of a burst.

## Crew voice lines (from the community Unity remake)
`Sound/Dialogue/<RACE><ROLE><nn>[B].ogg` are retail files; the community remake ("CL ModPack") groups them by event in
`CL_Data/Voicelines/English/<Race>/<event>/`. Events (e.g. `full_ahead`, `slow_ahead`, `full_stop`, `emergency_speed`, `engine_hit25/50/75`,
`sails_damaged`, `rudder_destroyed`, `fire_weapon`, `enemy_ship_spotted_<faction>`, `hull_damage`, `retreat`, `mission_won`) -> file lists are exported by
`tools/export_voices.py` to `content/voice/events.json`, which ships with the code (event and file names only; Procyon, HumanMale, HumanFemale, Robot). The import copies the named retail Ogg files, so neither players nor contributors need the CL ModPack. The grouping is the community's
interpretation, not exe-derived: PROVISIONAL. Wired so far: helmsman line on player engine-order change (`audio.rs::helmsman`).
Regenerate the table: `python3 -m tools.export_voices table "$PROCYON_GAME_ROOT" <CL_Data/Voicelines/English> Procyon HumanMale HumanFemale Robot`.
