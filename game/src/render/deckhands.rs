//! Not retail: deck crews of the render profile (part of render/mod.rs's look). Small figures that man the deck guns, keep watch,
//! wander the decks, go aloft when the sails fold or open, fall as the hull is shot to pieces and cheer a victory. Presentation only:
//! they read the battle (gun reloads, orders, hull, outcome), never change it, and use their own random numbers.
//!
//! - The figure and its moves come from the mod: `data/render/deckhand.json` and `.bin` (built by the expansion's
//!   `tools/build_deckhand.py`): one mesh whose vertices are posed on the GPU from a vertex animation texture (`deckhand_vat.wesl`;
//!   the engine's vertex stages `deckhand_vertex.wesl` and `deckhand_prepass.wesl`, the mod's `shaders/deckhands.wesl` shades). Every
//!   figure shares one mesh and one material, so a whole battle's crews are one batch; what each plays is its `MeshTag`.
//! - Where they go comes from the ship's model: empty nodes named `Crew_<Kind>_...` (`Walk` lanes, `Gun` places by a deck mount,
//!   `Post`s, `Climb` pairs `_a` at the foot of a shroud and `_b` up it). A ship without them gets no crew. How many wander grows
//!   with the deck (`headcount`: 1-2 on a fishing boat, 40 on a Man-o'-War, 60 on a first rate) unless a `Crew_Count_<n>` node sets the total.
//! - Figures are children of the ship, so they ride its motion; they cast no shadows and vanish beyond `range`.
//! - Map scenery (a town, a pier) gets townsfolk the same way: `Crew_Walk` lanes, `Post`s, and `Work` places where a docker hauls
//!   or loads for ever. They wear the `Civilian` outfits, one wanders per three walk points (at most `per_ship`), and they cheer a
//!   victory; nothing else reaches them (no hull, guns or sails).
//!
//! PROVISIONAL (a prototype): straight-line walking along the lanes (no avoidance), headcount from the hull's core HP, no blending
//! between clips.

use std::collections::{HashMap, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshTag, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use serde_json::Value;

use super::{Look, num, rgb};
use crate::combat::{GunStates, Health, Library};
use crate::scenario::Outcome;
use crate::{Ship, Side};

/// The fragment shader; the mod ships it.
pub const SHADER: &str = "shaders/deckhands.wesl";
/// The figure and its baked moves (`.bin` beside it); the mod ships them.
pub const DATA: &str = "data/render/deckhand.json";
const VERTEX_SHADER: &str = "embedded://openprocyon/render/deckhand_vertex.wesl";
const PREPASS_SHADER: &str = "embedded://openprocyon/render/deckhand_prepass.wesl";

const MAX_CLIPS: usize = 8;
const PARTS: usize = 5;
const MAX_OUTFITS: usize = 16;
/// The clips the engine plays, in the shader's tag numbering; `deckhand.json` must list them in this order.
const CLIPS: [&str; MAX_CLIPS] = ["idle", "walk", "run", "haul", "load", "climb", "cheer", "fall"];
const IDLE: u32 = 0;
const WALK: u32 = 1;
const RUN: u32 = 2;
const HAUL: u32 = 3;
const LOAD: u32 = 4;
const CLIMB: u32 = 5;
const CHEER: u32 = 6;
const FALL: u32 = 7;
/// Walk points closer than this join, whichever lanes they are on.
const LINK: f32 = 2.3;
/// Seconds a fallen figure lies on deck before it goes.
const LIE: f32 = 12.0;
/// Seconds after a gun last reloaded that the crew still runs (beat to quarters).
const ALARM: f32 = 10.0;

/// Headcount calibration: deck area (units^2) that counts as one unit before the power law. Calibrated on retail hulls (see `headcount`), not read from the game's data.
const HEADCOUNT_DECK_UNIT: f32 = 16.5;
/// Headcount calibration: the power law's exponent. Calibrated on retail hulls (see `headcount`), not read from the game's data.
const HEADCOUNT_EXPONENT: f32 = 1.52;
/// A gun place belongs to the nearest gun mount only within this distance (units). Engine choice, not the original's.
const GUN_MOUNT_REACH: f32 = 4.0;
/// A figure waits a random time up to this many seconds before its first walk. Engine choice, not the original's.
const MAX_IDLE_WAIT: f32 = 6.0;
/// Distance (units) over which a crew figure fades out past its draw range. Engine choice, not the original's.
const FADE_OUT_RANGE: f32 = 30.0;
/// PROVISIONAL: share of the crew that stays standing until the hull is gone; a guess at how the original thins a crew with hull damage.
const STANDING_MIN: f32 = 0.3;
/// PROVISIONAL: shortest pause (seconds) between two figures falling as the hull weakens; a guess.
const KILL_WAIT_MIN: f32 = 0.4;
/// PROVISIONAL: random extra pause (seconds) on top of `KILL_WAIT_MIN`; a guess.
const KILL_WAIT_SPAN: f32 = 0.8;

/// Figures on a ship whose model gives no `Crew_Count_<n>`, from its deck: the collision box's length times beam, in units^2. Crews
/// grow faster than decks (`area^1.52`), calibrated on retail hulls: a fishing boat (15-26) carries 1-2, a sloop or packet (24-48) 2-5,
/// a cutter (61) 7, a frigate (115) 19, a Man-o'-War (186) 40; a first rate reaches `per_ship` (60), which caps every ship. Gun places,
/// posts and work places are always manned, and a ship with shrouds keeps one wanderer per shroud to go aloft; the count only sizes the
/// wanderers.
fn headcount(deck: f32) -> usize {
    (deck.max(0.0) / HEADCOUNT_DECK_UNIT).powf(HEADCOUNT_EXPONENT).round().max(1.0) as usize
}

/// One outfit: the colours of a figure's parts (linear rgb), worn by a race's crews; `officer` outfits go to the posts.
#[derive(Clone, Debug, PartialEq)]
pub struct Outfit {
    pub race: String,
    pub officer: bool,
    pub colors: [Vec3; PARTS],
}

/// The `deckhands` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Deckhands {
    /// Most figures on one ship, a ceiling: how many a ship carries comes from its deck (`headcount`) or its model's `Crew_Count_<n>`
    /// (gun places and posts fill first, the rest wander).
    pub per_ship: usize,
    /// Camera distance beyond which figures are not drawn.
    pub range: f32,
    pub outfits: Vec<Outfit>,
}

pub fn parse(v: &Value) -> Option<Deckhands> {
    if !v.is_object() {
        return None;
    }
    let outfits = v["outfits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| {
            let mut colors = [Vec3::splat(0.5); PARTS];
            for (i, k) in ["skin", "coat", "trousers", "shoes", "hat"].iter().enumerate() {
                if let Some(c) = rgb(&o[*k]) {
                    colors[i] = c;
                }
            }
            Some(Outfit { race: o["race"].as_str()?.to_string(), officer: o["officer"].as_bool().unwrap_or(false), colors })
        })
        .take(MAX_OUTFITS)
        .collect();
    Some(Deckhands { per_ship: num(&v["per_ship"], 60.0).max(0.0) as usize, range: num(&v["range"], 160.0).max(1.0), outfits })
}

pub fn plugin(app: &mut App, look: &Look) {
    if look.deckhands.is_some() {
        bevy::shader::load_shader_library!(app, "deckhand_vat.wesl");
        bevy::asset::embedded_asset!(app, "deckhand_vertex.wesl");
        bevy::asset::embedded_asset!(app, "deckhand_prepass.wesl");
        app.add_plugins(MaterialPlugin::<DeckhandMaterial>::default()).add_systems(Startup, load_kit).add_systems(Update, (gather, staff, drive).chain());
    }
}

/// What the vertex stages get at `@group(MATERIAL_BIND_GROUP) @binding(100)` (`struct Deckhand` in deckhand_vat.wesl).
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct DeckhandUniform {
    /// Per clip: x first row, y frames, z frames a second, w 1 when it loops.
    pub clips: [Vec4; MAX_CLIPS],
    /// x: frames in all (the normal rows start there).
    pub info: Vec4,
    /// rgb per outfit and part: `outfit * 5 + part`.
    pub palette: [Vec4; MAX_OUTFITS * PARTS],
}

impl Default for DeckhandUniform {
    fn default() -> Self {
        DeckhandUniform { clips: [Vec4::ZERO; MAX_CLIPS], info: Vec4::ZERO, palette: [Vec4::ONE; MAX_OUTFITS * PARTS] }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct DeckhandExt {
    #[uniform(100)]
    pub u: DeckhandUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`: the battle clock the moves run on.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    /// The baked moves (Rgba16Float, read with textureLoad).
    #[texture(101)]
    pub vat: Handle<Image>,
}

impl MaterialExtension for DeckhandExt {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn vertex_shader() -> ShaderRef {
        VERTEX_SHADER.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        PREPASS_SHADER.into()
    }
}

pub type DeckhandMaterial = ExtendedMaterial<StandardMaterial, DeckhandExt>;

#[derive(Clone, Copy)]
struct Clip {
    seconds: f32,
}

/// The loaded figure: the shared mesh and material, clip lengths, how fast he moves, and which outfit index each race may wear.
#[derive(Resource)]
struct Kit {
    mesh: Handle<Mesh>,
    material: Handle<DeckhandMaterial>,
    clips: Vec<Clip>,
    walk: f32,
    run: f32,
    climb: f32,
    aabb: Aabb,
    range: f32,
    per_ship: usize,
    outfits: Vec<Outfit>,
}

fn f16_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The figure's header and texels, checked: None (with a warning) leaves the block off.
fn read_kit(root: &std::path::Path) -> Option<(Value, Vec<u8>)> {
    let head: Value = crate::data::load_or_base(&root.join(DATA), None, "deck crews are left off")?;
    let bin = std::fs::read(root.join(DATA).with_extension("bin")).ok()?;
    let (verts, frames) = (head["verts"].as_u64()? as usize, head["frames"].as_u64()? as usize);
    let clips = head["clips"].as_array()?;
    let names: Vec<&str> = clips.iter().filter_map(|c| c["name"].as_str()).collect();
    if bin.len() != verts * frames * 2 * 8 || names != CLIPS || clips.len() != CLIPS.len() || head["part"].as_array()?.len() != verts {
        warn!("render profile: {DATA} does not match its .bin or the clips {CLIPS:?}; deck crews left off");
        return None;
    }
    Some((head, bin))
}

fn load_kit(mut commands: Commands, look: Res<Look>, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<DeckhandMaterial>>) {
    let Some(d) = look.deckhands.as_ref() else { return };
    let Some((head, bin)) = read_kit(crate::assets_dir()) else {
        warn!("render profile: `deckhands` needs {DATA} and its .bin in the mod; left off");
        return;
    };
    let verts = head["verts"].as_u64().unwrap_or(0) as usize;
    let frames = head["frames"].as_u64().unwrap_or(0) as usize;
    let fps = num(&head["fps"], 15.0);
    let texel = |i: usize| -> Vec3 {
        let h = |k: usize| f16_to_f32(u16::from_le_bytes([bin[i * 8 + k * 2], bin[i * 8 + k * 2 + 1]]));
        Vec3::new(h(0), h(1), h(2))
    };
    // the rest shape is the first frame (idle); the shader replaces it every frame, the CPU only needs its count and bounds
    let pos: Vec<[f32; 3]> = (0..verts).map(|i| texel(i).to_array()).collect();
    let nor: Vec<[f32; 3]> = (0..verts).map(|i| texel(frames * verts + i).to_array()).collect();
    let uv: Vec<[f32; 2]> = (0..verts).map(|i| [i as f32, 0.0]).collect();
    let col: Vec<[f32; 4]> = head["part"].as_array().unwrap().iter().map(|p| [p.as_f64().unwrap_or(0.0) as f32 / 8.0, 0.0, 0.0, 1.0]).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32((0..verts as u32).collect()));
    let vat = Image::new(
        Extent3d { width: verts as u32, height: 2 * frames as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        bin,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mut u = DeckhandUniform { info: Vec4::new(frames as f32, verts as f32, 0.0, 0.0), ..default() };
    let mut clips = vec![];
    for (i, c) in head["clips"].as_array().unwrap().iter().enumerate() {
        let n = num(&c["frames"], 1.0);
        u.clips[i] = Vec4::new(num(&c["start"], 0.0), n, fps, f32::from(u8::from(c["loop"].as_bool().unwrap_or(true))));
        clips.push(Clip { seconds: num(&c["seconds"], n / fps) });
    }
    for (o, outfit) in d.outfits.iter().enumerate() {
        for (p, c) in outfit.colors.iter().enumerate() {
            u.palette[o * PARTS + p] = c.extend(1.0);
        }
    }
    let b = &head["bounds"];
    let v = |k: usize| Vec3::new(num(&b[k][0], -1.0), num(&b[k][1], 0.0), num(&b[k][2], -1.0));
    let material = materials.add(ExtendedMaterial {
        base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() },
        extension: DeckhandExt { u, frame: default(), vat: images.add(vat) },
    });
    commands.insert_resource(Kit {
        mesh: meshes.add(mesh),
        material,
        clips,
        walk: num(&head["speeds"]["walk"], 1.3),
        run: num(&head["speeds"]["run"], 3.6),
        climb: num(&head["speeds"]["climb"], 0.9),
        aabb: Aabb::from_min_max(v(0), v(1)),
        range: d.range,
        per_ship: d.per_ship,
        outfits: d.outfits.clone(),
    });
}

/// A ship's crew places, read from its model's `Crew_*` nodes (ship frame).
#[derive(Default, Clone)]
struct Places {
    /// Walk points and the points each joins.
    walk: Vec<Vec3>,
    links: Vec<Vec<usize>>,
    guns: Vec<Transform>,
    posts: Vec<(String, Transform)>,
    /// Places where a figure works on the spot (scenery: dockers at the cargo).
    work: Vec<Transform>,
    /// Foot and top of each shroud a rigger climbs, and the way he faces on it.
    climbs: Vec<(Vec3, Vec3, Quat)>,
    /// `Crew_Count_<n>`: the figures the model asks for (all roles), instead of the count from the deck.
    count: Option<usize>,
}

impl Places {
    fn from_nodes(nodes: &[(String, Transform)]) -> Places {
        let mut p = Places::default();
        let mut lanes: HashMap<String, Vec<(u32, usize)>> = HashMap::new();
        let mut climbs: HashMap<String, [Option<Transform>; 2]> = HashMap::new();
        for (name, t) in nodes {
            let rest = &name["Crew_".len()..];
            let (kind, id) = rest.split_once('_').unwrap_or((rest, ""));
            match kind {
                "Walk" => {
                    let (lane, k) = id.rsplit_once('_').unwrap_or((id, "0"));
                    lanes.entry(lane.into()).or_default().push((k.parse().unwrap_or(0), p.walk.len()));
                    p.walk.push(t.translation);
                }
                "Gun" => p.guns.push(*t),
                "Post" => p.posts.push((id.to_string(), *t)),
                "Work" => p.work.push(*t),
                "Count" => p.count = id.split('_').next().and_then(|n| n.parse().ok()),
                "Climb" => {
                    let (stem, end) = id.rsplit_once('_').unwrap_or((id, "a"));
                    climbs.entry(stem.into()).or_default()[usize::from(end == "b")] = Some(*t);
                }
                _ => {}
            }
        }
        p.links = vec![vec![]; p.walk.len()];
        let join = |a: usize, b: usize, l: &mut Vec<Vec<usize>>| {
            if a != b && !l[a].contains(&b) {
                l[a].push(b);
                l[b].push(a);
            }
        };
        for lane in lanes.values_mut() {
            lane.sort();
            for w in lane.windows(2) {
                join(w[0].1, w[1].1, &mut p.links);
            }
        }
        for a in 0..p.walk.len() {
            for b in a + 1..p.walk.len() {
                if p.walk[a].distance(p.walk[b]) < LINK {
                    join(a, b, &mut p.links);
                }
            }
        }
        let mut stems: Vec<_> = climbs.into_iter().collect();
        stems.sort_by(|a, b| a.0.cmp(&b.0));
        p.climbs = stems.into_iter().filter_map(|(_, [a, b])| Some((a?.translation, b?.translation, a?.rotation))).collect();
        p
    }

    fn nearest(&self, at: Vec3) -> Option<usize> {
        (0..self.walk.len()).min_by(|&a, &b| self.walk[a].distance_squared(at).total_cmp(&self.walk[b].distance_squared(at)))
    }

    /// Points to walk through from `from` to `to`: along the lanes (breadth first) from the walk point nearest each end.
    fn route(&self, from: Vec3, to: Vec3) -> VecDeque<Vec3> {
        let mut out = VecDeque::new();
        if let (Some(a), Some(b)) = (self.nearest(from), self.nearest(to))
            && from.distance(to) > self.walk[a].distance(from) + self.walk[b].distance(to)
        {
            let mut prev = vec![usize::MAX; self.walk.len()];
            let mut queue = VecDeque::from([a]);
            prev[a] = a;
            while let Some(n) = queue.pop_front() {
                if n == b {
                    break;
                }
                for &m in &self.links[n] {
                    if prev[m] == usize::MAX {
                        prev[m] = n;
                        queue.push_back(m);
                    }
                }
            }
            if prev[b] != usize::MAX {
                let mut n = b;
                while n != a {
                    out.push_front(self.walk[n]);
                    n = prev[n];
                }
                out.push_front(self.walk[a]);
            }
        }
        out.push_back(to);
        out
    }
}

/// A ship's model nodes named `Crew_*`, gathered the frame its scene spawns.
#[derive(Component)]
struct CrewNodes {
    /// The scene root the nodes hang under (the model's frame).
    frame: Entity,
    nodes: Vec<(String, Transform)>,
}

/// A manned ship: its places, its figures and what they are reacting to.
#[derive(Component)]
struct Crew {
    places: Places,
    hands: Vec<Entity>,
    /// Figures at the start; the headcount falls from it with the hull.
    total: usize,
    max_core: f32,
    furled: bool,
    alarm: f32,
    kill_wait: f32,
    rng: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    /// The gun place's index in `Places::guns`, the mount (index in the ship's `GunStates`) and his slot (0, 1 behind the gun).
    Gun {
        place: usize,
        mount: usize,
        slot: usize,
    },
    Post {
        place: usize,
    },
    /// A work place's index in `Places::work`: he goes there and hauls (odd places) or loads (even ones).
    Work {
        place: usize,
    },
    Wander,
    /// A shroud (index in `Places::climbs`) and how far along: 0 going to its foot, 1 climbing, 2 working aloft, 3 coming down.
    Rigger {
        climb: usize,
        stage: u8,
    },
}

#[derive(Component)]
struct Hand {
    role: Role,
    pos: Vec3,
    face: Quat,
    path: VecDeque<Vec3>,
    /// What the path is for: run, or climb (faces the shroud).
    running: bool,
    climbing: bool,
    wait: f32,
    clip: u32,
    outfit: u32,
    phase: u32,
    started: u32,
    /// Seconds since he fell; None while alive.
    fallen: Option<f32>,
}

impl Hand {
    fn tag(&self) -> MeshTag {
        MeshTag::new(self.clip | (self.outfit << 4) | (self.phase << 8) | (self.started << 16))
    }
}

fn rand(rng: &mut u64) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng >> 40) as f32 / (1u64 << 24) as f32
}

/// The battle clock in tag units (tenths of a second, wrapping), as the shader reads `frame.clock.y`.
fn stamp(time: &Time) -> u32 {
    ((time.elapsed_secs_f64() * 10.0) as u64 & 0xffff) as u32
}

/// Collects the `Crew_*` nodes of a ship's (or a map scenery object's) model as its scene spawns. They hang under the scene's root (a child of the ship), whose
/// frame is the model's; the figures go under the same root, so the nodes' own transforms place them.
#[allow(clippy::type_complexity)]
fn gather(
    mut commands: Commands,
    nodes: Query<(Entity, &Name, &ChildOf, &Transform), Added<Name>>,
    ships: Query<(), (Or<(With<Ship>, With<crate::clips::SceneLoop>)>, Without<Crew>)>,
    parents: Query<&ChildOf>,
) {
    let mut found: HashMap<Entity, (Entity, Vec<(String, Transform)>)> = HashMap::new();
    for (e, name, parent, t) in &nodes {
        if !name.as_str().starts_with("Crew_") {
            continue;
        }
        if let Some(ship) = parents.iter_ancestors(e).take(4).find(|&a| ships.contains(a)) {
            found.entry(ship).or_insert((parent.parent(), vec![])).1.push((name.as_str().to_string(), *t));
        }
    }
    for (ship, (frame, nodes)) in found {
        commands.entity(ship).insert(CrewNodes { frame, nodes });
    }
}

/// Mans a ship whose crew places arrived: gun places, posts and work places first, then wanderers up to the headcount (the model's
/// `Crew_Count_<n>`, else `headcount` of the ship's deck); each figure picks an outfit of its race. Scenery (no `Ship`) is Civilian
/// and gets one wanderer per three walk points unless it gives a `Crew_Count`.
fn staff(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    lib: Res<Library>,
    time: Res<Time>,
    ships: Query<(Entity, Option<&Ship>, Option<&Health>, &CrewNodes, Option<&GunStates>), Without<Crew>>,
    mounts: Query<&Transform>,
) {
    let Some(kit) = kit else { return };
    for (e, ship, health, nodes, guns) in &ships {
        let places = Places::from_nodes(&nodes.nodes);
        let race = ship.map_or("Civilian", |s| lib.classes[s.class].race.as_str());
        let wear: Vec<(u32, bool)> = kit.outfits.iter().enumerate().filter(|(_, o)| o.race.eq_ignore_ascii_case(race)).map(|(i, o)| (i as u32, o.officer)).collect();
        let mut rng = 0x9e37_79b9_7f4a_7c15 ^ (e.to_bits().wrapping_mul(0x2545_f491_4f6c_dd1d));
        let mounts: Vec<Vec3> = guns.map_or(vec![], |g| g.reloads().map(|(root, _)| mounts.get(root).map_or(Vec3::splat(1e9), |t| t.translation)).collect());
        let mut roles = vec![];
        for (place, t) in places.guns.iter().enumerate() {
            let near = (0..mounts.len()).min_by(|&a, &b| mounts[a].distance(t.translation).total_cmp(&mounts[b].distance(t.translation)));
            if let Some(m) = near.filter(|&m| mounts[m].distance(t.translation) < GUN_MOUNT_REACH) {
                let slot = roles.iter().filter(|r| matches!(r, Role::Gun { mount, .. } if *mount == m)).count();
                roles.push(Role::Gun { place, mount: m, slot });
            }
        }
        roles.extend((0..places.posts.len()).map(|place| Role::Post { place }));
        roles.extend((0..places.work.len()).map(|place| Role::Work { place }));
        roles.truncate(kit.per_ship);
        let target = places.count.unwrap_or_else(|| match ship {
            Some(s) => {
                let c = &lib.classes[s.class];
                headcount(c.length * (c.collision_bounds[1].x - c.collision_bounds[0].x))
            }
            None => roles.len() + places.walk.len() / 3,
        });
        // riggers go aloft from the wanderers: one per shroud stays on deck whatever the count
        let target = if ship.is_some() { target.max(roles.len() + places.climbs.len()) } else { target };
        let wanderers = target.min(kit.per_ship).saturating_sub(roles.len());
        if !places.walk.is_empty() {
            roles.extend(std::iter::repeat_n(Role::Wander, wanderers));
        }
        let now = stamp(&time);
        let mut hands = vec![];
        for role in roles {
            let (pos, face) = match role {
                Role::Gun { place, .. } => (places.guns[place].translation, places.guns[place].rotation),
                Role::Post { place } => (places.posts[place].1.translation, places.posts[place].1.rotation),
                Role::Work { place } => (places.work[place].translation, places.work[place].rotation),
                _ => {
                    let i = (rand(&mut rng) * places.walk.len() as f32) as usize % places.walk.len();
                    (places.walk[i], Quat::from_rotation_y(rand(&mut rng) * std::f32::consts::TAU))
                }
            };
            let officer = matches!(role, Role::Post { .. });
            let pick: Vec<u32> = wear.iter().filter(|(_, o)| *o == officer).map(|(i, _)| *i).collect();
            let pick = if pick.is_empty() { wear.iter().map(|(i, _)| *i).collect() } else { pick };
            let outfit = if pick.is_empty() { 0 } else { pick[(rand(&mut rng) * pick.len() as f32) as usize % pick.len()] };
            let hand = Hand {
                role,
                pos,
                face,
                path: VecDeque::new(),
                running: false,
                climbing: false,
                wait: rand(&mut rng) * MAX_IDLE_WAIT,
                clip: IDLE,
                outfit,
                phase: (rand(&mut rng) * 255.0) as u32,
                started: now,
                fallen: None,
            };
            let tag = hand.tag();
            hands.push(
                commands
                    .spawn((
                        hand,
                        Transform::from_translation(pos).with_rotation(face),
                        Mesh3d(kit.mesh.clone()),
                        MeshMaterial3d(kit.material.clone()),
                        tag,
                        kit.aabb,
                        NoAutoAabb,
                        NotShadowCaster,
                        VisibilityRange { start_margin: 0.0..0.0, end_margin: kit.range..kit.range + FADE_OUT_RANGE, use_aabb: false },
                        ChildOf(nodes.frame),
                    ))
                    .id(),
            );
        }
        let total = hands.len();
        debug!("deckhands: {total} on {race} ship {e:?} ({} walk points, {} gun places)", places.walk.len(), places.guns.len());
        commands.entity(e).remove::<CrewNodes>().insert(Crew {
            places,
            hands,
            total,
            max_core: health.map_or(1.0, |h| h.core.max(1.0)),
            furled: ship.is_some_and(|s| s.ordered() == 0.0),
            alarm: 0.0,
            kill_wait: 0.0,
            rng,
        });
    }
}

/// Each manned ship: who falls, whether the riggers go aloft, then every figure's next step and clip.
#[allow(clippy::type_complexity)]
fn drive(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    time: Res<Time>,
    outcome: Option<Res<Outcome>>,
    mut ships: Query<(Option<&Ship>, Option<&Health>, Option<&GunStates>, &mut Crew)>,
    mut hands: Query<(&mut Hand, &mut Transform, &mut MeshTag)>,
) {
    let Some(kit) = kit else { return };
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let now = stamp(&time);
    let won = outcome.is_some_and(|o| *o == Outcome::Victory);
    for (ship, health, guns, mut crew) in &mut ships {
        let crew = &mut *crew;
        let reloads: Vec<f32> = guns.map_or(vec![], |g| g.reloads().map(|(_, c)| c).collect());
        if reloads.iter().any(|&c| c > 0.0) {
            crew.alarm = ALARM;
        }
        crew.alarm = (crew.alarm - dt).max(0.0);
        let cheer = won && ship.is_none_or(|s| s.side == Side::Player);

        // the headcount follows the hull: a share stays standing till the end, all fall when the ship goes
        let alive: Vec<Entity> = crew.hands.iter().copied().filter(|&h| hands.get(h).is_ok_and(|(h, _, _)| h.fallen.is_none())).collect();
        let (core, destroyed) = health.map_or((crew.max_core, false), |h| (h.core, h.destroyed));
        let share = (core / crew.max_core).clamp(0.0, 1.0);
        let target = if destroyed { 0 } else { (crew.total as f32 * (STANDING_MIN + (1.0 - STANDING_MIN) * share)).ceil() as usize };
        crew.kill_wait -= dt;
        if alive.len() > target && (crew.kill_wait <= 0.0 || destroyed) {
            let n = if destroyed { alive.len() } else { 1 };
            for _ in 0..n {
                let i = (rand(&mut crew.rng) * alive.len() as f32) as usize % alive.len();
                if let Ok((mut h, _, _)) = hands.get_mut(alive[i])
                    && h.fallen.is_none()
                {
                    h.fallen = Some(0.0);
                    h.path.clear();
                    h.started = now;
                    h.phase = 0;
                    h.clip = FALL;
                }
            }
            crew.kill_wait = KILL_WAIT_MIN + rand(&mut crew.rng) * KILL_WAIT_SPAN;
        }

        // sails fold or open: riggers go aloft, one per shroud, taken from the idle wanderers
        let furled = ship.is_some_and(|s| s.ordered() == 0.0);
        if furled != crew.furled {
            crew.furled = furled;
            let mut free: Vec<Entity> = alive.iter().copied().filter(|&h| hands.get(h).is_ok_and(|(h, _, _)| h.role == Role::Wander)).collect();
            for climb in 0..crew.places.climbs.len() {
                let Some(e) = free.pop() else { break };
                if let Ok((mut h, _, _)) = hands.get_mut(e) {
                    h.role = Role::Rigger { climb, stage: 0 };
                    h.path = crew.places.route(h.pos, crew.places.climbs[climb].0);
                    h.running = true;
                    h.climbing = false;
                }
            }
        }

        let places = &crew.places;
        let rng = &mut crew.rng;
        crew.hands.retain(|&e| {
            let Ok((mut h, mut t, mut tag)) = hands.get_mut(e) else { return false };
            let h = &mut *h;
            let mut clip;
            if let Some(f) = h.fallen.as_mut() {
                *f += dt;
                if *f > LIE + kit.clips[FALL as usize].seconds {
                    commands.entity(e).despawn();
                    return false;
                }
                return true;
            }
            if cheer {
                h.path.clear();
                clip = CHEER;
            } else if let Some(&next) = h.path.front() {
                let speed = if h.climbing {
                    kit.climb
                } else if h.running || crew.alarm > 0.0 {
                    kit.run
                } else {
                    kit.walk
                };
                let to = next - h.pos;
                let step = speed * dt;
                if !h.climbing && to.xz().length() > 0.05 {
                    h.face = Quat::from_rotation_y((-to.x).atan2(-to.z));
                }
                if to.length() <= step {
                    h.pos = next;
                    h.path.pop_front();
                } else {
                    h.pos += to / to.length() * step;
                }
                clip = if h.climbing {
                    CLIMB
                } else if speed >= kit.run {
                    RUN
                } else {
                    WALK
                };
            } else {
                h.running = false;
                clip = IDLE;
                match h.role {
                    Role::Gun { place, mount, slot } => {
                        let home = places.guns[place];
                        if h.pos.distance(home.translation) > 0.1 {
                            h.path = places.route(h.pos, home.translation);
                            h.running = crew.alarm > 0.0;
                        } else {
                            h.face = home.rotation;
                            if reloads.get(mount).is_some_and(|&c| c > 0.0) {
                                clip = if slot < 2 { LOAD } else { HAUL };
                            }
                        }
                    }
                    Role::Post { place } => {
                        let home = places.posts[place].1;
                        if h.pos.distance(home.translation) > 0.1 {
                            h.path = places.route(h.pos, home.translation);
                        } else {
                            h.face = home.rotation;
                        }
                    }
                    Role::Work { place } => {
                        let home = places.work[place];
                        if h.pos.distance(home.translation) > 0.1 {
                            h.path = places.route(h.pos, home.translation);
                        } else {
                            h.face = home.rotation;
                            clip = if place % 2 == 0 { LOAD } else { HAUL };
                        }
                    }
                    Role::Wander => {
                        h.wait -= dt;
                        if h.wait <= 0.0 && !places.walk.is_empty() {
                            let i = (rand(rng) * places.walk.len() as f32) as usize % places.walk.len();
                            h.path = places.route(h.pos, places.walk[i]);
                            h.wait = 2.0 + rand(rng) * 8.0;
                        }
                    }
                    Role::Rigger { climb, stage } => {
                        let (foot, top, face) = places.climbs[climb];
                        h.face = face;
                        match stage {
                            0 => {
                                h.climbing = true;
                                h.path = VecDeque::from([top]);
                                h.role = Role::Rigger { climb, stage: 1 };
                            }
                            1 => {
                                h.climbing = false;
                                h.wait = 3.0 + rand(rng) * 3.0;
                                h.role = Role::Rigger { climb, stage: 2 };
                            }
                            2 => {
                                clip = HAUL;
                                h.wait -= dt;
                                if h.wait <= 0.0 {
                                    h.climbing = true;
                                    h.path = VecDeque::from([foot]);
                                    h.role = Role::Rigger { climb, stage: 3 };
                                }
                            }
                            _ => {
                                h.climbing = false;
                                h.wait = rand(rng) * 4.0;
                                h.role = Role::Wander;
                            }
                        }
                    }
                }
            }
            if clip != h.clip {
                h.clip = clip;
                h.started = now;
                *tag = h.tag();
            }
            t.translation = h.pos;
            t.rotation = t.rotation.slerp(h.face, (dt * 10.0).min(1.0));
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_kit_header_leaves_deck_crews_off_without_panicking() {
        let root = std::env::temp_dir().join(format!("procyon-kit-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let head = root.join(DATA);
        std::fs::create_dir_all(head.parent().unwrap()).unwrap();
        std::fs::write(head.with_extension("bin"), []).unwrap();
        // not JSON, JSON of another shape, and a clip list longer than the eight the shader has room for
        for text in ["{ clips", r#"["verts"]"#, r#"{"verts": 0, "frames": 0, "part": [], "clips": [{"name": "idle"}, {"name": "walk"}, {"name": "run"}, {"name": "haul"}, {"name": "load"}, {"name": "climb"}, {"name": "cheer"}, {"name": "fall"}, {}]}"#] {
            std::fs::write(&head, text).unwrap();
            assert!(read_kit(&root).is_none(), "{text}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn node(name: &str, x: f32, z: f32) -> (String, Transform) {
        (name.into(), Transform::from_xyz(x, 0.0, z))
    }

    #[test]
    fn lanes_join_along_and_across_and_route_through_them() {
        let p = Places::from_nodes(&[
            node("Crew_Walk_P_0", -2.0, 0.0),
            node("Crew_Walk_P_1", -2.0, -5.0),
            node("Crew_Walk_P_2", -2.0, -10.0),
            node("Crew_Walk_X0_0", 0.0, -10.0),
            node("Crew_Walk_S_0", 2.0, -10.0),
            node("Crew_Climb_Main_P_a", -6.0, 0.0),
            node("Crew_Climb_Main_P_b", -2.0, 0.0),
            node("Crew_Gun_0_0", 1.0, 1.0),
            node("Crew_Work_Crates_0", 3.0, 1.0),
        ]);
        assert_eq!(p.walk.len(), 5);
        assert_eq!(p.work.len(), 1);
        assert_eq!(p.climbs.len(), 1);
        assert_eq!(p.guns.len(), 1);
        let r: Vec<Vec3> = p.route(Vec3::new(-2.0, 0.0, 0.5), Vec3::new(2.0, 0.0, -10.5)).into();
        assert_eq!(r.len(), 6, "{r:?}");
        assert_eq!(r[3], Vec3::new(0.0, 0.0, -10.0));
    }

    #[test]
    fn the_headcount_grows_with_the_deck_and_a_node_overrides_it() {
        // decks (length x beam) from a rowing-boat size to a ship of the line
        let n: Vec<usize> = [0.0, 15.0, 26.0, 48.0, 60.0, 115.0, 186.0, 1200.0].map(headcount).into();
        assert_eq!(n, [1, 1, 2, 5, 7, 19, 40, 676]);
        assert_eq!(Places::from_nodes(&[node("Crew_Count_3", 0.0, 0.0)]).count, Some(3));
        assert_eq!(Places::from_nodes(&[node("Crew_Count_12_x", 0.0, 0.0)]).count, Some(12));
        assert_eq!(Places::from_nodes(&[node("Crew_Walk_P_0", 0.0, 0.0)]).count, None);
    }

    #[test]
    fn a_short_hop_goes_straight() {
        let p = Places::from_nodes(&[node("Crew_Walk_P_0", -2.0, 0.0), node("Crew_Walk_P_1", -2.0, -20.0)]);
        assert_eq!(p.route(Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.5, 0.0, -10.0)).len(), 1);
    }

    #[test]
    fn half_floats_decode() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert!((f16_to_f32(0x3555) - 0.333).abs() < 0.001);
    }

    #[test]
    fn the_block_parses_outfits() {
        let d = parse(&serde_json::json!({"per_ship": 40, "outfits": [{"race": "Navy", "officer": true, "coat": [0.1, 0.2, 0.5]}]})).unwrap();
        assert_eq!(d.per_ship, 40);
        assert!(d.outfits[0].officer);
        assert_eq!(d.outfits[0].colors[1], Vec3::new(0.1, 0.2, 0.5));
        assert!(parse(&Value::Null).is_none());
    }
}
