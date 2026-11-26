//! Not retail: deck crews of the render profile (part of render/mod.rs's look). Small figures that man the deck guns, keep watch,
//! wander the decks, go aloft when the sails fold or open, fall as the hull is shot to pieces and cheer a victory. Presentation only:
//! they read the battle (gun reloads, orders, hull, outcome), never change it, and use their own random numbers.
//!
//! - The figures and their moves come from the mod: `data/render/deckhand.json` (bake format version 2), `deckhand.bin` (a bone-transform
//!   texture: a quaternion and a translation per bone per frame) and `deckhand_mesh.bin` (each figure's rest-pose mesh, one per level of
//!   detail, with bone index and colour slot per vertex), built by the expansion's `tools/build_deckhand.py`. The GPU poses every vertex from
//!   its bone's transform (`deckhand_vat.wesl`; the engine's vertex stages `deckhand_vertex.wesl` and `deckhand_prepass.wesl`, the mod's
//!   `shaders/deckhands.wesl` shades). One material serves every figure and there is one mesh per figure and level of detail, so a whole
//!   battle's crews are a handful of batches; what each figure plays and wears is its `MeshTag` (clip, outfit, phase, start). Clips are found by
//!   name (`idle`, `walk`, `run`, `haul`, `load`, `climb`, `cheer`, `fall` are required; `talk` and `carry` fall back to `idle` and `walk`).
//!   An outfit names its `figure` (the first when it does not), so a race's crew is a mix of figures through its outfits. Far figures swap to
//!   the figure's second mesh (a box per bone) by camera distance, checked a few times a second and staggered over the frames.
//! - Where they go comes from the ship's model: empty nodes named `Crew_<Kind>_...` (`Walk` lanes, `Gun` places by a deck mount,
//!   `Post`s, `Climb` pairs `_a` at the foot of a shroud and `_b` up it). A ship without them gets no crew. How many wander grows
//!   with the deck (`headcount`, from the collision box's length times beam: 1-2 on a fishing boat, 40 on a Man-o'-War, 60 on a first rate)
//!   unless a `Crew_Count_<n>` node sets the total. The same nodes may come from JSON (a ship's `crew_nodes`, scenery's
//!   `data/crew_nodes.json`; `data/crew_nodes.rs`): `gather` combines them with the model's, a node of the same name replacing it.
//! - Figures are children of the ship, so they ride its motion; they cast no shadows and vanish beyond `range`. A crew beyond `range` plus the
//!   fade is not stepped at all (the clips run on the GPU clock anyway); its figures carry on from where they stood when it comes back.
//! - Map scenery (a town, a pier) gets townsfolk the same way: `Crew_Walk` lanes, `Post`s, and `Work` places where a docker works on the spot
//!   and now and then carries a load to the nearest other work place. They wear the `Civilian` outfits, one wanders per three walk points (at
//!   most `per_scenery`), and they cheer a victory; nothing else reaches them (no hull, guns or sails).
//! - Behaviour (all presentation, own random numbers): a walking figure keeps to the right of its lane by its own offset and stops a little
//!   short of its walk point at a random spot; a wanderer that stops near another idle figure turns to it and talks for a while; a fallen
//!   figure sinks away in its last second instead of popping.
//!
//! PROVISIONAL (a prototype): straight-line walking along the lanes (no avoidance beyond the lane offset), no blending between clips;
//! the distances and times of the behaviour below are eyeballed.

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
use crate::Ship;
use crate::combat::xorshift_unit as rand;
use crate::combat::{GunStates, Health, Library};
use crate::scenario::Outcome;

/// The fragment shader; the mod ships it.
pub const SHADER: &str = "shaders/deckhands.wesl";
/// The figures and their baked moves (`deckhand.bin`, the pose texture, and `deckhand_mesh.bin`, the rest meshes, beside it); the mod ships them.
pub const DATA: &str = "data/render/deckhand.json";
const VERTEX_SHADER: &str = "embedded://openprocyon/render/deckhand_vertex.wesl";
const PREPASS_SHADER: &str = "embedded://openprocyon/render/deckhand_prepass.wesl";

/// The bake format this reads (`version` in `deckhand.json`).
const FORMAT: u64 = 2;
/// Most clips a bake may hold (the tag's 4 bits).
const MAX_CLIPS: usize = 16;
/// Colour slots a figure's vertices pick from, by name: an outfit's keys.
const SLOTS: [&str; 8] = ["skin", "coat", "trousers", "shoes", "hat", "trim", "metal", "hair"];
/// Most outfits (the tag's 5 bits).
const MAX_OUTFITS: usize = 32;
/// The clips the engine plays, in the shader's tag numbering; the first `REQUIRED` of them must be in `deckhand.json` (found by name).
const CLIPS: [&str; 10] = ["idle", "walk", "run", "haul", "load", "climb", "cheer", "fall", "talk", "carry"];
const REQUIRED: usize = 8;
const IDLE: u32 = 0;
const WALK: u32 = 1;
const RUN: u32 = 2;
const HAUL: u32 = 3;
const LOAD: u32 = 4;
const CLIMB: u32 = 5;
const CHEER: u32 = 6;
const FALL: u32 = 7;
const TALK: u32 = 8;
const CARRY: u32 = 9;
/// What an optional clip plays when the bake lacks it: (clip, stand-in).
const FALLBACK: [(u32, u32); 2] = [(TALK, IDLE), (CARRY, WALK)];
/// Walk points closer than this join, whichever lanes they are on.
const LINK: f32 = 2.3;
/// Seconds a fallen figure lies on deck before it goes.
const LIE: f32 = 12.0;
/// Seconds at the end of that, over which a fallen figure sinks and shrinks away. Engine choice.
const SINK: f32 = 1.0;
/// Depth (units) a fallen figure has sunk by when it is gone. Engine choice.
const SINK_DEPTH: f32 = 0.12;
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
/// Camera distance (units) beyond which a figure wears its second, boxy mesh, when `deckhands.lod_distance` does not say. Engine choice: about
/// where a figure is 20 px tall on a 720p screen; PROVISIONAL, not compared with the box mesh's look at every zoom.
const LOD_DISTANCE: f32 = 45.0;
/// The swap distance is this much further out going to the far mesh and this much nearer coming back, so a figure at the edge does not flicker. Engine choice.
const LOD_HYSTERESIS: f32 = 0.1;
/// Each figure's level of detail is checked every this many frames (the crews' figures are spread over them). Engine choice.
const LOD_PERIOD: u32 = 8;
/// Heights a figure's scale is drawn from (1.0 is the baked height). Engine choice.
const HEIGHT_MIN: f32 = 0.94;
const HEIGHT_SPAN: f32 = 0.12;
/// A walking figure keeps this far (units, a random share of the span each) to the right of its lane. Engine choice.
const LANE_MIN: f32 = 0.12;
const LANE_SPAN: f32 = 0.25;
/// A figure ends a walk this far (units) around its walk point, at most, and a little short of it. Engine choice.
const STOP_SCATTER: f32 = 0.6;
/// A wanderer that stops within this distance (units) of another idle figure turns to it and talks. Engine choice.
const CHAT_REACH: f32 = 1.5;
/// Seconds a chat lasts: the least and the random extra. Engine choice.
const CHAT_MIN: f32 = 3.0;
const CHAT_SPAN: f32 = 5.0;
/// Seconds a docker works on the spot between carrying trips: the least and the random extra. Engine choice.
const WORK_MIN: f32 = 6.0;
const WORK_SPAN: f32 = 8.0;
/// Most figures on one piece of map scenery when `per_scenery` does not say. Engine choice.
const PER_SCENERY: usize = 150;

/// Figures on a ship whose model gives no `Crew_Count_<n>`, from its deck: the collision box's length times beam, in units^2. Crews
/// grow faster than decks (`area^1.52`), calibrated on retail hulls: a fishing boat (15-26) carries 1-2, a sloop or packet (24-48) 2-5,
/// a cutter (61) 7, a frigate (115) 19, a Man-o'-War (186) 40; a first rate reaches `per_ship` (60), which caps every ship. Gun places,
/// posts and work places are always manned, and a ship with shrouds keeps one wanderer per shroud to go aloft; the count only sizes the
/// wanderers.
fn headcount(deck: f32) -> usize {
    (deck.max(0.0) / HEADCOUNT_DECK_UNIT).powf(HEADCOUNT_EXPONENT).round().max(1.0) as usize
}

/// One outfit: the colours of a figure's slots (linear rgb), worn by a race's crews; `officer` outfits go to the posts.
#[derive(Clone, Debug, PartialEq)]
pub struct Outfit {
    pub race: String,
    pub officer: bool,
    /// The figure (by the bake's name) that wears it; None is the bake's first.
    pub figure: Option<String>,
    pub colors: [Vec3; SLOTS.len()],
}

/// The `deckhands` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Deckhands {
    /// Most figures on one ship, a ceiling: how many a ship carries comes from its deck (`headcount`) or its model's `Crew_Count_<n>`
    /// (gun places and posts fill first, the rest wander).
    pub per_ship: usize,
    /// Most figures on one piece of map scenery (a town), separate from `per_ship`.
    pub per_scenery: usize,
    /// Camera distance beyond which figures are not drawn.
    pub range: f32,
    /// Camera distance beyond which figures wear their far mesh.
    pub lod_distance: f32,
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
            let mut colors = [Vec3::splat(0.5); SLOTS.len()];
            for (i, k) in SLOTS.iter().enumerate() {
                if let Some(c) = rgb(&o[*k]) {
                    colors[i] = c;
                }
            }
            Some(Outfit { race: o["race"].as_str()?.to_string(), officer: o["officer"].as_bool().unwrap_or(false), figure: o["figure"].as_str().map(String::from), colors })
        })
        .take(MAX_OUTFITS)
        .collect();
    Some(Deckhands {
        per_ship: num(&v["per_ship"], 60.0).max(0.0) as usize,
        per_scenery: num(&v["per_scenery"], PER_SCENERY as f32).max(0.0) as usize,
        range: num(&v["range"], 160.0).max(1.0),
        lod_distance: num(&v["lod_distance"], LOD_DISTANCE).max(1.0),
        outfits,
    })
}

pub fn plugin(app: &mut App, look: &Look) {
    if look.deckhands.is_some() {
        bevy::shader::load_shader_library!(app, "deckhand_vat.wesl");
        bevy::asset::embedded_asset!(app, "deckhand_vertex.wesl");
        bevy::asset::embedded_asset!(app, "deckhand_prepass.wesl");
        app.add_plugins(MaterialPlugin::<DeckhandMaterial>::default()).add_systems(Startup, load_kit).add_systems(Update, (gather, staff, drive, lod).chain());
    }
}

/// What the vertex stages get at `@group(MATERIAL_BIND_GROUP) @binding(100)` (`struct Deckhand` in deckhand_vat.wesl).
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct DeckhandUniform {
    /// Per clip (the tag's numbering): x first row, y frames, z frames a second, w 1 when it loops.
    pub clips: [Vec4; MAX_CLIPS],
    /// rgb per outfit and slot: `outfit * 8 + slot`.
    pub palette: [Vec4; MAX_OUTFITS * SLOTS.len()],
}

impl Default for DeckhandUniform {
    fn default() -> Self {
        DeckhandUniform { clips: [Vec4::ZERO; MAX_CLIPS], palette: [Vec4::ONE; MAX_OUTFITS * SLOTS.len()] }
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
pub struct DeckhandExt {
    #[uniform(100)]
    pub u: DeckhandUniform,
    /// The frame data (`frame.rs`) at `@binding(99)`: the battle clock the moves run on.
    #[storage(99, read_only)]
    pub frame: super::frame::FrameBinding,
    /// The baked bone transforms (Rgba16Float, read with textureLoad).
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

/// One figure of the bake: its meshes by level of detail (0 the full one) and its bounds.
struct FigureKit {
    lods: Vec<Handle<Mesh>>,
    aabb: Aabb,
}

/// The loaded figures: the shared material, clip lengths (by the tag's numbering), how fast they move, and which figure each outfit wears.
#[derive(Resource)]
struct Kit {
    figures: Vec<FigureKit>,
    material: Handle<DeckhandMaterial>,
    clips: Vec<Clip>,
    walk: f32,
    run: f32,
    climb: f32,
    range: f32,
    per_ship: usize,
    per_scenery: usize,
    lod_distance: f32,
    outfits: Vec<Outfit>,
    /// Per outfit: the index in `figures`.
    outfit_figure: Vec<usize>,
}

/// A clip of the bake's header.
#[derive(Clone, Debug, PartialEq)]
struct ClipDef {
    name: String,
    start: usize,
    frames: usize,
    looped: bool,
    seconds: f32,
}

/// A figure of the bake's header: its rig, bounds and meshes (`(vertices, byte offset)` per level of detail).
#[derive(Clone, Debug, PartialEq)]
struct FigureDef {
    name: String,
    rig: usize,
    bounds: [Vec3; 2],
    lods: Vec<(usize, usize)>,
}

/// `deckhand.json` (format 2), checked.
#[derive(Clone, Debug, PartialEq)]
struct Header {
    fps: f32,
    /// Units a second the walk, run and climb clips cover (the baker's `speeds`).
    speeds: [f32; 3],
    clips: Vec<ClipDef>,
    /// Per rig: its bone count and the first texel of its row.
    rigs: Vec<(usize, usize)>,
    figures: Vec<FigureDef>,
    width: usize,
    height: usize,
}

/// The header read and checked against itself (the file sizes are `read_kit`'s).
fn parse_header(head: &Value) -> Result<Header, String> {
    if head["version"].as_u64() != Some(FORMAT) {
        return Err(format!("not bake format {FORMAT} (rebuild it with the expansion's tools/build_deckhand.py)"));
    }
    let (width, height) = (head["pose"]["width"].as_u64().unwrap_or(0) as usize, head["pose"]["height"].as_u64().unwrap_or(0) as usize);
    let parts: Vec<&str> = head["parts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if parts != SLOTS {
        return Err(format!("`parts` is {parts:?}, not {SLOTS:?}"));
    }
    let mut clips = vec![];
    for c in head["clips"].as_array().ok_or("no `clips`")? {
        let def = ClipDef {
            name: c["name"].as_str().ok_or("a clip without a name")?.to_string(),
            start: c["start"].as_u64().ok_or("a clip without `start`")? as usize,
            frames: c["frames"].as_u64().ok_or("a clip without `frames`")? as usize,
            looped: c["loop"].as_bool().unwrap_or(true),
            seconds: num(&c["seconds"], 1.0),
        };
        if def.frames == 0 || def.start + def.frames > height {
            return Err(format!("clip `{}` lies outside the pose texture's {height} rows", def.name));
        }
        clips.push(def);
    }
    if clips.len() > MAX_CLIPS {
        return Err(format!("{} clips, at most {MAX_CLIPS}", clips.len()));
    }
    let mut rigs = vec![];
    let mut rig_names = vec![];
    for r in head["rigs"].as_array().ok_or("no `rigs`")? {
        let (bones, column) = (r["bones"].as_u64().unwrap_or(0) as usize, r["column"].as_u64().unwrap_or(0) as usize);
        if bones == 0 || column + 2 * bones > width {
            return Err(format!("rig {:?} lies outside the pose texture's {width} columns", r["name"]));
        }
        rig_names.push(r["name"].as_str().unwrap_or(""));
        rigs.push((bones, column));
    }
    let mut figures = vec![];
    for f in head["figures"].as_array().ok_or("no `figures`")? {
        let name = f["name"].as_str().ok_or("a figure without a name")?.to_string();
        let rig = rig_names.iter().position(|r| Some(*r) == f["rig"].as_str()).ok_or_else(|| format!("figure `{name}`: unknown rig {:?}", f["rig"]))?;
        let lods: Vec<(usize, usize)> =
            f["lods"].as_array().into_iter().flatten().map(|l| (l["verts"].as_u64().unwrap_or(0) as usize, l["offset"].as_u64().unwrap_or(0) as usize)).collect();
        if lods.is_empty() || lods.iter().any(|l| l.0 == 0 || l.0 % 3 != 0) {
            return Err(format!("figure `{name}`: no mesh, or a vertex count that is not whole triangles"));
        }
        let v = |k: usize| Vec3::new(num(&f["bounds"][k][0], -1.0), num(&f["bounds"][k][1], 0.0), num(&f["bounds"][k][2], -1.0));
        figures.push(FigureDef { name, rig, bounds: [v(0), v(1)], lods });
    }
    if figures.is_empty() {
        return Err("no figures".into());
    }
    let speeds = [num(&head["speeds"]["walk"], 1.3), num(&head["speeds"]["run"], 3.6), num(&head["speeds"]["climb"], 0.9)];
    Ok(Header { fps: num(&head["fps"], 15.0), speeds, clips, rigs, figures, width, height })
}

/// For each of the engine's clips (`CLIPS`, the tag's numbering) the index of the bake's clip that plays it: by name, the optional ones
/// falling back (`talk` to `idle`, `carry` to `walk`). Err names a missing required clip.
fn resolve_clips(clips: &[ClipDef]) -> Result<Vec<usize>, String> {
    let find = |name: &str| clips.iter().position(|c| c.name == name);
    let mut out = vec![];
    for (i, name) in CLIPS.iter().enumerate() {
        let found = find(name).or_else(|| {
            let stand_in = FALLBACK.iter().find(|f| f.0 as usize == i)?.1 as usize;
            out.get(stand_in).copied()
        });
        out.push(found.ok_or_else(|| format!("no clip `{name}`"))?);
    }
    debug_assert!(out.len() > REQUIRED);
    Ok(out)
}

/// The header, the pose texels and the meshes, read and checked: None (with a warning) leaves the block off.
fn read_kit(root: &std::path::Path) -> Option<(Header, Vec<u8>, Vec<u8>)> {
    let json: Value = crate::data::load_or_base(&root.join(DATA), None, "deck crews are left off")?;
    let fail = |why: String| {
        warn!("render profile: {DATA}: {why}; deck crews left off");
        None
    };
    let head = match parse_header(&json) {
        Ok(h) => h,
        Err(why) => return fail(why),
    };
    let Ok(pose) = std::fs::read(root.join(DATA).with_extension("bin")) else { return fail("no .bin beside it".into()) };
    let Ok(mesh) = std::fs::read(root.join(DATA).with_file_name("deckhand_mesh.bin")) else { return fail("no deckhand_mesh.bin beside it".into()) };
    if pose.len() != head.width * head.height * 8 {
        return fail(format!("the .bin holds {} bytes, not {}x{} texels", pose.len(), head.width, head.height));
    }
    if head.figures.iter().any(|f| f.lods.iter().any(|&(v, o)| o + v * MESH_VERTEX_BYTES > mesh.len())) {
        return fail("deckhand_mesh.bin is shorter than its meshes".into());
    }
    if let Err(why) = resolve_clips(&head.clips) {
        return fail(why);
    }
    Some((head, pose, mesh))
}

/// Bytes a vertex takes in `deckhand_mesh.bin`: position, normal, bone index and colour slot, as f32.
const MESH_VERTEX_BYTES: usize = 8 * 4;

/// One level of detail of a figure, from `deckhand_mesh.bin`. `column` is the figure's rig's first texel: a vertex's bone becomes the texel of
/// its rotation, as the vertex stages read it from `uv.x`; its colour slot becomes `color.r` (a share of 8).
fn build_mesh(bytes: &[u8], verts: usize, column: usize) -> Mesh {
    let f = |i: usize, k: usize| f32::from_le_bytes(bytes[i * MESH_VERTEX_BYTES + k * 4..][..4].try_into().unwrap());
    let pos: Vec<[f32; 3]> = (0..verts).map(|i| [f(i, 0), f(i, 1), f(i, 2)]).collect();
    let nor: Vec<[f32; 3]> = (0..verts).map(|i| [f(i, 3), f(i, 4), f(i, 5)]).collect();
    let uv: Vec<[f32; 2]> = (0..verts).map(|i| [column as f32 + 2.0 * f(i, 6), 0.0]).collect();
    let col: Vec<[f32; 4]> = (0..verts).map(|i| [f(i, 7) / SLOTS.len() as f32, 0.0, 0.0, 1.0]).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32((0..verts as u32).collect()));
    mesh
}

fn load_kit(mut commands: Commands, look: Res<Look>, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<DeckhandMaterial>>) {
    let Some(d) = look.deckhands.as_ref() else { return };
    commands.insert_resource(SceneryCrews(crate::data::crew_nodes::load_scenery(crate::assets_dir())));
    let Some((head, pose, mesh_bytes)) = read_kit(crate::assets_dir()) else {
        warn!("render profile: `deckhands` needs {DATA} (format {FORMAT}) with its .bin and deckhand_mesh.bin in the mod; left off");
        return;
    };
    let figures: Vec<FigureKit> = head
        .figures
        .iter()
        .map(|f| FigureKit {
            lods: f.lods.iter().map(|&(verts, offset)| meshes.add(build_mesh(&mesh_bytes[offset..], verts, head.rigs[f.rig].1))).collect(),
            aabb: Aabb::from_min_max(f.bounds[0], f.bounds[1]),
        })
        .collect();
    let vat = Image::new(
        Extent3d { width: head.width as u32, height: head.height as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pose,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mut u = DeckhandUniform::default();
    let mut clips = vec![];
    for (i, &c) in resolve_clips(&head.clips).unwrap_or_default().iter().enumerate() {
        let c = &head.clips[c];
        u.clips[i] = Vec4::new(c.start as f32, c.frames as f32, head.fps, f32::from(u8::from(c.looped)));
        clips.push(Clip { seconds: c.seconds });
    }
    let outfit_figure: Vec<usize> = d
        .outfits
        .iter()
        .map(|o| {
            o.figure.as_ref().map_or(0, |name| {
                head.figures.iter().position(|f| &f.name == name).unwrap_or_else(|| {
                    warn!("render profile: an outfit wears figure `{name}`, which {DATA} lacks; it gets `{}`", head.figures[0].name);
                    0
                })
            })
        })
        .collect();
    for (o, outfit) in d.outfits.iter().enumerate() {
        for (p, c) in outfit.colors.iter().enumerate() {
            u.palette[o * SLOTS.len() + p] = c.extend(1.0);
        }
    }
    let material = materials.add(ExtendedMaterial {
        base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() },
        extension: DeckhandExt { u, frame: default(), vat: images.add(vat) },
    });
    commands.insert_resource(Kit {
        figures,
        material,
        clips,
        walk: head.speeds[0],
        run: head.speeds[1],
        climb: head.speeds[2],
        range: d.range,
        per_ship: d.per_ship,
        per_scenery: d.per_scenery,
        lod_distance: d.lod_distance,
        outfits: d.outfits.clone(),
        outfit_figure,
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
    /// The scene root the figures hang under (their frame), and a sphere around every place of the crew in it: the crew is not stepped
    /// while the camera is further from the sphere than `range` and the fade.
    frame: Entity,
    centre: Vec3,
    radius: f32,
    /// Figures at the start; the headcount falls from it with the hull.
    total: usize,
    /// Of `hands`, how many are lying on the deck.
    fallen: usize,
    max_core: f32,
    furled: bool,
    alarm: f32,
    kill_wait: f32,
    rng: u64,
    /// The gun mounts' reload times of this frame (kept to reuse its memory).
    reloads: Vec<f32>,
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
    /// A work place's index in `Places::work`: he works there on the spot (hauls at odd places, loads at even ones) and now and then carries a load to the nearest other one and back.
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

/// Where a docker is on a carrying trip.
#[derive(Clone, Copy, PartialEq)]
enum Errand {
    None,
    /// Walking to the other work place with a load.
    Out,
    /// Walking home with it.
    Back,
}

#[derive(Component)]
struct Hand {
    role: Role,
    pos: Vec3,
    face: Quat,
    path: VecDeque<Vec3>,
    /// Where the walk along `path` last set off or passed a point (the lane's direction is from it to the next point).
    from: Vec3,
    /// What the path is for: run, or climb (faces the shroud).
    running: bool,
    climbing: bool,
    /// He was walking last frame (so a stop is noticed once).
    moving: bool,
    carry: bool,
    errand: Errand,
    wait: f32,
    /// Seconds of a chat still to go.
    chat: f32,
    clip: u32,
    outfit: u32,
    phase: u32,
    started: u32,
    /// The figure (index in `Kit::figures`) and which of its meshes he wears now.
    figure: usize,
    lod: usize,
    /// Scale of this one (1.0 is the baked height), and how far (units) he walks to the right of his lane.
    height: f32,
    side: f32,
    /// Seconds since he fell; None while alive.
    fallen: Option<f32>,
}

/// A figure's `MeshTag`: bits 0-3 the clip, 4-8 the outfit, 9-15 the phase (a share of the clip, 1/128), 16-31 when the clip started (battle
/// seconds x 10, wrapping at 65536). `deckhand_vat.wesl` reads it back.
fn pack_tag(clip: u32, outfit: u32, phase: u32, started: u32) -> u32 {
    (clip & 15) | ((outfit & 31) << 4) | ((phase & 127) << 9) | ((started & 0xffff) << 16)
}

impl Hand {
    fn tag(&self) -> MeshTag {
        MeshTag::new(pack_tag(self.clip, self.outfit, self.phase, self.started))
    }

    /// Sets off along a route to `to`; with a random number generator he ends it at a random spot a little short of `to`.
    fn go(&mut self, places: &Places, to: Vec3, scatter: Option<&mut u64>) {
        let mut path = places.route(self.pos, to);
        if let Some(rng) = scatter
            && let Some(last) = path.back().copied()
        {
            let before = if path.len() > 1 { path[path.len() - 2] } else { self.pos };
            path.pop_back();
            path.push_back(stop_spot(last, before, rng));
        }
        self.path = path;
        self.from = self.pos;
    }
}

/// A spot around `to` (within `STOP_SCATTER`, and nearer the way he comes from, `before`) to stop at, so two never stand in one place.
fn stop_spot(to: Vec3, before: Vec3, rng: &mut u64) -> Vec3 {
    let a = rand(rng) * std::f32::consts::TAU;
    let r = rand(rng).sqrt() * STOP_SCATTER;
    let back = (before - to).with_y(0.0).normalize_or_zero() * rand(rng) * STOP_SCATTER * 0.5;
    to + Vec3::new(a.cos() * r, 0.0, a.sin() * r) + back
}

/// The point a walker aims for on the way to `next`: `side` units to the right of the lane `from` -> `next` (forward is -z, right +x).
fn lane_goal(from: Vec3, next: Vec3, side: f32) -> Vec3 {
    let d = (next - from).xz().normalize_or_zero();
    next + Vec3::new(-d.y, 0.0, d.x) * side
}

/// The battle clock in tag units (tenths of a second, wrapping), as the shader reads `frame.clock.y`.
fn stamp(time: &Time) -> u32 {
    ((time.elapsed_secs_f64() * 10.0) as u64 & 0xffff) as u32
}

/// The crew places of map scenery by type (`data/crew_nodes.json`), read with the kit.
#[derive(Resource, Default)]
struct SceneryCrews(HashMap<String, Vec<(String, Transform)>>);

/// Collects the `Crew_*` nodes of a ship's (or a map scenery object's) model as its scene spawns, plus the ones its JSON gives (the
/// class's `crew_nodes`, the scenery type's entry of `data/crew_nodes.json`), a JSON node replacing a model node of its name. They
/// hang under the scene's root (a child of the ship), whose frame is the model's; the figures go under the same root, so the nodes'
/// own transforms place them. A model with no `Crew_*` nodes of its own is framed by the first node its scene spawns.
#[allow(clippy::type_complexity)]
fn gather(
    mut commands: Commands,
    nodes: Query<(Entity, &Name, &ChildOf, &Transform), Added<Name>>,
    ships: Query<(), (Or<(With<Ship>, With<crate::clips::SceneLoop>)>, Without<Crew>, Without<CrewNodes>)>,
    classes: Query<&Ship>,
    types: Query<&crate::data::crew_nodes::SceneryType>,
    lib: Option<Res<Library>>,
    scenery: Option<Res<SceneryCrews>>,
    parents: Query<&ChildOf>,
) {
    let mut found: HashMap<Entity, (Entity, Vec<(String, Transform)>)> = HashMap::new();
    // the child of each ship the added nodes hang under: the frame of a model that places no crew of its own
    let mut roots: HashMap<Entity, Entity> = HashMap::new();
    for (e, name, parent, t) in &nodes {
        let mut below = e;
        let Some(ship) = parents.iter_ancestors(e).take(4).find(|&a| {
            let hit = ships.contains(a);
            if !hit {
                below = a;
            }
            hit
        }) else {
            continue;
        };
        if name.as_str().starts_with("Crew_") {
            found.entry(ship).or_insert((parent.parent(), vec![])).1.push((name.as_str().to_string(), *t));
        } else {
            roots.entry(ship).or_insert(below);
        }
    }
    for ship in roots.keys().chain(found.keys()).copied().collect::<std::collections::HashSet<_>>() {
        let json = match (classes.get(ship), types.get(ship), &lib, &scenery) {
            (Ok(s), _, Some(lib), _) => lib.classes.get(s.class).map(|c| c.crew_nodes.as_slice()),
            (_, Ok(t), _, Some(sc)) => sc.0.get(&t.0).map(Vec::as_slice),
            _ => None,
        };
        let Some(json) = json.filter(|j| !j.is_empty()) else { continue };
        let entry = found.entry(ship).or_insert_with(|| (roots[&ship], vec![]));
        entry.1 = crate::data::crew_nodes::combine(std::mem::take(&mut entry.1), json);
    }
    for (ship, (frame, nodes)) in found {
        commands.entity(ship).insert(CrewNodes { frame, nodes });
    }
}

/// A sphere (centre, radius) around every place of a crew, in its frame.
fn place_sphere(places: &Places) -> (Vec3, f32) {
    let points: Vec<Vec3> = places
        .walk
        .iter()
        .copied()
        .chain(places.guns.iter().map(|t| t.translation))
        .chain(places.posts.iter().map(|p| p.1.translation))
        .chain(places.work.iter().map(|t| t.translation))
        .chain(places.climbs.iter().flat_map(|c| [c.0, c.1]))
        .collect();
    if points.is_empty() {
        return (Vec3::ZERO, 0.0);
    }
    let (lo, hi) = points.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let centre = (lo + hi) / 2.0;
    (centre, points.iter().map(|p| p.distance(centre)).fold(0.0, f32::max) + 1.0)
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
        // a ship's crew is capped by `per_ship`, a piece of scenery's by `per_scenery`
        let cap = if ship.is_some() { kit.per_ship } else { kit.per_scenery };
        roles.truncate(cap);
        let target = places.count.unwrap_or_else(|| match ship {
            Some(s) => {
                let c = &lib.classes[s.class];
                headcount(c.length * (c.collision_bounds[1].x - c.collision_bounds[0].x))
            }
            None => roles.len() + places.walk.len() / 3,
        });
        // riggers go aloft from the wanderers: one per shroud stays on deck whatever the count
        let target = if ship.is_some() { target.max(roles.len() + places.climbs.len()) } else { target };
        let wanderers = target.min(cap).saturating_sub(roles.len());
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
                    (stop_spot(places.walk[i], places.walk[i], &mut rng), Quat::from_rotation_y(rand(&mut rng) * std::f32::consts::TAU))
                }
            };
            let officer = matches!(role, Role::Post { .. });
            let pick: Vec<u32> = wear.iter().filter(|(_, o)| *o == officer).map(|(i, _)| *i).collect();
            let pick = if pick.is_empty() { wear.iter().map(|(i, _)| *i).collect() } else { pick };
            let outfit = if pick.is_empty() { 0 } else { pick[(rand(&mut rng) * pick.len() as f32) as usize % pick.len()] };
            let figure = kit.outfit_figure.get(outfit as usize).copied().unwrap_or(0);
            let height = HEIGHT_MIN + rand(&mut rng) * HEIGHT_SPAN;
            let side = LANE_MIN + rand(&mut rng) * LANE_SPAN;
            let hand = Hand {
                role,
                pos,
                face,
                path: VecDeque::new(),
                from: pos,
                running: false,
                climbing: false,
                moving: false,
                carry: false,
                errand: Errand::None,
                wait: if matches!(role, Role::Work { .. }) { 1.0 + rand(&mut rng) * WORK_SPAN } else { rand(&mut rng) * MAX_IDLE_WAIT },
                chat: 0.0,
                clip: IDLE,
                outfit,
                phase: (rand(&mut rng) * 127.0) as u32,
                started: now,
                figure,
                lod: 0,
                height,
                side,
                fallen: None,
            };
            let tag = hand.tag();
            hands.push(
                commands
                    .spawn((
                        hand,
                        Transform::from_translation(pos).with_rotation(face).with_scale(Vec3::splat(height)),
                        Mesh3d(kit.figures[figure].lods[0].clone()),
                        MeshMaterial3d(kit.material.clone()),
                        tag,
                        kit.figures[figure].aabb,
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
        let (centre, radius) = place_sphere(&places);
        commands.entity(e).remove::<CrewNodes>().insert(Crew {
            places,
            hands,
            frame: nodes.frame,
            centre,
            radius,
            total,
            fallen: 0,
            max_core: health.map_or(1.0, |h| h.core.max(1.0)),
            furled: ship.is_some_and(|s| s.ordered() == 0.0),
            alarm: 0.0,
            kill_wait: 0.0,
            rng,
            reloads: vec![],
        });
    }
}

/// Whether the camera is within `reach` of the crew's places (the camera's position, the crew's frame): a crew beyond it is not stepped.
/// No camera counts as near.
fn within(camera: Option<Vec3>, frame: Option<&GlobalTransform>, crew: &Crew, reach: f32) -> bool {
    let (Some(cam), Some(frame)) = (camera, frame) else { return true };
    let scale = frame.affine().matrix3.x_axis.length().max(1e-3);
    cam.distance(frame.transform_point(crew.centre)) <= reach + crew.radius * scale
}

/// Each manned ship: who falls, whether the riggers go aloft, then every figure's next step and clip. A crew the camera is too far from to
/// see (`range` and the fade) is skipped: its figures stand where they were till it is near again.
#[allow(clippy::type_complexity)]
fn drive(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    time: Res<Time>,
    (outcome, table): (Option<Res<Outcome>>, crate::alliance::Hostility),
    camera: Option<Single<&GlobalTransform, (With<Camera3d>, With<crate::MainCamera>)>>,
    frames: Query<&GlobalTransform>,
    mut ships: Query<(Option<&Ship>, Option<&Health>, Option<&GunStates>, &mut Crew)>,
    mut hands: Query<(&mut Hand, &mut Transform, &mut MeshTag)>,
) {
    let Some(kit) = kit else { return };
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let cam = camera.map(|c| c.translation());
    let now = stamp(&time);
    let won = outcome.is_some_and(|o| *o == Outcome::Victory);
    for (ship, health, guns, mut crew) in &mut ships {
        let crew = &mut *crew;
        if !within(cam, frames.get(crew.frame).ok(), crew, kit.range + FADE_OUT_RANGE) {
            continue;
        }
        let mut reloads = std::mem::take(&mut crew.reloads);
        reloads.clear();
        if let Some(g) = guns {
            reloads.extend(g.reloads().map(|(_, c)| c));
        }
        if reloads.iter().any(|&c| c > 0.0) {
            crew.alarm = ALARM;
        }
        crew.alarm = (crew.alarm - dt).max(0.0);
        let cheer = won && ship.is_none_or(|s| table.with_human(s.side));

        // the headcount follows the hull: a share stays standing till the end, all fall when the ship goes
        let alive_count = crew.hands.len() - crew.fallen;
        let (core, destroyed) = health.map_or((crew.max_core, false), |h| (h.core, h.destroyed));
        let share = (core / crew.max_core).clamp(0.0, 1.0);
        let target = if destroyed { 0 } else { (crew.total as f32 * (STANDING_MIN + (1.0 - STANDING_MIN) * share)).ceil() as usize };
        crew.kill_wait -= dt;
        if alive_count > target && (crew.kill_wait <= 0.0 || destroyed) {
            let alive: Vec<Entity> = crew.hands.iter().copied().filter(|&h| hands.get(h).is_ok_and(|(h, _, _)| h.fallen.is_none())).collect();
            let n = if destroyed { alive.len() } else { 1 };
            for _ in 0..n {
                if alive.is_empty() {
                    break;
                }
                let i = (rand(&mut crew.rng) * alive.len() as f32) as usize % alive.len();
                if let Ok((mut h, _, _)) = hands.get_mut(alive[i])
                    && h.fallen.is_none()
                {
                    h.fallen = Some(0.0);
                    h.path.clear();
                    h.started = now;
                    h.phase = 0;
                    h.clip = FALL;
                    crew.fallen += 1;
                }
            }
            crew.kill_wait = KILL_WAIT_MIN + rand(&mut crew.rng) * KILL_WAIT_SPAN;
        }

        // sails fold or open: riggers go aloft, one per shroud, taken from the idle wanderers
        let furled = ship.is_some_and(|s| s.ordered() == 0.0);
        if furled != crew.furled {
            crew.furled = furled;
            let mut free: Vec<Entity> = crew.hands.iter().copied().filter(|&h| hands.get(h).is_ok_and(|(h, _, _)| h.fallen.is_none() && h.role == Role::Wander)).collect();
            for climb in 0..crew.places.climbs.len() {
                let Some(e) = free.pop() else { break };
                if let Ok((mut h, _, _)) = hands.get_mut(e) {
                    h.role = Role::Rigger { climb, stage: 0 };
                    h.path = crew.places.route(h.pos, crew.places.climbs[climb].0);
                    h.from = h.pos;
                    h.running = true;
                    h.climbing = false;
                    h.carry = false;
                    h.chat = 0.0;
                }
            }
        }

        let places = &crew.places;
        let rng = &mut crew.rng;
        let mut gone = 0;
        let mut stopped: Vec<(Entity, Vec3)> = vec![];
        crew.hands.retain(|&e| {
            let Ok((mut h, mut t, mut tag)) = hands.get_mut(e) else { return false };
            let h = &mut *h;
            let mut clip;
            if let Some(f) = h.fallen.as_mut() {
                *f += dt;
                let end = LIE + kit.clips[FALL as usize].seconds;
                if *f > end {
                    commands.entity(e).despawn();
                    gone += 1;
                    return false;
                }
                if *f > end - SINK {
                    // the last second: down and small, instead of popping
                    let k = (*f - (end - SINK)) / SINK;
                    t.translation = h.pos - Vec3::Y * SINK_DEPTH * k;
                    t.scale = Vec3::splat(h.height * (1.0 - k));
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
                h.moving = true;
                let goal = if h.climbing || h.path.len() == 1 { next } else { lane_goal(h.from, next, h.side) };
                let to = goal - h.pos;
                let step = speed * dt;
                if !h.climbing && to.xz().length() > 0.05 {
                    h.face = Quat::from_rotation_y(crate::geom::heading_of(to));
                }
                if to.length() <= step {
                    h.pos = goal;
                    h.from = next;
                    h.path.pop_front();
                } else {
                    h.pos += to / to.length() * step;
                }
                clip = if h.climbing {
                    CLIMB
                } else if speed >= kit.run {
                    RUN
                } else if h.carry {
                    CARRY
                } else {
                    WALK
                };
            } else {
                h.running = false;
                clip = IDLE;
                if h.moving {
                    h.moving = false;
                    if h.role == Role::Wander {
                        stopped.push((e, h.pos));
                    }
                }
                match h.role {
                    Role::Gun { place, mount, slot } => {
                        let home = places.guns[place];
                        if h.pos.distance(home.translation) > 0.1 {
                            h.go(places, home.translation, None);
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
                            h.go(places, home.translation, None);
                        } else {
                            h.face = home.rotation;
                        }
                    }
                    Role::Work { place } => {
                        let home = places.work[place];
                        if h.errand == Errand::Out {
                            // delivered: carry back
                            h.errand = Errand::Back;
                            h.go(places, home.translation, None);
                        } else {
                            if h.errand == Errand::Back {
                                h.errand = Errand::None;
                                h.carry = false;
                                h.wait = WORK_MIN + rand(rng) * WORK_SPAN;
                            }
                            if h.pos.distance(home.translation) > 0.1 {
                                h.go(places, home.translation, None);
                            } else {
                                h.face = home.rotation;
                                clip = if place % 2 == 0 { LOAD } else { HAUL };
                                h.wait -= dt;
                                if h.wait <= 0.0 {
                                    let other = (0..places.work.len()).filter(|&o| o != place).min_by(|&a, &b| {
                                        places.work[a].translation.distance_squared(home.translation).total_cmp(&places.work[b].translation.distance_squared(home.translation))
                                    });
                                    if let Some(o) = other {
                                        h.errand = Errand::Out;
                                        h.carry = true;
                                        h.go(places, places.work[o].translation, Some(&mut *rng));
                                    } else {
                                        h.wait = WORK_MIN;
                                    }
                                }
                            }
                        }
                    }
                    Role::Wander => {
                        h.wait -= dt;
                        if h.chat > 0.0 {
                            h.chat -= dt;
                            clip = TALK;
                        } else if h.wait <= 0.0 && !places.walk.is_empty() {
                            let i = (rand(rng) * places.walk.len() as f32) as usize % places.walk.len();
                            h.go(places, places.walk[i], Some(&mut *rng));
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
                                h.from = h.pos;
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
                                    h.from = h.pos;
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
            // write only what changed: a standing figure costs no transform propagation
            if t.translation != h.pos {
                t.translation = h.pos;
            }
            if t.rotation != h.face {
                let turned = t.rotation.slerp(h.face, (dt * 10.0).min(1.0));
                t.rotation = if turned.angle_between(h.face) < 1e-3 { h.face } else { turned };
            }
            true
        });
        crew.fallen -= gone;
        // wanderers that stopped by another idle figure turn to it and talk
        for (e, at) in stopped {
            let mate = crew
                .hands
                .iter()
                .copied()
                .filter(|&o| o != e)
                .filter_map(|o| {
                    let (h, _, _) = hands.get(o).ok()?;
                    let idle = h.role == Role::Wander && h.fallen.is_none() && h.path.is_empty() && h.chat <= 0.0 && !h.moving;
                    (idle && h.pos.xz().distance(at.xz()) < CHAT_REACH).then_some((h.pos, o))
                })
                .min_by(|a, b| a.0.xz().distance_squared(at.xz()).total_cmp(&b.0.xz().distance_squared(at.xz())));
            if let Some((p, o)) = mate {
                let secs = CHAT_MIN + rand(&mut crew.rng) * CHAT_SPAN;
                for (who, other) in [(e, p), (o, at)] {
                    if let Ok((mut h, _, _)) = hands.get_mut(who) {
                        let d = other - h.pos;
                        h.face = Quat::from_rotation_y(crate::geom::heading_of(d));
                        h.chat = secs;
                        h.wait = h.wait.max(secs);
                    }
                }
            }
        }
        crew.reloads = reloads;
    }
}

/// Far figures wear their figure's second mesh. Every figure is looked at once every `LOD_PERIOD` frames (a crew's figures spread over them),
/// with a margin either side of the distance so one at the edge does not flicker. The mesh handle is swapped on the figure's own entity.
#[allow(clippy::type_complexity)]
fn lod(
    mut frame: Local<u32>,
    kit: Option<Res<Kit>>,
    camera: Option<Single<&GlobalTransform, (With<Camera3d>, With<crate::MainCamera>)>>,
    frames: Query<&GlobalTransform>,
    crews: Query<&Crew>,
    mut hands: Query<(&mut Hand, &GlobalTransform, &mut Mesh3d)>,
) {
    let (Some(kit), Some(camera)) = (kit, camera) else { return };
    *frame = frame.wrapping_add(1);
    let cam = camera.translation();
    for crew in &crews {
        if !within(Some(cam), frames.get(crew.frame).ok(), crew, kit.range + FADE_OUT_RANGE) {
            continue;
        }
        for &e in crew.hands.iter().skip((*frame % LOD_PERIOD) as usize).step_by(LOD_PERIOD as usize) {
            let Ok((mut h, gt, mut mesh)) = hands.get_mut(e) else { continue };
            let lods = &kit.figures[h.figure].lods;
            let want = want_lod(h.lod, cam.distance(gt.translation()), kit.lod_distance, lods.len());
            if want != h.lod {
                h.lod = want;
                mesh.0 = lods[want].clone();
            }
        }
    }
}

/// The level of detail for a figure that wears `current` at `distance` from the camera: the far one (the last of `levels`) beyond `at`,
/// with `LOD_HYSTERESIS` of margin on whichever side he is on now.
fn want_lod(current: usize, distance: f32, at: f32, levels: usize) -> usize {
    let far = levels - 1;
    if far == 0 {
        return 0;
    }
    let edge = if current == 0 { at * (1.0 + LOD_HYSTERESIS) } else { at * (1.0 - LOD_HYSTERESIS) };
    if distance > edge { far } else { 0 }
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
        // not JSON, JSON of another shape, a version-1 header, and a clip list longer than the shader has room for
        let many = format!(r#"{{"version": {FORMAT}, "clips": [{}]}}"#, vec![r#"{"name": "x"}"#; MAX_CLIPS + 1].join(","));
        for text in ["clips", r#"["verts"]"#, r#"{"version": 1, "verts": 0, "frames": 0, "part": [], "clips": []}"#, many.as_str()] {
            std::fs::write(&head, text).unwrap();
            assert!(read_kit(&root).is_none(), "{text}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn node(name: &str, x: f32, z: f32) -> (String, Transform) {
        (name.into(), Transform::from_xyz(x, 0.0, z))
    }

    #[test]
    fn json_nodes_are_read_like_model_nodes_and_replace_those_of_their_name() {
        let model = vec![node("Crew_Walk_P_0", -2.0, 0.0), node("Crew_Walk_P_1", -2.0, -5.0), node("Crew_Gun_0", 1.0, 0.0)];
        let json = vec![node("Crew_Walk_P_1", -2.0, -9.0), node("Crew_Walk_P_2", -2.0, -10.0), node("Crew_Post_Captain", 0.0, 3.0)];
        let p = Places::from_nodes(&crate::data::crew_nodes::combine(model, &json));
        assert_eq!((p.walk.len(), p.guns.len(), p.posts.len()), (3, 1, 1));
        assert_eq!(p.walk[1].z, -9.0, "the JSON node replaced the model's");
        assert!(p.links[1].contains(&2) && p.links[0].contains(&1), "one lane across both sources");
    }

    /// A minimal v2 header: the eight required clips (and `extra`), one rig of 11 bones, one figure with two levels of detail.
    fn header(extra: &[&str]) -> Value {
        let names: Vec<&str> = CLIPS[..REQUIRED].iter().copied().chain(extra.iter().copied()).collect();
        let clips: Vec<Value> = names.iter().enumerate().map(|(i, n)| serde_json::json!({"name": n, "start": i * 10, "frames": 10, "loop": true, "seconds": 1.0})).collect();
        serde_json::json!({
            "version": 2, "fps": 15, "frames": 200, "clips": clips,
            "speeds": {"walk": 0.7, "run": 2.0, "climb": 0.5},
            "parts": SLOTS, "rigs": [{"name": "human", "bones": 11, "column": 0}],
            "figures": [{"name": "sailor", "rig": "human", "bounds": [[-0.3, 0.0, -0.6], [0.3, 1.2, 0.5]],
                         "lods": [{"verts": 1116, "offset": 0}, {"verts": 396, "offset": 35712}]}],
            "pose": {"width": 22, "height": 200}
        })
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
    fn the_block_parses_outfits_figures_and_caps() {
        let d = parse(&serde_json::json!({"per_ship": 40, "outfits": [
            {"race": "Navy", "officer": true, "coat": [0.1, 0.2, 0.5], "trim": [0.9, 0.8, 0.1], "figure": "officer"},
            {"race": "Navy"}]}))
        .unwrap();
        assert_eq!(d.per_ship, 40);
        assert_eq!(d.per_scenery, 150, "the scenery cap has its own default");
        assert_eq!(d.lod_distance, LOD_DISTANCE);
        assert!(d.outfits[0].officer);
        assert_eq!(d.outfits[0].figure.as_deref(), Some("officer"));
        assert_eq!(d.outfits[0].colors[1], Vec3::new(0.1, 0.2, 0.5));
        assert_eq!(d.outfits[0].colors[5], Vec3::new(0.9, 0.8, 0.1), "trim is slot 5");
        assert_eq!(d.outfits[1].figure, None);
        assert!(parse(&Value::Null).is_none());
        let d = parse(&serde_json::json!({"per_scenery": 20, "lod_distance": 30.0, "outfits": (0..40).map(|_| serde_json::json!({"race": "Navy"})).collect::<Vec<_>>()})).unwrap();
        assert_eq!((d.per_scenery, d.lod_distance, d.outfits.len()), (20, 30.0, MAX_OUTFITS));
    }

    #[test]
    fn the_header_parses_and_is_checked() {
        let h = parse_header(&header(&[])).unwrap();
        assert_eq!((h.width, h.height, h.clips.len(), h.rigs.clone()), (22, 200, 8, vec![(11, 0)]));
        assert_eq!(h.speeds, [0.7, 2.0, 0.5]);
        assert_eq!(h.figures[0].lods, vec![(1116, 0), (396, 35712)]);
        assert_eq!(h.figures[0].bounds[1], Vec3::new(0.3, 1.2, 0.5));
        let mut v1 = header(&[]);
        v1["version"] = 1.into();
        assert!(parse_header(&v1).unwrap_err().contains("format 2"));
        let mut bad = header(&[]);
        bad["figures"][0]["rig"] = "lizard".into();
        assert!(parse_header(&bad).unwrap_err().contains("unknown rig"));
        let mut bad = header(&[]);
        bad["clips"][7]["start"] = 195.into();
        assert!(parse_header(&bad).unwrap_err().contains("outside"));
        let mut bad = header(&[]);
        bad["rigs"][0]["bones"] = 12.into();
        assert!(parse_header(&bad).unwrap_err().contains("outside"));
        let mut bad = header(&[]);
        bad["parts"] = serde_json::json!(["skin"]);
        assert!(parse_header(&bad).is_err());
        let mut bad = header(&[]);
        bad["figures"][0]["lods"][1]["verts"] = 395.into();
        assert!(parse_header(&bad).is_err());
        let many: Vec<&str> = vec!["x"; MAX_CLIPS];
        let mut tall = header(&many);
        tall["pose"]["height"] = 1000.into();
        assert!(parse_header(&tall).unwrap_err().contains("at most"));
    }

    #[test]
    fn clips_are_found_by_name_and_optional_ones_fall_back() {
        let none = parse_header(&header(&[])).unwrap();
        let r = resolve_clips(&none.clips).unwrap();
        assert_eq!(r.len(), CLIPS.len());
        assert_eq!(r[TALK as usize], r[IDLE as usize], "talk falls back to idle");
        assert_eq!(r[CARRY as usize], r[WALK as usize], "carry falls back to walk");
        let both = parse_header(&header(&["talk", "carry"])).unwrap();
        let r = resolve_clips(&both.clips).unwrap();
        assert_eq!((r[TALK as usize], r[CARRY as usize]), (8, 9));
        // by name, not by position
        let mut shuffled = both.clips.clone();
        shuffled.swap(0, 7);
        let r = resolve_clips(&shuffled).unwrap();
        assert_eq!(shuffled[r[IDLE as usize]].name, "idle");
        assert_eq!(shuffled[r[FALL as usize]].name, "fall");
        // a required clip missing is an error naming it
        let mut missing = none.clips.clone();
        missing.retain(|c| c.name != "haul");
        assert!(resolve_clips(&missing).unwrap_err().contains("haul"));
    }

    #[test]
    fn the_tag_packs_clip_outfit_phase_and_start_in_their_bits() {
        let t = pack_tag(9, 31, 127, 0xabcd);
        assert_eq!(t & 15, 9);
        assert_eq!((t >> 4) & 31, 31);
        assert_eq!((t >> 9) & 127, 127);
        assert_eq!(t >> 16, 0xabcd);
        // fields do not spill into each other, and too-big values are cut to their bits
        assert_eq!(pack_tag(0, 1, 0, 0), 16);
        assert_eq!(pack_tag(0, 0, 1, 0), 512);
        assert_eq!(pack_tag(0, 0, 0, 1), 65536);
        assert_eq!(pack_tag(16, 32, 128, 0x10000), 0);
    }

    #[test]
    fn walkers_keep_to_the_right_of_their_lane() {
        // walking toward the bow (-z) the right hand is starboard (+x)
        let g = lane_goal(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0), 0.3);
        assert!((g - Vec3::new(0.3, 0.0, -10.0)).length() < 1e-5, "{g:?}");
        // the other way along the same lane is the opposite side: two opposite streams pass each other
        let back = lane_goal(Vec3::new(0.0, 0.0, -10.0), Vec3::ZERO, 0.3);
        assert!((back - Vec3::new(-0.3, 0.0, 0.0)).length() < 1e-5, "{back:?}");
        // along +x the right hand is +z (aft)
        let g = lane_goal(Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), 0.2);
        assert!((g - Vec3::new(5.0, 0.0, 0.2)).length() < 1e-5, "{g:?}");
        assert_eq!(lane_goal(Vec3::ONE, Vec3::ONE, 0.3), Vec3::ONE, "no direction, no offset");
    }

    #[test]
    fn stops_scatter_around_the_walk_point() {
        let mut rng = 12345u64;
        let to = Vec3::new(3.0, 1.0, -4.0);
        let spots: Vec<Vec3> = (0..50).map(|_| stop_spot(to, Vec3::ZERO, &mut rng)).collect();
        assert!(spots.iter().all(|s| s.y == to.y && s.distance(to) <= STOP_SCATTER * 1.5 + 1e-4));
        assert!(spots.windows(2).all(|w| w[0] != w[1]), "two never stand in one place");
    }

    #[test]
    fn the_far_mesh_has_a_margin_either_way() {
        let at = 45.0;
        assert_eq!(want_lod(0, 46.0, at, 2), 0, "inside the margin going out");
        assert_eq!(want_lod(0, 50.0, at, 2), 1);
        assert_eq!(want_lod(1, 44.0, at, 2), 1, "inside the margin coming back");
        assert_eq!(want_lod(1, 40.0, at, 2), 0);
        assert_eq!(want_lod(0, 1000.0, at, 1), 0, "one mesh only");
    }
}
