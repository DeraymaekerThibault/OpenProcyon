//! The console's second set of commands: mission flow (`rules`, `skip`, `nis end`, `win`, `lose`, `restart`), ships and inspection
//! (`ships`, `inspect`, `primary`, `damage`, `attack`, `stance`, `ally`, `hostile`, `tow`, `untow`), authoring helpers (`where`,
//! `mark`, `show` / `hide`) and camera and sound (`cam`, `hud`, `say`, `music`). Where a rule action exists the command builds it
//! and runs it through `Script::act`; the rest read the script's state or ask the ECS side (systems/dev_console.rs) through `Outcome`.

use std::collections::BTreeSet;
use std::io::Write as _;

use super::*;

/// What the drawing commands can show.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ShapeKind {
    Volumes,
    Paths,
    Points,
}
impl ShapeKind {
    pub const ALL: [(&'static str, ShapeKind); 3] = [("volumes", ShapeKind::Volumes), ("paths", ShapeKind::Paths), ("points", ShapeKind::Points)];
    pub fn parse(word: &str) -> Option<ShapeKind> {
        Self::ALL.iter().find(|(n, _)| *n == word.to_lowercase()).map(|x| x.1)
    }
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|x| x.1 == self).map_or("", |x| x.0)
    }
}

/// Where the camera is taken (`cam`).
#[derive(Clone, Debug, PartialEq)]
pub enum CamMode {
    /// A free-flying camera.
    Free,
    /// Back to the orbit camera on the player's ship.
    Player,
    /// The orbit camera on a group.
    Follow(String),
}

/// A change to the developer view the ECS side applies (`DevView`).
#[derive(Clone, Debug, PartialEq)]
pub enum ViewChange {
    Hud(bool),
    /// The frame-rate counter: `None` toggles. The ECS side owns the setting (it is saved with the options).
    Fps(Option<bool>),
    FreeCam(bool),
    Show(ShapeKind, Option<String>),
    Hide(Option<ShapeKind>, Option<String>),
}

/// The developer view: what is drawn in the world, the free camera, the HUD. Shared state; the view draws it.
#[derive(Resource, Default, Clone, Debug)]
pub struct DevView {
    pub hud_hidden: bool,
    pub free_cam: bool,
    /// Set when the free camera was just switched on: the system takes the camera's place as its start.
    pub free_fresh: bool,
    pub shown: BTreeSet<(ShapeKind, Option<String>)>,
}
static HUD_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FREE_CAM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// `hud off`: the HUD, banners, objective text and indicators are not drawn (read by the views, like `panels::cinematic`).
pub fn hud_hidden() -> bool {
    HUD_HIDDEN.load(std::sync::atomic::Ordering::Relaxed)
}
/// `cam free`: the orbit camera leaves the camera alone (`view::camera`).
pub fn free_cam() -> bool {
    FREE_CAM.load(std::sync::atomic::Ordering::Relaxed)
}

impl DevView {
    pub fn apply(&mut self, c: &ViewChange) {
        match c {
            ViewChange::Hud(on) => {
                self.hud_hidden = !on;
                HUD_HIDDEN.store(!on, std::sync::atomic::Ordering::Relaxed);
            }
            ViewChange::Fps(_) => {}
            ViewChange::FreeCam(on) => {
                self.free_fresh = *on && !self.free_cam;
                self.free_cam = *on;
                FREE_CAM.store(*on, std::sync::atomic::Ordering::Relaxed);
            }
            ViewChange::Show(k, name) => {
                self.shown.insert((*k, name.clone()));
            }
            ViewChange::Hide(kind, name) => self.shown.retain(|(k, n)| !(kind.is_none_or(|x| x == *k) && (name.is_none() || n == name))),
        }
    }
}

/// One drawn thing: its label and points (game positions at height `h`, in Bevy axes). One point is a cross, more are a line.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub label: String,
    pub points: Vec<Vec3>,
    pub closed: bool,
    pub kind: ShapeKind,
}

/// What the ECS side knows about a ship that the unit list does not.
#[derive(Clone, Debug, Default)]
pub struct ShipInfo {
    pub e: Option<Entity>,
    pub stance: String,
    pub order: usize,
    pub waypoints: usize,
    pub attack: Vec<String>,
    pub human: bool,
    pub ram: bool,
    pub speed: f32,
}

/// What a command may need besides the script and the unit list.
#[derive(Default)]
pub struct Env {
    /// The player's current target.
    pub target: Option<Entity>,
    /// The mission's id ("mission03"), for `win`.
    pub mission: String,
    pub ships: Vec<ShipInfo>,
    /// Where `mark` appends (`<user data>/console_marks.txt`).
    pub marks: Option<std::path::PathBuf>,
}

/// Game units of a Bevy position: x east, y north (mission JSON), z up.
pub fn game_pos(p: Vec3) -> (f32, f32, f32) {
    (p.x, -p.z, p.y)
}

pub fn marks_path() -> Option<std::path::PathBuf> {
    Some(crate::campaign::user_dir()?.join("console_marks.txt"))
}

/// 'Goto Next Level' to the mission after `id`: the retail campaign's next one, else an empty world (the plain victory line).
pub fn win_action(id: &str) -> Value {
    let world = id.strip_prefix("mission").and_then(|n| n.parse::<u32>().ok()).map_or(String::new(), |n| format!("Mission{:02}.twt", n + 1));
    json!({"Type": "Goto Next Level", "World": world})
}
pub fn lose_action() -> Value {
    json!({"Type": "End Game", "Loser - Custom Message String ID": ""})
}

fn contains_any(text: &str, hay: &[&str]) -> bool {
    let t = text.to_lowercase();
    hay.iter().any(|h| h.to_lowercase().contains(&t))
}

impl Script {
    pub(in crate::rules) fn run_more(&mut self, cmd: &Command, w: &mut [Unit], env: &Env, out: &mut Vec<Fx>, o: &mut Outcome) {
        let player = self.player.clone();
        match cmd {
            Command::Rules(filter) => {
                let f = filter.as_deref().unwrap_or("").to_lowercase();
                for (i, r) in self.rules.iter().enumerate().filter(|(_, r)| r.name.to_lowercase().contains(&f)) {
                    let state = if !self.ext.is_active(i) {
                        "inactive"
                    } else if self.ext.busy(i) {
                        "waiting"
                    } else if self.fired[i] {
                        "fired"
                    } else if self.ran[i] {
                        "ran"
                    } else {
                        "never fired"
                    };
                    o.lines.push(format!("{state:<11} #{i:<3} {}{}", r.name.trim(), if r.once { "" } else { "  (repeats)" }));
                }
                if o.lines.is_empty() {
                    o.lines.push(format!("no rule contains '{}'", filter.as_deref().unwrap_or("")));
                }
            }
            Command::Skip => {
                o.skip_voice = self.skip_speech();
                o.lines.push(if o.skip_voice { "speech line skipped (it counts as played)".into() } else { "no speech line is playing".into() });
            }
            Command::Win => {
                self.console_act(&win_action(&env.mission), w, out);
                o.lines.push("mission won (Goto Next Level)".into());
            }
            Command::Lose => {
                self.console_act(&lose_action(), w, out);
                o.lines.push("mission lost (End Game)".into());
            }
            Command::Restart => {
                o.restart = true;
                o.lines.push("restarting the mission".into());
            }
            Command::Ships(filter) => {
                let f = filter.as_deref().unwrap_or("");
                let list: Vec<&Unit> = w
                    .iter()
                    .filter(|u| !u.owner.is_empty() || !u.name.is_empty())
                    .filter(|u| f.is_empty() || contains_any(f, &[&u.name, &u.owner]) || u.groups.iter().any(|g| g.to_lowercase().contains(&f.to_lowercase())))
                    .collect();
                o.lines.extend(list.iter().map(|u| self.describe_unit(u, env)));
                o.lines.push(format!("{} unit(s)", list.len()));
            }
            Command::Inspect => match env.target.and_then(|t| w.iter().find(|u| u.e == t)) {
                Some(u) => {
                    o.lines.push(self.describe_unit(u, env));
                    if let Some(i) = env.ships.iter().find(|i| i.e == Some(u.e)) {
                        o.lines.push(format!(
                            "  order {}, {} waypoint(s), speed {:.1}{}{}",
                            i.order,
                            i.waypoints,
                            i.speed,
                            if i.human { ", human-controlled" } else { "" },
                            if i.ram { ", ramming" } else { "" }
                        ));
                        if !i.attack.is_empty() {
                            o.lines.push(format!("  attacking {}", i.attack.join(", ")));
                        }
                    }
                    let name = |e: Entity| w.iter().find(|x| x.e == e).map_or("?".to_string(), |x| x.name.clone());
                    for (tower, towed) in self.tows.iter().filter(|t| t.0 == u.e || t.1 == u.e) {
                        o.lines.push(if *tower == u.e { format!("  towing {}", name(*towed)) } else { format!("  towed by {}", name(*tower)) });
                    }
                }
                None => o.lines.push("no target: click a ship first".into()),
            },
            Command::Where => match w.iter().find(|u| u.primary && u.alive) {
                Some(p) => {
                    let (x, y, z) = game_pos(p.pos);
                    o.lines.push(format!("{} at x {x:.1}, y {y:.1}, z {z:.1} (game units: x east, y north)", p.name));
                }
                None => o.lines.push("the player has no ship".into()),
            },
            Command::Mark(name) => match (w.iter().find(|u| u.primary && u.alive), &env.marks) {
                (Some(p), Some(path)) => {
                    let (x, y, z) = game_pos(p.pos);
                    let line = format!("{{\"name\": {}, \"pos\": [{x:.1}, {y:.1}, {z:.1}]}}", Value::String(name.clone()));
                    let wrote = path
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(path))
                        .and_then(|mut f| writeln!(f, "{line}"));
                    o.lines.push(match wrote {
                        Ok(()) => format!("{line}\nappended to {}", path.display()),
                        Err(e) => format!("could not write {}: {e}", path.display()),
                    });
                }
                (None, _) => o.lines.push("the player has no ship".into()),
                (_, None) => o.lines.push("no user data folder to write the marks to".into()),
            },
            Command::Show(kind, name) => {
                let n = self.debug_shapes(*kind, name.as_deref(), 0.0).len();
                o.view.push(ViewChange::Show(*kind, name.clone()));
                o.lines.push(format!("showing {n} {}{}", kind.name(), name.as_ref().map_or(String::new(), |n| format!(" '{n}'"))));
            }
            Command::Hide(kind, name) => {
                o.view.push(ViewChange::Hide(*kind, name.clone()));
                o.lines.push("hidden".into());
            }
            Command::Cam(mode) => match mode {
                CamMode::Free => {
                    o.view.push(ViewChange::FreeCam(true));
                    o.lines.push("free camera: arrows move, PageUp / PageDown rise and sink, right mouse drag looks, Shift is faster; `cam player` returns".into());
                }
                CamMode::Player => {
                    o.view.push(ViewChange::FreeCam(false));
                    self.console_act(&focus_camera(&flagship(&player)), w, out);
                    o.lines.push("camera back on the player's ship".into());
                }
                CamMode::Follow(group) => {
                    o.view.push(ViewChange::FreeCam(false));
                    self.console_act(&focus_camera(group), w, out);
                    o.lines.push(format!("camera follows {group}"));
                }
            },
            Command::Fps(on) => o.view.push(ViewChange::Fps(*on)),
            Command::Hud(on) => {
                o.view.push(ViewChange::Hud(*on));
                o.lines.push(format!("HUD {}", if *on { "on" } else { "off" }));
            }
            _ => {}
        }
    }

    /// One unit on a line: id, name, owner, groups, hull damage, position (game units), visibility, stance.
    fn describe_unit(&self, u: &Unit, env: &Env) -> String {
        let (x, y, _) = game_pos(u.pos);
        let stance = env.ships.iter().find(|i| i.e == Some(u.e)).map_or("-", |i| i.stance.as_str());
        let groups: Vec<&str> = u.groups.iter().map(String::as_str).filter(|g| *g != u.owner).collect();
        format!(
            "#{} {} [{}]{} dmg {:.0}% at ({x:.0}, {y:.0}){}{} {stance}",
            u.id,
            if u.name.is_empty() { "(unnamed)" } else { &u.name },
            u.owner,
            if groups.is_empty() { String::new() } else { format!(" groups {}", groups.join(",")) },
            u.damage * 100.0,
            if u.visible { "" } else { " hidden" },
            if u.alive { "" } else { " destroyed" }
        )
    }

    /// The mission's volumes, paths or points as lines to draw at height `h`; `name` limits them to one. Points are the point sets and
    /// the objective points.
    pub fn debug_shapes(&self, kind: ShapeKind, name: Option<&str>, h: f32) -> Vec<Shape> {
        let want = |n: &str| name.is_none_or(|x| x == n);
        let at = |x: f32, y: f32| Vec3::new(x, h, -y);
        let mut shapes: Vec<Shape> = match kind {
            ShapeKind::Volumes => self
                .volumes
                .iter()
                .filter(|(n, r)| want(n) && r.len() > 1)
                .map(|(n, r)| Shape { label: n.clone(), points: r.0.iter().map(|p| at(p.x, p.y)).collect(), closed: true, kind })
                .collect(),
            ShapeKind::Paths => self
                .paths
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(n, _)| want(n))
                .map(|(n, v)| Shape {
                    label: n.clone(),
                    points: v
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| {
                            let (x, y) = gxy(p);
                            at(x, y)
                        })
                        .collect(),
                    closed: false,
                    kind,
                })
                .collect(),
            ShapeKind::Points => {
                let sets = self.point_sets.as_object().into_iter().flatten().filter(|(n, _)| want(n) && n.as_str() != TP_SET);
                let mut v: Vec<Shape> = sets
                    .flat_map(|(n, pts)| pts.as_array().into_iter().flatten().enumerate().map(move |(i, p)| (n, i, p)))
                    .map(|(n, i, p)| {
                        let (x, y) = gxy(&p["pos"]);
                        Shape { label: if i == 0 { n.clone() } else { format!("{n} {}", i + 1) }, points: vec![at(x, y)], closed: false, kind }
                    })
                    .collect();
                v.extend(self.points.as_array().into_iter().flatten().filter(|p| want(s(p, "Name"))).map(|p| {
                    let (x, y) = gxy(&p["Position"]);
                    Shape { label: s(p, "Name").to_string(), points: vec![at(x, y)], closed: false, kind }
                }));
                v
            }
        };
        shapes.retain(|s| !s.points.is_empty());
        shapes.sort_by(|a, b| a.label.cmp(&b.label));
        shapes
    }

    /// The names `show` / `hide` accept for a kind.
    pub fn shape_names(&self, kind: ShapeKind) -> Vec<String> {
        let mut v: Vec<String> = self.debug_shapes(kind, None, 0.0).into_iter().map(|s| s.label).collect();
        v.sort();
        v.dedup();
        v
    }
}

/// The game x, y of a JSON position `[x, y, z]`.
fn gxy(v: &Value) -> (f32, f32) {
    (v[0].as_f64().unwrap_or(0.0) as f32, v[1].as_f64().unwrap_or(0.0) as f32)
}

/// 'Focus Camera On Group': the orbit camera on a group (the player's ship puts it back on the player).
pub fn focus_camera(group: &str) -> Value {
    json!({"Type": "Focus Camera On Group", "GroupUnit Name": group, "Distance": 60.0, "Relative Angle": 0.0})
}

#[cfg(test)]
mod tests;
