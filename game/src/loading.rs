//! Battle start: the battle waits, paused and behind the view's cover, until what it shows is ready: models and their textures
//! loaded and placed, the sky's pictures loaded, the shader pipelines compiled. The scene is drawn under the cover meanwhile, so
//! the uploads and compiles that would stutter the first seconds happen unseen, and its music waits with it. Not retail's loading screen (PROVISIONAL: its
//! layout is not specified yet); the battle clock starts when the cover lifts, so nothing of the battle is lost.
use bevy::asset::{UntypedAssetId, VisitAssetDependencies};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames in a row with nothing left to load before the cover lifts: loads follow one another (a model brings its materials,
/// which bring their textures, which bring their pipelines), each a frame or two after the one before.
const SETTLE: u32 = 10;
/// Real seconds after which the battle starts anyway (a shader that never arrives must not hold the game).
const GIVE_UP: f32 = 20.0;

/// Present while a battle is loading. Views draw their cover while it exists.
#[derive(Resource, Default)]
pub struct Loading {
    quiet: u32,
    waited: f32,
    /// What was last waited for (named when the wait gives up).
    last: &'static str,
}

impl Loading {
    /// Something is still loading this frame: any system that waits on an asset says so.
    pub fn busy(&mut self, what: &'static str) {
        self.quiet = 0;
        self.last = what;
    }
    /// One frame of `dt` real seconds went by; whether the battle may start.
    fn frame(&mut self, dt: f32) -> bool {
        self.quiet += 1;
        self.waited += dt;
        self.quiet > SETTLE || self.waited > GIVE_UP
    }
}

/// Run condition: no battle is loading.
pub fn done(loading: Option<Res<Loading>>) -> bool {
    loading.is_none()
}

/// Whether the render world still has pipelines to compile (written there, read here).
#[derive(Resource, Clone, Default)]
struct Compiling(Arc<AtomicBool>);

fn pipelines(cache: Res<bevy::render::render_resource::PipelineCache>, flag: Res<Compiling>) {
    flag.0.store(cache.waiting_pipelines().next().is_some(), Ordering::Relaxed);
}

pub(crate) fn pending(server: &AssetServer, id: UntypedAssetId) -> bool {
    server.load_state(id).is_loading() || server.recursive_dependency_load_state(id).is_loading()
}

/// Holds the battle while loading: time paused, input dropped (nothing is clicked through the cover), and lifts when ready.
fn wait(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    (real, mut vtime): (Res<Time<Real>>, ResMut<Time<Virtual>>),
    (mut keys, mut mouse): (ResMut<ButtonInput<KeyCode>>, ResMut<ButtonInput<MouseButton>>),
    (server, compiling): (Res<AssetServer>, Res<Compiling>),
    models: Query<(&WorldAssetRoot, Has<Children>)>,
    skies: Query<&crate::view::HdSky>,
    mut arrived: MessageReader<AssetEvent<Image>>,
) {
    if loading.frame(real.delta_secs()) {
        if loading.waited > GIVE_UP {
            warn!("battle still loading after {GIVE_UP} s (waiting for {}): starting anyway", loading.last);
        }
        info!("battle loaded in {:.1} s", loading.waited);
        commands.remove_resource::<Loading>();
        vtime.unpause(); // a fresh battle never starts paused (`scenario::resume_scenario`)
        return;
    }
    vtime.pause();
    keys.reset_all();
    mouse.reset_all();
    // a model is ready when it and its textures are loaded and its parts are placed; one that failed to load is not waited for
    let model = models.iter().any(|(m, placed)| pending(&server, m.0.id().untyped()) || (server.is_loaded_with_dependencies(&m.0) && !placed));
    let sky = skies.iter().any(|s| pending(&server, s.0.id().untyped()));
    // only files that finish loading count: a battle also makes and rewrites pictures of its own every frame
    // (`count`, not `any`: every message is read, or the rest would trickle in over the next frames)
    let arrived = arrived.read().filter(|e| matches!(e, AssetEvent::LoadedWithDependencies { id } if server.get_path(*id).is_some())).count() > 0;
    for (waits, what) in [(model, "models"), (sky, "the sky"), (arrived, "textures arriving"), (compiling.0.load(Ordering::Relaxed), "shader pipelines")] {
        if waits {
            loading.busy(what);
        }
    }
}

/// Materials of type `M` whose textures are still loading hold the battle.
fn textures<M: Asset>(mut loading: ResMut<Loading>, server: Res<AssetServer>, materials: Res<Assets<M>>, base: fn(&M) -> &StandardMaterial) {
    let waits = |m: &M| {
        let mut any = false;
        base(m).visit_dependencies(&mut |id| any |= pending(&server, id));
        any
    };
    if materials.iter().any(|(_, m)| waits(m)) {
        loading.busy("material textures");
    }
}
fn plain(loading: ResMut<Loading>, server: Res<AssetServer>, materials: Res<Assets<StandardMaterial>>) {
    textures(loading, server, materials, |m| m);
}
/// The same for a material that extends the standard one (`render/`): add it where the material's plugin is added.
pub fn extended<E: MaterialExtension>(loading: ResMut<Loading>, server: Res<AssetServer>, materials: Res<Assets<ExtendedMaterial<StandardMaterial, E>>>) {
    textures(loading, server, materials, |m| &m.base);
}

/// No battle sound under the cover: music and every other sound that starts while the battle loads starts held, and plays when
/// the cover lifts. A sound already playing (the click that launched the battle) plays on.
fn hush(loading: Option<Res<Loading>>, mut starting: Query<&mut PlaybackSettings, Without<AudioSink>>, sinks: Query<&AudioSink>, mut held: Local<bool>) {
    use bevy::audio::AudioSinkPlayback;
    if loading.is_some() {
        *held = true;
        starting.iter_mut().for_each(|mut s| s.paused = true);
    } else if std::mem::take(&mut *held) {
        // nothing else holds a sound, so everything held is ours: those whose file has not arrived yet included
        starting.iter_mut().for_each(|mut s| s.paused = false);
        sinks.iter().for_each(|s| s.play());
    }
}

pub fn plugin(app: &mut App) {
    let compiling = Compiling::default();
    if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render.insert_resource(compiling.clone()).add_systems(bevy::render::Render, pipelines.in_set(bevy::render::RenderSystems::Cleanup));
    }
    app.insert_resource(compiling)
        .add_systems(PreUpdate, wait.after(bevy::input::InputSystems).before(bevy::ui::UiSystems::Focus).run_if(resource_exists::<Loading>))
        .add_systems(Update, plain.run_if(resource_exists::<Loading>))
        // between Update, whose systems start sounds, and PostUpdate, where Bevy begins to play them (its set there is private)
        .add_systems(SpawnScene, hush);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cover_lifts_after_quiet_frames_and_a_busy_frame_starts_the_count_again() {
        let mut l = Loading::default();
        for _ in 0..SETTLE {
            assert!(!l.frame(0.016));
        }
        l.busy("test");
        for _ in 0..SETTLE {
            assert!(!l.frame(0.016), "a busy frame starts the count again");
        }
        assert!(l.frame(0.016));
    }

    #[test]
    fn a_sound_that_starts_while_loading_is_held_until_the_cover_lifts() {
        let mut world = World::new();
        let hush = world.register_system(hush);
        let before = world.spawn(PlaybackSettings::LOOP).id();
        world.run_system(hush).unwrap();
        assert!(!world.get::<PlaybackSettings>(before).unwrap().paused, "no battle loading: sounds start as usual");
        world.insert_resource(Loading::default());
        let music = world.spawn(PlaybackSettings::LOOP).id();
        world.run_system(hush).unwrap();
        assert!(world.get::<PlaybackSettings>(music).unwrap().paused);
        world.remove_resource::<Loading>();
        world.run_system(hush).unwrap();
        assert!(!world.get::<PlaybackSettings>(music).unwrap().paused);
    }

    #[test]
    fn a_load_that_never_ends_does_not_hold_the_battle() {
        let mut l = Loading::default();
        l.busy("test");
        assert!(!l.frame(GIVE_UP - 1.0));
        l.busy("test");
        assert!(l.frame(2.0));
    }
}
