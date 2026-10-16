//! Sound: original Ogg files from assets/sound (tools/build_assets.sh). Names, volumes and max distances come from the WOTs
//! (gun SoundName/SoundVolume/SoundMaxDistance, bullet impact pools, ship EngineSoundName Half/Full/Emergency) and the exe's
//! built-in pools (docs/re/sound.md). PROVISIONAL: the distance falloff curve, engine/music levels, and which event uses which exe pool.

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::combat::{Health, Library};
use crate::menu::Screen;
use crate::{PlayerShip, Ship};

/// A one-shot positional sound; `at: None` plays at full volume (interface sounds).
#[derive(Message)]
pub struct Sfx {
    pub name: String,
    pub at: Option<Vec3>,
    pub vol: f32,
    pub max: f32,
}
impl Sfx {
    pub fn ui(name: &str) -> Self {
        Sfx { name: name.into(), at: None, vol: 1.0, max: 1.0 }
    }
}

/// Looping engine sound of `ship` for engine order `order` (1 Half, 2 Full, 3 Emergency).
#[derive(Component)]
struct EngineLoop {
    ship: Entity,
    order: usize,
}

/// PROVISIONAL: engine loops have no max distance in the WOT; levels for engine and music are guesses.
const ENGINE_MAX: f32 = 300.0;
const ENGINE_VOL: f32 = 0.25; // PROVISIONAL mix: quieter rumble leaves speech and weapon impacts audible.
const MUSIC_VOL: f32 = 0.35;

pub fn plugin(app: &mut App) {
    app.add_plugins(crate::crew::plugin)
        .add_message::<Sfx>()
        .add_systems(PostUpdate, (play, (engines, hud_sounds, helmsman, travel, ambience, ui_sounds).run_if(in_state(Screen::Battle))));
}

/// A shell's looping flight sound (bullet 'Travel Sound', its volume and short max distance). The loop only exists while the
/// camera is within reach, so a broadside does not open dozens of silent sinks.
#[derive(Component)]
pub struct Travel {
    pub name: String,
    pub vol: f32,
    pub max: f32,
}
#[derive(Component)]
struct TravelLoop;

fn travel(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    shells: Query<(Entity, &GlobalTransform, &Travel, Option<&Children>)>,
    mut loops: Query<(Entity, &mut AudioSink), With<TravelLoop>>,
) {
    for (e, tf, t, children) in &shells {
        let v = t.vol * gain(cam.translation(), Some(tf.translation()), t.max) * crate::menu::volume(crate::menu::Setting::Effects);
        let playing = children.and_then(|c| c.iter().find(|c| loops.contains(*c)));
        match (playing, v > 0.01) {
            (Some(l), true) => {
                if let Ok((_, mut sink)) = loops.get_mut(l) {
                    sink.set_volume(Volume::Linear(v));
                }
            }
            (Some(l), false) => commands.entity(l).despawn(),
            (None, true) => {
                commands.spawn((TravelLoop, ChildOf(e), AudioPlayer::new(assets.load(sound(&t.name))), PlaybackSettings::LOOP.with_volume(Volume::Linear(v))));
            }
            (None, false) => {}
        }
    }
}

/// Looping environment beds by camera distance: black holes (`Ambience_Black_Hole`), meteor storms (inside/outside variants by
/// the emitter's distance) and mission 7's Calyan Abyss ('Set Is In Calyan Abyss', played on the black-hole bed).
/// PROVISIONAL: the distances and which bed the abyss uses are not recovered from the exe.
#[derive(Component)]
struct Ambient(Entity);

fn ambience(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mission: Option<Res<crate::campaign::Mission>>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    holes: Query<(Entity, &GlobalTransform), With<crate::blackhole::BlackHole>>,
    meteors: Query<(Entity, &GlobalTransform), With<crate::nebula::MeteorEmitter>>,
    mut beds: Query<(Entity, &Ambient, &mut AudioSink)>,
    mut abyss: Local<Option<Entity>>,
) {
    let c = cam.translation();
    let mut has = vec![];
    for (e, a, mut sink) in &mut beds {
        let source = holes.get(a.0).map(|h| (h.1.translation(), 1500.0)).or_else(|_| meteors.get(a.0).map(|m| (m.1.translation(), 400.0)));
        match source {
            Ok((at, max)) => {
                has.push(a.0);
                sink.set_volume(Volume::Linear(0.6 * gain(c, Some(at), max) * crate::menu::volume(crate::menu::Setting::Effects)));
            }
            Err(_) if Some(e) == *abyss => {}
            Err(_) => commands.entity(e).despawn(),
        }
    }
    for (e, _) in &holes {
        if !has.contains(&e) {
            commands.spawn((crate::Scenario, Ambient(e), AudioPlayer::new(assets.load(sound("Ambience_Black_Hole"))), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0))));
        }
    }
    for (e, tf) in &meteors {
        if !has.contains(&e) {
            let inside = tf.translation().distance(c) < 150.0;
            let name = if inside { "MeteorStorm_Inside" } else { "MeteorStorm_Outside" };
            commands.spawn((crate::Scenario, Ambient(e), AudioPlayer::new(assets.load(sound(name))), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0))));
        }
    }
    let want = mission.is_some_and(|m| m.script.abyss);
    match (*abyss, want) {
        (None, true) => {
            *abyss =
                Some(commands.spawn((crate::Scenario, AudioPlayer::new(assets.load(sound("Ambience_Black_Hole"))), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.5)))).id());
        }
        (Some(e), false) => {
            commands.entity(e).try_despawn();
            *abyss = None;
        }
        _ => {}
    }
}

/// Front-end and HUD cues that have no owner of their own: pause/unpause and the NIS letterbox bars.
fn ui_sounds(vtime: Res<Time<Virtual>>, mission: Option<Res<crate::campaign::Mission>>, mut last: Local<(bool, bool)>, mut out: MessageWriter<Sfx>) {
    let now = (vtime.is_paused(), mission.is_some_and(|m| m.script.nis && m.opening.is_none()));
    if now.0 != last.0 {
        out.write(Sfx::ui(if now.0 { "HUD_Pause" } else { "HUD_UnPause" }));
    }
    if now.1 && !last.1 {
        out.write(Sfx::ui("HUD_NISBars"));
    }
    *last = now;
}

/// Asset path of a sound by name (case-insensitive, no extension). Files live in `sound/<kind>/` or in a mission's `maps/<id>/speech/`
/// and are found by scanning once, so mods may add sounds anywhere under those folders. Unknown names fall back to `sound/<name>.ogg`.
pub fn sound(name: &str) -> String {
    static INDEX: std::sync::OnceLock<std::collections::HashMap<String, String>> = std::sync::OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let root = crate::assets_dir();
        let mut files = vec![];
        crate::modding::walk(&root.join("sound"), std::path::Path::new("sound"), &mut files);
        for m in std::fs::read_dir(root.join("maps")).into_iter().flatten().flatten() {
            let id = m.file_name();
            crate::modding::walk(&m.path().join("speech"), &std::path::Path::new("maps").join(&id).join("speech"), &mut files);
        }
        files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "ogg"))
            .filter_map(|p| Some((p.file_stem()?.to_str()?.to_lowercase(), p.to_string_lossy().replace('\\', "/"))))
            .collect()
    });
    let name = name.to_lowercase();
    index.get(&name).cloned().unwrap_or_else(|| format!("sound/{name}.ogg"))
}

/// Within this distance of the camera a positional sound plays at full volume. PROVISIONAL: about two ship lengths.
const FULL_VOLUME_DISTANCE: f32 = 25.0;

/// Inverse-distance rolloff, the DirectSound 3D model the original used: full volume up to `FULL_VOLUME_DISTANCE`, halving with
/// every doubling of distance, silent beyond the sound's max distance. PROVISIONAL: the original's min-distance and rolloff settings
/// are not recovered; no stereo panning.
fn gain(listener: Vec3, at: Option<Vec3>, max: f32) -> f32 {
    at.map_or(1.0, |p| {
        let d = p.distance(listener);
        if d >= max { 0.0 } else { FULL_VOLUME_DISTANCE / d.max(FULL_VOLUME_DISTANCE) }
    })
}

#[derive(Component)]
pub struct Music;

/// Looping music track (the map's 'Play Music Track' file); returns the entity so the caller can tag it.
pub fn music(commands: &mut Commands, assets: &AssetServer, name: &str) -> Entity {
    commands
        .spawn((
            Music,
            AudioPlayer::new(assets.load(sound(name))),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(MUSIC_VOL * crate::menu::volume(crate::menu::Setting::Music))),
        ))
        .id()
}

fn play(mut commands: Commands, assets: Res<AssetServer>, mut msgs: MessageReader<Sfx>, cam: Single<&GlobalTransform, With<Camera3d>>, screen: Res<State<Screen>>) {
    for s in msgs.read() {
        // Front-end screens only play UI sounds (a menu backdrop battle stays silent).
        if s.at.is_some() && *screen.get() != Screen::Battle {
            continue;
        }
        let v = s.vol * gain(cam.translation(), s.at, s.max) * crate::menu::volume(crate::menu::Setting::Effects);
        debug!("sfx {} vol {v:.2}", s.name);
        if v > 0.01 {
            commands.spawn((AudioPlayer::new(assets.load(sound(&s.name))), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v))));
        }
    }
}

/// One loop per moving ship, swapped when the order changes; silent at All Stop (there is no idle sound file).
fn engines(
    mut commands: Commands,
    assets: Res<AssetServer>,
    lib: Res<Library>,
    cam: Single<&GlobalTransform, With<Camera3d>>,
    ships: Query<(Entity, &GlobalTransform, &Ship, &Health)>,
    mut loops: Query<(Entity, &EngineLoop, Option<&mut AudioSink>)>,
) {
    let mut has = vec![];
    for (e, l, sink) in &mut loops {
        match ships.get(l.ship) {
            Ok((_, tf, s, h)) if !h.destroyed && s.order == l.order => {
                has.push(l.ship);
                if let Some(mut sink) = sink {
                    sink.set_volume(Volume::Linear(ENGINE_VOL * crate::menu::volume(crate::menu::Setting::Effects) * gain(cam.translation(), Some(tf.translation()), ENGINE_MAX)));
                }
            }
            _ => commands.entity(e).despawn(),
        }
    }
    for (e, tf, s, h) in &ships {
        if h.destroyed || s.order == 0 || has.contains(&e) {
            continue;
        }
        let Some(name) = &lib.classes[s.class].engine_snd[s.order.min(3) - 1] else { continue };
        let v = ENGINE_VOL * crate::menu::volume(crate::menu::Setting::Effects) * gain(cam.translation(), Some(tf.translation()), ENGINE_MAX);
        commands.spawn((EngineLoop { ship: e, order: s.order }, ChildOf(e), AudioPlayer::new(assets.load(sound(name))), PlaybackSettings::LOOP.with_volume(Volume::Linear(v))));
    }
}

/// Speed-control clicks when the player's engine order changes (keys or dial), weapon-bar open/close.
fn hud_sounds(
    player: Option<Single<(&Ship, Has<crate::docking::Docker>), With<PlayerShip>>>,
    toggles: Res<crate::panels::Toggles>,
    mission: Option<Res<crate::campaign::Mission>>,
    mut last: Local<Option<(usize, bool)>>,
    mut last_wp: Local<Option<Vec3>>,
    mut out: MessageWriter<Sfx>,
) {
    let Some(player) = player else { return };
    let (p, docking) = *player;
    // Orders during the opening briefing are scripted, like docking ones.
    let docking = docking || mission.is_some_and(|m| m.opening.is_some());
    let now = (p.order, toggles.weapons);
    if let Some((order, weapons)) = *last {
        if !docking && now.0 > order {
            out.write(Sfx::ui("HUD_SpeedControl_Up"));
        }
        if !docking && now.0 < order {
            out.write(Sfx::ui("HUD_SpeedControl_Down"));
        }
        if now.1 != weapons {
            out.write(Sfx::ui(if now.1 { "HUD_WeaponBar_Open" } else { "HUD_WeaponBar_Close" }));
        }
    }
    *last = Some(now);
    // A new last waypoint means a click (replace or Shift-add); reaching waypoints only shortens the list. PROVISIONAL: HUD_Move is the move-order click.
    let wp = p.waypoints.last().copied();
    if !docking && wp.is_some() && wp != *last_wp {
        out.write(Sfx::ui("HUD_Move"));
    }
    *last_wp = wp;
}

/// Crew line (content/voice/events.json, grouping from the community remake; tools/export_voices.py) when the player's engine order changes.
/// Spoken by the helmsman's voice (`crew::voice_line`). PROVISIONAL: order->event mapping (3 -> emergency_speed vs full_power) is a guess.
fn helm_acknowledges(last: &mut Option<usize>, order: usize, docking: bool) -> bool {
    // Track automatic orders too, so leaving the dock does not announce a stale change.
    last.replace(order).is_some_and(|prev| prev != order) && !docking
}

fn helmsman(
    player: Option<Single<(&Ship, Has<crate::docking::Docker>, Option<&crate::crew_arms::CrewPosts>), With<PlayerShip>>>,
    lib: Res<Library>,
    mission: Option<Res<crate::campaign::Mission>>,
    mut last: Local<Option<usize>>,
    mut n: Local<usize>,
    mut commands: Commands,
    assets: Res<AssetServer>,
) {
    let Some(player) = player else { return };
    let (p, docking, posts) = *player;
    // Scripted orders during the opening briefing are not the player's.
    let scripted = docking || mission.is_some_and(|m| m.opening.is_some());
    if !helm_acknowledges(&mut last, p.order, scripted) {
        return;
    }
    let ev = ["full_stop", "slow_ahead", "full_ahead"].get(p.order).copied().unwrap_or("emergency_speed");
    *n += 1;
    debug!("voice {ev}");
    let Some(file) = crate::crew::voice_line(ev, *n, &lib.classes[p.class].race, posts, crate::crew::voice_exists) else { return };
    commands.spawn((
        AudioPlayer::new(assets.load(format!("voice/{file}.ogg"))),
        PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(crate::menu::volume(crate::menu::Setting::Dialogue))),
        crate::panels::Helmsman,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_sounds_fade_with_camera_distance() {
        let at = |d: f32| gain(Vec3::ZERO, Some(Vec3::X * d), 200.0);
        assert_eq!(at(10.0), 1.0, "close to the camera: full volume");
        assert!((at(50.0) - 0.5).abs() < 1e-6 && (at(100.0) - 0.25).abs() < 1e-6, "halves per doubling of distance");
        assert!(at(60.0) < 0.5, "a collision at the usual camera distance is clearly quieter than full volume");
        assert_eq!(at(200.0), 0.0, "silent beyond the max distance");
        assert_eq!(gain(Vec3::ZERO, None, 200.0), 1.0, "interface sounds are not positional");
    }

    #[test]
    fn menu_duelist_order_changes_are_silent_but_button_sounds_still_play() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default(), bevy::state::app::StatesPlugin))
            .init_asset::<AudioSource>()
            .init_state::<Screen>()
            .insert_resource(Library::load(&crate::assets_dir()))
            .insert_resource(crate::panels::Toggles { weapons: false, names: false })
            .add_plugins(plugin);
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::default()));
        let hero = app
            .world_mut()
            .spawn((
                PlayerShip,
                Ship {
                    class: 0,
                    side: crate::Side::Player,
                    heading: 0.0,
                    speed: 0.0,
                    ang_vel: 0.0,
                    order: 1,
                    waypoints: vec![],
                    goal: None,
                    cap: 1.0,
                    throttle: None,
                    sail_energy: 1.0,
                    crew_speed: 1.0,
                    crew_turn: 1.0,
                },
            ))
            .id();
        app.update();
        app.world_mut().get_mut::<Ship>(hero).unwrap().order = 2;
        app.update();
        assert_eq!(app.world_mut().query::<&AudioPlayer>().iter(app.world()).count(), 0, "no helmsman or HUD audio from the menu duel");
        app.world_mut().resource_mut::<Messages<Sfx>>().write(Sfx::ui("FE_Button_Select"));
        app.update();
        assert_eq!(app.world_mut().query::<&AudioPlayer>().iter(app.world()).count(), 1, "menu button audio remains enabled");
    }

    #[test]
    fn sounds_resolve_by_name_in_their_folder() {
        assert_eq!(sound("Weapon_Caranade_FireMed"), "sound/weapons/weapon_caranade_firemed.ogg");
        assert_eq!(sound("HUD_Map_Open"), "sound/ui/hud_map_open.ogg");
        assert_eq!(sound("M01AMELIA01"), "maps/mission01/speech/m01amelia01.ogg");
        for name in ["Engine_NAVY_MED", "Damage_Wood01", "Explosion_Heavy08", "BTL_NavyBig_Full02", "Ambience_Nebula", "Animal_WhaleCall01", "mtutamelia49a"] {
            assert!(crate::assets_dir().join(sound(name)).is_file(), "{name}");
        }
    }

    #[test]
    fn docking_orders_are_silent_without_delaying_acknowledgements_until_release() {
        let mut last = None;
        assert!(!helm_acknowledges(&mut last, 0, false), "initial state is silent");
        assert!(helm_acknowledges(&mut last, 2, false), "player order");
        assert!(!helm_acknowledges(&mut last, 1, true), "automatic approach");
        assert!(!helm_acknowledges(&mut last, 0, true), "automatic stop");
        assert!(!helm_acknowledges(&mut last, 0, false), "dock release is silent");
        assert!(helm_acknowledges(&mut last, 1, false), "next player order still works");
    }
}
