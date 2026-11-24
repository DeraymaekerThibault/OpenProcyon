//! The rule language as data: every condition and action `Type` the mission rules can use, with its fields, the kind of each field
//! and the value the interpreter reads when a field is missing.
//!
//! This table is the machine-readable vocabulary (`--dump-rule-schema`) and what `--check-mission` validates a mission against
//! (`check.rs`). It does not drive the interpreter: `act.rs` / `cond.rs` stay the authority on behaviour, and the unit test
//! `schema_matches_the_interpreter` fails when a `Type` is handled there without an entry here, or the other way round.
//!
//! To add a `Type`: write its arm in `act.rs` / `cond.rs`, then append one `cond(..)` / `act(..)` entry to `RULE_TYPES` (engine-added
//! Types use `.engine()`). Retail field names are kept exactly as the TWT exports them, typos included.

use serde_json::{Value, json};

use super::Station;

/// What a field's value refers to. The checker resolves each kind against the mission's own definitions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 'Group' or 'Group,Unit': a TWT group, a player (owner) or a named unit.
    Group,
    /// A player name (or the group the rules treat as one).
    Player,
    /// A path (`paths`).
    Path,
    /// A trigger volume (`volumes`).
    Volume,
    /// An objective task (`tasks`).
    Task,
    /// An objective point (`points`).
    Point,
    /// A timer: created by 'Start Timer', read by 'Timer Condition'.
    Timer,
    /// A flag: written by 'Set Flag Action', read by 'Flag Condition'.
    Flag,
    /// A counter (engine-added): written by 'OP Set Counter' / 'OP Add To Counter', read by 'OP Counter Condition'.
    Counter,
    /// A campaign counter (engine-added): kept for the next mission of the same mod campaign, so one set by an earlier mission is not an
    /// error. Written by 'OP Set Campaign Counter' / 'OP Add To Campaign Counter', read by 'OP Campaign Counter Condition'.
    CampaignCounter,
    /// The name of a rule of the mission (engine-added: 'OP Set Rule Active').
    Rule,
    /// A list of condition objects (engine-added: 'OP Any Of'); each is checked like a top-level condition.
    ConditionList,
    /// One condition object (engine-added: 'OP Not').
    Condition,
    /// A speech event (`media.json` `speech`, or the mission's own `opening_speech`).
    Speech,
    /// A point set (`environment.point_sets`).
    PointSet,
    /// A star map label (`map_text`).
    MapText,
    /// An end-of-game message id (`media.json` `messages`).
    Message,
    /// A skirmish team name id.
    Team,
    /// A localized ship name id (`media.json` `ship_names`).
    ShipName,
    /// "TRUE" / "FALSE". Anything else, a JSON `true` included, reads as FALSE.
    Bool,
    /// A number, optionally with a range (`Field::range`).
    Number,
    /// One of a fixed list of strings.
    Enum(&'static [&'static str]),
    /// Free text: file names, effect names, ids the engine only passes on.
    Text,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Group => "group",
            Kind::Player => "player",
            Kind::Path => "path",
            Kind::Volume => "volume",
            Kind::Task => "task",
            Kind::Point => "point",
            Kind::Timer => "timer",
            Kind::Flag => "flag",
            Kind::Counter => "counter",
            Kind::CampaignCounter => "campaign_counter",
            Kind::Rule => "rule",
            Kind::ConditionList => "condition_list",
            Kind::Condition => "condition",
            Kind::Speech => "speech_event",
            Kind::PointSet => "point_set",
            Kind::MapText => "map_text",
            Kind::Message => "message",
            Kind::Team => "team",
            Kind::ShipName => "ship_name",
            Kind::Bool => "bool",
            Kind::Number => "number",
            Kind::Enum(_) => "enum",
            Kind::Text => "text",
        }
    }
    /// Editor placeholders retail rules carry for "nothing chosen"; they mean unset and are never looked up.
    pub fn placeholders(self) -> &'static [&'static str] {
        match self {
            Kind::Path => &["NO PATH", "PATH NAME"],
            Kind::Point => &["NO OBJECTIVE POINT"],
            Kind::PointSet => &["POINT SET"],
            Kind::Message => &["GAME STRING"],
            Kind::Player => &["NO PLAYER"],
            Kind::ShipName => &["LOCALIZED SHIP NAME"],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Condition,
    Action,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// In the original game's rule editor.
    Retail,
    /// Added by OpenProcyon; the original ignores or rejects it.
    Engine,
}
/// The `Equivalence` choice of the comparison conditions: the one place its three spellings and their meaning live. Callers keep
/// their own epsilon for `EqualTo` (the interpreter's `cmp` 1e-4, the skirmish end rules 1e-3); any other spelling holds never.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Equivalence {
    GreaterThan,
    LessThan,
    EqualTo,
}
impl Equivalence {
    pub const NAMES: [&'static str; 3] = ["Greater Than", "Less Than", "Equal To"];
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Greater Than" => Some(Self::GreaterThan),
            "Less Than" => Some(Self::LessThan),
            "Equal To" => Some(Self::EqualTo),
            _ => None,
        }
    }
    /// `a <op> b`, `EqualTo` meaning `|a - b| < epsilon`.
    pub fn holds(self, a: f32, b: f32, epsilon: f32) -> bool {
        match self {
            Self::GreaterThan => a > b,
            Self::LessThan => a < b,
            Self::EqualTo => (a - b).abs() < epsilon,
        }
    }
}

/// Who evaluates the Type. Skirmish end conditions are read by `victory.rs`, not by the interpreter's match arms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handler {
    Interpreter,
    Victory,
}

#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    /// What the interpreter reads when the field is missing, as JSON text ("" for a name, "FALSE", "0", ...).
    pub default: &'static str,
    /// A missing field is a mistake (the default is a silent no-op or an always-false condition).
    pub required: bool,
    /// False for fields the retail export carries and the engine never reads.
    pub used: bool,
    pub range: Option<(f32, f32)>,
    /// A reference that does not resolve is a warning, not an error: the retail missions carry such breaks and the engine tolerates them.
    pub soft: bool,
}

impl Field {
    const fn new(name: &'static str, kind: Kind, default: &'static str) -> Field {
        Field { name, kind, default, required: true, used: true, range: None, soft: false }
    }
    /// Optional: the default is meaningful.
    pub const fn opt(mut self) -> Field {
        self.required = false;
        self
    }
    /// Exported by the retail editor, never read: accepted, not checked.
    pub const fn ignored(mut self) -> Field {
        self.required = false;
        self.used = false;
        self
    }
    pub const fn range(mut self, lo: f32, hi: f32) -> Field {
        self.range = Some((lo, hi));
        self
    }
    pub const fn soft(mut self) -> Field {
        self.soft = true;
        self
    }
    pub const fn dflt(mut self, d: &'static str) -> Field {
        self.default = d;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TypeDef {
    pub name: &'static str,
    pub role: Role,
    pub origin: Origin,
    pub handler: Handler,
    /// Only a campaign mission shows or reads its effect: a skirmish map script that uses it is an error for `--check-mission`, and
    /// a warning when the map loads (the cutscene camera, the mission HUD and tutorial hooks, level changes, the human's ship handoffs).
    pub campaign_only: bool,
    pub fields: &'static [Field],
}

impl TypeDef {
    const fn new(name: &'static str, role: Role, fields: &'static [Field]) -> TypeDef {
        TypeDef { name, role, origin: Origin::Retail, handler: Handler::Interpreter, campaign_only: false, fields }
    }
    /// Added by OpenProcyon, not in the original's rule editor.
    pub const fn engine(mut self) -> TypeDef {
        self.origin = Origin::Engine;
        self
    }
    /// Only campaign missions can use it (see `campaign_only`).
    const fn campaign(mut self) -> TypeDef {
        self.campaign_only = true;
        self
    }
    const fn victory(mut self) -> TypeDef {
        self.handler = Handler::Victory;
        self
    }
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

// Field constructors, one per kind.
const fn grp(n: &'static str) -> Field {
    Field::new(n, Kind::Group, "")
}
const fn plr(n: &'static str) -> Field {
    Field::new(n, Kind::Player, "")
}
const fn path(n: &'static str) -> Field {
    Field::new(n, Kind::Path, "")
}
const fn vol(n: &'static str) -> Field {
    Field::new(n, Kind::Volume, "")
}
const fn task(n: &'static str) -> Field {
    Field::new(n, Kind::Task, "")
}
const fn point(n: &'static str) -> Field {
    Field::new(n, Kind::Point, "")
}
const fn timer(n: &'static str) -> Field {
    Field::new(n, Kind::Timer, "")
}
const fn flag(n: &'static str) -> Field {
    Field::new(n, Kind::Flag, "")
}
const fn counter(n: &'static str) -> Field {
    Field::new(n, Kind::Counter, "")
}
const fn campaign_counter(n: &'static str) -> Field {
    Field::new(n, Kind::CampaignCounter, "")
}
const fn rule_name(n: &'static str) -> Field {
    Field::new(n, Kind::Rule, "")
}
const fn conditions(n: &'static str) -> Field {
    Field::new(n, Kind::ConditionList, "[]")
}
const fn condition(n: &'static str) -> Field {
    Field::new(n, Kind::Condition, "")
}
const fn speech(n: &'static str) -> Field {
    Field::new(n, Kind::Speech, "")
}
const fn pset(n: &'static str) -> Field {
    Field::new(n, Kind::PointSet, "")
}
const fn maptext(n: &'static str) -> Field {
    Field::new(n, Kind::MapText, "")
}
const fn message(n: &'static str) -> Field {
    Field::new(n, Kind::Message, "")
}
const fn team(n: &'static str) -> Field {
    Field::new(n, Kind::Team, "")
}
const fn shipname(n: &'static str) -> Field {
    Field::new(n, Kind::ShipName, "")
}
const fn boolean(n: &'static str) -> Field {
    Field::new(n, Kind::Bool, "FALSE")
}
const fn num(n: &'static str) -> Field {
    Field::new(n, Kind::Number, "0")
}
/// A number that is a fraction 0..1 (damage, throttle, volume).
const fn frac(n: &'static str) -> Field {
    Field::new(n, Kind::Number, "0").range(0.0, 1.0)
}
const fn choice(n: &'static str, values: &'static [&'static str]) -> Field {
    Field::new(n, Kind::Enum(values), "")
}
const fn text(n: &'static str) -> Field {
    Field::new(n, Kind::Text, "")
}

const COMPARE: &[&str] = &Equivalence::NAMES;
const STATION: &[&str] = &Station::NAMES;
const FOLLOW: &[&str] = &["to end", "loop", "teleport loop", "end to end"];
const STANCE: &[&str] = &["Stance_Dummy", "Stance_Neutral", "Stance_Defensive", "Stance_Aggressive", "Stance Persistant", "Stance Persistent", "AI STANCE"];
const FORMATION: &[&str] = &["None", "Custom", "LineAbreast", "EchelonRight", "EchelonLeft", "Column", "Diamond", "Convoy"];
const GUNNERY: &[&str] = &["CREW SKILL LEVEL", "Green", "Average", "Elite"];

const fn cond(name: &'static str, fields: &'static [Field]) -> TypeDef {
    TypeDef::new(name, Role::Condition, fields)
}
const fn act(name: &'static str, fields: &'static [Field]) -> TypeDef {
    TypeDef::new(name, Role::Action, fields)
}

/// Every Type of the rule language. Append a new Type's entry at the end of its section.
pub static RULE_TYPES: &[TypeDef] = &[
    // ---- Conditions -------------------------------------------------------------------------------------------------
    cond("World Initialize", &[]),
    cond("Flag Condition", &[flag("Flag Name"), boolean("Boolean Value")]),
    cond("Timer Condition", &[timer("Timer Name"), choice("Equivalence", COMPARE), num("Time in seconds")]),
    cond("Speech Event Played Once", &[speech("Speech Event Name")]),
    cond("Speech Event Not Played Yet", &[speech("Speech Event Name")]),
    cond("Is Group In Volume", &[grp("Group Name"), vol("Volume Name"), boolean("Entire Group").opt()]),
    cond("Enter Volume", &[grp("Group Name"), vol("Volume Name")]),
    cond("Exit Volume", &[grp("Group Name"), vol("Volume Name")]),
    cond("Team Member Enters Volume", &[team("Team Name"), vol("Volume Name")]),
    cond("Group Destroyed", &[grp("Group Name")]),
    cond("Group Under Attack", &[grp("Group Name")]),
    cond("Group/Unit Fired X Shots", &[grp("Group/Unit Name"), choice("Equivalence", COMPARE), num("Number Of Rounds Fired")]),
    cond("Group/Unit Vital Section has >,<,= X damage", &[grp("Group/Unit Name"), text("Vital Section").ignored(), choice("Equivalence", COMPARE), frac("Damage Percent")]),
    cond("Does Group Contain Unit Name", &[grp("Unit Name"), boolean("Exists?")]),
    cond("Is Ship In Tow", &[grp("Towing Group"), grp("Target to Tow"), boolean("Boolean")]),
    cond("Is Group A attacking Group B", &[grp("GroupA Name"), grp("GroupB Name")]),
    cond("Group/Unit Hit at least x Times", &[grp("Group/Unit Name"), num("Number Of Times Hit")]),
    cond(
        "Group/Unit Hit at least x Times by Player ( with eqivalence )",
        &[grp("Group/Unit Name"), plr("Player Name"), choice("Equivalence", COMPARE), num("Number Of Times Hit")],
    ),
    cond("Player has hit Group/Unit at least x Times", &[plr("Player Name"), grp("Group/Unit Name"), num("Number Of Times Hit")]),
    cond("Group has X members", &[grp("Group Name"), choice("Equivalence", COMPARE), num("number")]),
    cond("Group/Unit has >,<,= X damage", &[grp("Group/Unit Name"), choice("Equivalence", COMPARE), frac("Damage Percent")]),
    cond("Group/Unit contains no mission essential ships", &[grp("Group Name")]),
    cond("Unit Flag Texture", &[grp("Unit Name"), text("Flag Type"), boolean("Boolean")]),
    cond("Group to Group Distance", &[grp("GroupA Name"), grp("GroupB Name"), choice("Equivalence", COMPARE), num("Distance")]),
    cond("Group/Unit Docked", &[grp("Dockers Group/Unit Name"), grp("Targets Group/Unit Name")]),
    cond("Group/Unit Is Docked", &[grp("Dockers Group/Unit Name"), grp("Targets Group/Unit Name")]),
    cond("Unit is Within any Nebula", &[grp("Unit Name")]),
    cond(
        "All Units from Group have entered trigger volume Once",
        &[grp("Group Name"), vol("Volume Name"), plr("Player Name").ignored(), num("Object that have already entered - Size").ignored()],
    ),
    cond("Player Vs Player capture count", &[plr("Player Name A"), plr("Player Name B"), choice("Equivalence", COMPARE), num("Number Of Captures")]),
    cond("Team has captured a ship from Group/Unit", &[team("Team Name"), grp("Group/Unit"), num("Number Of Captures")]),
    cond("Team has destroyed a ship from Group/Unit", &[team("Team Name"), grp("Group/Unit"), num("Number Of Ships Destroyed")]),
    cond(
        "Unit from Group enters trigger volume ( Once per Unit )",
        &[
            grp("Group Name"),
            vol("Volume Name"),
            team("Team Name"),
            num("Number of objects entered, waiting to report").ignored(),
            num("Object that have already entered - Size").ignored(),
        ],
    ),
    cond("Team has X points", &[team("Team Name"), num("Points")]),
    cond("Player has no lifeboats", &[plr("Player Name")]),
    cond("Is Starmap Open", &[]),
    cond("Mission 9 - If Mortar explodes within Area", &[vol("Volume Name")]).campaign(),
    cond("Player Killed A Object", &[plr("Player Name"), choice("World Object Type", &["Asteroid", "Ship"])]),
    // Skirmish end conditions: read by victory.rs from the map's `victory` rules.
    cond("No Human Controlled Ships Remain", &[]).victory(),
    cond("No Team Has Ships", &[]).victory(),
    cond("Skirmish Game Complete", &[]).victory(),
    cond("Team Game Complete", &[]).victory(),
    cond("Team X has no ships", &[team("Team Name")]).victory(),
    // Engine-added (not retail): combinators and counters, interpreted in `ext.rs`.
    cond("OP Any Of", &[conditions("Conditions")]).engine(),
    cond("OP Not", &[condition("Condition")]).engine(),
    cond("OP Counter Condition", &[counter("Counter Name"), choice("Equivalence", COMPARE), num("Value")]).engine(),
    cond("OP Campaign Counter Condition", &[campaign_counter("Counter Name"), choice("Equivalence", COMPARE), num("Value")]).engine(),
    cond("OP Group Is Cloaked", &[grp("Group Name"), boolean("Boolean").dflt("TRUE").opt()]).engine(),
    cond("OP Group Speed", &[grp("Group Name"), choice("Which", &["Fastest", "Slowest"]), choice("Equivalence", COMPARE), num("Speed")]).engine(),
    cond("OP Task State", &[task("Objective Task"), choice("State", &["Active", "Complete", "Failed"])]).engine(),
    // ---- Actions ----------------------------------------------------------------------------------------------------
    act("Start Timer", &[timer("Timer Name")]),
    act("Stop Timer", &[timer("Timer Name")]),
    act("Set Flag Action", &[flag("Flag Name"), boolean("Boolean Value")]),
    act("Play speech event", &[speech("Speech Event Name")]),
    act(
        "Play Music Track",
        &[text("File Name"), boolean("Crossfade transition").ignored(), num("Fade In Time ( secs )"), num("Fade Out Time ( secs )"), frac("New Volume ( 0 to 1 )")],
    ),
    act("Set Objective Task Active State", &[task("Objective Task"), boolean("Active State")]),
    act("Set Objective Task Failed State", &[task("Objective Task"), boolean("Failed State")]),
    act("Set Objective Task Complete State", &[task("Objective Task"), boolean("Complete State")]),
    act("Set Current Objective Point", &[point("Objective Point")]).campaign(),
    act("Set Current Objective Point On Ship", &[grp("Group Name")]).campaign(),
    act("Set Current Objective Point Visible On Starmap", &[boolean("Visible On Starmap")]).campaign(),
    act("Set Map Text Visibility", &[maptext("Map Text Name"), boolean("Visible On Starmap")]),
    act("Group to follow path", &[grp("Group Name"), path("Path Name"), boolean("Find Closest Point"), choice("Follow Mode", FOLLOW)]),
    act("Setup Space Animal Flock", &[grp("Group Name"), path("Path Name"), boolean("Find Closest Point"), choice("Follow Mode", FOLLOW), num("Velocity")]),
    act(
        "Setup Asteroid Belt",
        &[
            grp("Group Name"),
            path("Path Name"),
            boolean("Find Closest Point"),
            choice("Follow Mode", FOLLOW),
            num("Velocity Lower m/sec"),
            num("Velocity Upper m/sec"),
            num("Tumble Lower Rads/sec"),
            num("Tumble Upper Rads/sec"),
        ],
    ),
    act("Set Group Throttle Percent", &[grp("Group Name"), frac("Throttle Percent")]),
    act("Destroy Group/Unit", &[grp("Group Name")]),
    act("Transfer Group/Unit to Group", &[grp("Group/Unit to transfer"), grp("Target Group")]),
    act("GroupA to Ram GroupB", &[grp("Group Name"), grp("Group Unit Target")]),
    act("Teleport Group/Unit", &[grp("Group/Unit Name"), pset("Point Set Name")]),
    act("Dock Ships", &[grp("Docking Group"), grp("Target to Dock to")]),
    act("Set Group/Unit Dockable", &[grp("Group Name"), boolean("Boolean")]),
    act("Set Group/Unit Boardable", &[grp("Group Name"), boolean("Boolean")]),
    act("Toggle Island Repair When Docked", &[grp("Group Name"), boolean("Repair when docked")]),
    act("Set Dock Time", &[grp("Docking GroupUnit"), num("Dock Time")]),
    act("Set Group Space Objects velocity", &[grp("Group Name"), num("Velocity")]),
    act("Set Max Throttle Percent (Max user settable)", &[grp("Group Name"), frac("Throttle Percent")]),
    act("Set Ship Top Speed Percentage", &[grp("Group Name"), frac("Throttle Speed")]),
    act("Set Ship Name", &[grp("Group/Unit Name"), shipname("New Ship Name")]),
    act("Set Group/Unit AI Stance", &[grp("Group/Unit Name"), choice("AI Stance", STANCE)]),
    act("Set Group/Unit Owner", &[grp("Group/Unit Name"), plr("New Owner")]),
    act("Set Group/Unit Visibility", &[grp("Group/Unit Name"), boolean("Boolean")]),
    act("Set Group/Unit Vulnerability", &[grp("Group Name"), frac("Invulnerable Percent")]),
    act("Set Ships Flag Texture", &[grp("Group Name"), text("Flag Texture")]),
    act("Set Ship Banner Type", &[grp("Group/Unit Name"), text("Banner Type")]),
    act("GroupA to attack GroupB", &[grp("Attack Group"), grp("Target Group")]),
    act("Clear all AI commands", &[grp("Group/Unit Name")]),
    act("Damage Group/Unit by X percent", &[grp("Group Name"), frac("Percent")]),
    act("Set Group/Unit Hold Position", &[grp("Group/Unit Name"), boolean("Boolean").opt().dflt("TRUE")]),
    act("Reset Hit Count", &[grp("Group/Unit Name")]),
    act("Set FleetFormation Type", &[plr("Player Name"), choice("Formation Type", FORMATION), boolean("Hold Formation")]),
    act("Set Fleet Hold Formation", &[plr("Player Name"), boolean("Hold Formation")]),
    act("Set Fleet Primary Ship", &[grp("Group/Unit Name")]).campaign(),
    act("Set Fleet Hold Fire", &[plr("PLAYER FLEET"), boolean("HOLD FIRE")]),
    act("Set alliance between PlayerA and PlayerB to TRUE/FALSE", &[plr("PLAYER A"), plr("PLAYER B"), boolean("SET ALLIANCE TO")]),
    act("Tow Ship", &[grp("Docking Group"), grp("Target to Tow")]),
    act("Break Tow", &[grp("TowerGroupA"), grp("TowerGroupB")]),
    act("NIS Start", &[boolean("All Objects Visible").ignored(), boolean("Open NIS bars instantly").ignored()]).campaign(),
    act("NIS End", &[]).campaign(),
    act("NIS Set Camera Path", &[path("Path Name"), boolean("Jump to start").ignored()]).campaign(),
    act("NIS Set Camera Speed", &[num("Max Velocity"), num("Acceleration").ignored()]).campaign(),
    act("NIS Set Transition Camera Speed", &[num("Max Velocity"), num("Acceleration").ignored()]).campaign(),
    act("NIS Focus camera on Group/Unit", &[grp("GroupUnit Name")]).campaign(),
    act("NIS Focus camera on Point", &[pset("Point Set Name")]).campaign(),
    act("NIS Attach Camera", &[grp("Group/Unit Name"), num("Distance"), num("Angle YZ"), num("Angle XY")]).campaign(),
    act("NIS Position Camera Relative to Object", &[grp("Unit Name"), num("Distance"), num("Angle YZ"), num("Angle XY"), boolean("Jump to point").ignored()]).campaign(),
    act("NIS Focus on Main Ship", &[]).campaign(),
    act("Set Group/Unit Cloak State", &[grp("Group Name"), boolean("Cloak State"), boolean("AI Can Override")]),
    act("Focus Camera On Group", &[grp("GroupUnit Name"), num("Distance"), num("Relative Angle"), boolean("Use Transition").ignored()]).campaign(),
    act("Add Victory Points for SinglePlayer", &[num("Victory points to be added")]).campaign(),
    act("Goto Next Level", &[text("World"), boolean("Display Loading String").ignored()]).campaign(),
    act(
        "End Game",
        &[
            message("Loser - Custom Message String ID"),
            message("Winner - Custom Message String ID").ignored(),
            boolean("Use Custom Message").ignored(),
            boolean("Show Stats Screen").ignored(),
        ],
    ),
    act("Create/Release Event Effect", &[pset("Point Set"), text("Effect Name"), boolean("Boolean State")]),
    act("Open HUD Texture Overlay", &[text("Texture Base Name")]).campaign(),
    act("Close HUD Texture Overlay", &[]).campaign(),
    act("Open Weapon Bar", &[boolean("OpenState")]).campaign(),
    act("Play Special Effect", &[text("File Name"), boolean("Play As Dialog").ignored()]),
    act("Set Lifeboat Creation State", &[grp("Group Name"), boolean("Boolean")]),
    act(
        "*State Init* Setup Ship",
        &[
            num("World Object ID"),
            text("Ship Name").opt(),
            path("Ship Path").opt(),
            choice("Follow Mode", FOLLOW).opt(),
            choice("AI Stance", STANCE).opt(),
            plr("Player/Owner").opt(),
            boolean("Primary Ship").opt(),
            text("Crew Skill Level").opt(),
            boolean("Boardable").opt(),
            text("Localized Ship Name").opt(),
        ],
    ),
    act(
        "*State Init* Setup Island",
        &[num("World Object ID"), num("Combat Strength").opt(), plr("Player/Owner").opt(), choice("Gunnery Level", GUNNERY).opt(), choice("AI Stance", STANCE).opt()],
    ),
    act("Set Group/Unit Mission Essential", &[grp("Group Name"), boolean("Boolean")]),
    act("Set Radar Active State", &[boolean("Radar Active")]).campaign(),
    act("Set Group/Unit Towable", &[grp("Group Name"), boolean("Boolean")]),
    act("Set Group/Unit Movable", &[grp("Group Name"), boolean("Boolean")]),
    act("SetCollidable", &[grp("Group/Unit Name"), boolean("Boolean")]),
    act("Set Group/Unit Border Zone", &[grp("Group/Unit Name"), vol("Polygon Name")]),
    act("Clear Group/Unit Border Zone", &[grp("Group/Unit Name")]),
    act("Set Group/Unit Border Zone Return Point", &[grp("Group/Unit Name"), pset("Pointset Name")]),
    act("Crew Speech - Toggle On Off", &[boolean("Crew Speech State State")]).campaign(),
    act("Crew Speech - Helm Off Course", &[]).campaign(),
    act("Open Crew and Arms Screens", &[]).campaign(),
    act("Set Is In Calyan Abyss", &[boolean("Boolean Value")]).campaign(),
    act("Set Group/Unit AI Captain", &[grp("Group/Unit Name"), choice("AI Stance", &["Captain_Human"])]).campaign(),
    act("NIS Zoom", &[num("FOV"), num("Speed")]).campaign(),
    act("NIS Toggle NIS mode Gun Accuracy", &[boolean("Use NIS Gun Accuracy")]).campaign(),
    act("NIS Toggle All Objects Visibility", &[boolean("All Objects Visible")]).campaign(),
    act("Set Nebula Fadeout Distance", &[num("Fadeout Distance")]).campaign(),
    act("Set Nebula to Lock Objects", &[boolean("Trap")]).campaign(),
    act("Set Group/Unit Warning Shot Mode", &[grp("GroupUnit Name"), boolean("Boolean")]),
    act("TUTORIAL - Pause when starmap opens?", &[boolean("Pause when starmap opens")]).campaign(),
    act("*State Init* Setup Etherium Current", &[num("World Object ID").ignored(), text("Etherium Name").ignored(), path("Etherium Path").soft()]),
    act("Mission 9 - Do Dark Matter Explosion", &[vol("Affected Area")]),
    act("Mission 9 - Teleport Longboat", &[grp("Longboat Group Name")]).campaign(),
    act("Dragon - Set AI Stance", &[grp("Group/Unit Name"), choice("AI Stance", STANCE)]),
    act("Dragon - Set Damage Threshold", &[grp("Group Name"), frac("Damage Threshold")]),
    act("Grant Team X Points", &[team("Team Name"), num("Points")]),
    act("Setup Team Objective", &[team("Team Name"), point("Objective Point"), task("Objective Task").opt()]),
    act("Team X Wins", &[team("Team Name").opt()]),
    act("Remaining Team Wins", &[]),
    act("Reset Shots Fired Count", &[grp("Group/Unit Name")]),
    // Engine-added (not retail): sequences, counters and rule activation, interpreted in `ext.rs`.
    act("OP Wait", &[num("Seconds")]).engine(),
    // An empty name waits for the whole speech queue.
    act("OP Wait Until Speech Done", &[speech("Speech Event Name").opt()]).engine(),
    act("OP Set Counter", &[counter("Counter Name"), num("Value")]).engine(),
    act("OP Add To Counter", &[counter("Counter Name"), num("Value")]).engine(),
    act("OP Set Rule Active", &[rule_name("Rule Name"), boolean("Boolean")]).engine(),
    act("OP Set Campaign Counter", &[campaign_counter("Counter Name"), num("Value")]).engine(),
    act("OP Add To Campaign Counter", &[campaign_counter("Counter Name"), num("Value")]).engine(),
    // Orders that act on ships: applied by the module that owns the behaviour (boarding.rs, lifeboats.rs, ...), see docs/modding.md.
    act("OP Board Ship", &[grp("Group Name"), grp("Target Group")]).engine(),
    act("OP Abandon Ship", &[grp("Group Name")]).engine(),
    // `Heading` is a compass bearing in degrees in game axes (0 up the map along +y, 90 toward +x); absent, ships keep their headings.
    act("OP Teleport", &[grp("Group/Unit Name"), pset("Point Set Name"), num("Point Index").opt(), num("Heading").opt().dflt("")]).engine(),
    act(
        "OP Etherium Current",
        &[path("Etherium Path"), num("Strength").opt().dflt("").range(0.0, 1000.0), num("Width").opt().dflt("").range(0.0, 100000.0), boolean("Boolean").opt().dflt("")],
    )
    .engine(),
    act("OP Fade", &[num("Seconds").range(0.0, 600.0)]).engine().campaign(),
    act("OP Place Mines", &[pset("Point Set Name"), text("Bullet").opt().dflt("Bullet_StarMine_Med"), plr("Player/Owner").opt()]).engine(),
    act("OP Follow Group", &[grp("Group Name"), grp("Target Group"), choice("Station", STATION), num("Distance").range(0.0, 100000.0)]).engine(),
    act(
        "*State Init* Setup Nebula",
        &[
            num("World Object ID").ignored(),
            text("New Nebula Name").opt(),
            vol("Polygon Name").opt(),
            boolean("Lightning On/Off").opt(),
            num("Lightning Blast Recharge Time").opt(),
            boolean("Meteors On/Off").opt(),
            num("Meteor Strike Recharge Time").opt(),
            text("Nebula Cloud Effect Name").opt(),
            text("Solar Storm Effect Name").opt(),
            text("Meteor Shower Effect Name").opt(),
            pset("Nebula Cloud Point Set Name").opt(),
            pset("Solar Storm Point Set Name").opt(),
            pset("Meteor Shower Point Set Name").opt(),
            boolean("Rotational Winds On/Off").opt(),
            num("Wind Magnitude").opt(),
            boolean("Nebula Cloud Energy Drain On/Off").opt(),
            boolean("Nebula Occlusion On/Off").opt(),
            num("Solar Storm Wind Damage Frequency").opt(),
            num("Ambient sound max distance").opt(),
        ],
    ),
];

pub fn lookup(role: Role, name: &str) -> Option<&'static TypeDef> {
    RULE_TYPES.iter().find(|t| t.role == role && t.name == name)
}

/// The schema as JSON for tools (`--dump-rule-schema`): every Type with its fields, and the kinds with their placeholders.
pub fn to_json() -> Value {
    let field = |f: &Field| {
        let mut v = json!({"name": f.name, "kind": f.kind.name(), "default": f.default, "required": f.required, "used": f.used, "soft": f.soft});
        if let Kind::Enum(values) = f.kind {
            v["values"] = json!(values);
        }
        if let Some((lo, hi)) = f.range {
            v["range"] = json!([lo, hi]);
        }
        v
    };
    let ty = |t: &TypeDef| {
        json!({
            "name": t.name,
            "role": if t.role == Role::Condition { "condition" } else { "action" },
            "origin": if t.origin == Origin::Retail { "retail" } else { "engine" },
            "handler": if t.handler == Handler::Interpreter { "interpreter" } else { "victory" },
            "campaign_only": t.campaign_only,
            "fields": t.fields.iter().map(field).collect::<Vec<_>>(),
        })
    };
    let kinds = [
        Kind::Group,
        Kind::Player,
        Kind::Path,
        Kind::Volume,
        Kind::Task,
        Kind::Point,
        Kind::Timer,
        Kind::Flag,
        Kind::Counter,
        Kind::CampaignCounter,
        Kind::Rule,
        Kind::ConditionList,
        Kind::Condition,
        Kind::Speech,
        Kind::PointSet,
        Kind::MapText,
        Kind::Message,
        Kind::Team,
        Kind::ShipName,
        Kind::Bool,
        Kind::Number,
        Kind::Enum(&[]),
        Kind::Text,
    ];
    json!({
        "version": 1,
        "kinds": kinds.iter().map(|k| json!({"name": k.name(), "placeholders": k.placeholders()})).collect::<Vec<_>>(),
        "types": RULE_TYPES.iter().map(ty).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The strings of the match arms of `src` (`"A" | "B" => ...`, `"C" if guard => ...`): the Types the interpreter handles.
    fn arms(src: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for line in src.lines().map(str::trim).filter(|l| l.starts_with('"')) {
            let Some((pattern, _)) = line.split_once("=>") else { continue };
            let pattern = pattern.split(" if ").next().unwrap_or(pattern);
            for alt in pattern.split(" | ") {
                let alt = alt.trim();
                if alt.len() >= 2 && alt.starts_with('"') && alt.ends_with('"') {
                    out.insert(alt[1..alt.len() - 1].to_string());
                }
            }
        }
        out
    }

    fn names(role: Role, handler: Handler) -> BTreeSet<String> {
        RULE_TYPES.iter().filter(|t| t.role == role && t.handler == handler).map(|t| t.name.to_string()).collect()
    }

    #[test]
    fn schema_matches_the_interpreter() {
        // Conditions that the arms of `fn cond` also test through `t == "..."` guards are listed in their arm's pattern, so the arms
        // are the whole vocabulary.
        // The engine-added `OP ` Types go through one guard arm in cond.rs / act.rs into ext.rs, which has their arms.
        let ext: BTreeSet<String> = arms(include_str!("ext.rs")).into_iter().filter(|n| n.starts_with("OP ")).collect();
        let all: BTreeSet<String> = RULE_TYPES.iter().map(|t| t.name.to_string()).collect();
        let unlisted: Vec<_> = ext.difference(&all).collect();
        assert!(unlisted.is_empty(), "ext.rs handles Types with no entry in RULE_TYPES (schema.rs): {unlisted:?}");
        for (role, src, file) in [(Role::Condition, include_str!("cond.rs"), "cond.rs"), (Role::Action, include_str!("act.rs"), "act.rs")] {
            let mut handled = arms(src);
            handled.extend(ext.iter().filter(|n| RULE_TYPES.iter().any(|t| t.role == role && t.name == n.as_str())).cloned());
            let declared = names(role, Handler::Interpreter);
            let missing: Vec<_> = handled.difference(&declared).collect();
            let stale: Vec<_> = declared.difference(&handled).collect();
            assert!(missing.is_empty(), "{file} handles Types with no entry in RULE_TYPES (schema.rs): {missing:?}");
            assert!(stale.is_empty(), "RULE_TYPES (schema.rs) has Types {file} does not handle: {stale:?}");
        }
    }

    #[test]
    fn victory_rs_and_the_victory_entries_agree() {
        // The arms of victory.rs's `match c["Type"]`; its other `"..." =>` lines are not Types.
        let read: BTreeSet<String> =
            arms(include_str!("../../victory.rs")).into_iter().filter(|n| RULE_TYPES.iter().any(|t| t.name == n.as_str()) || !Equivalence::NAMES.contains(&n.as_str())).collect();
        let declared = names(Role::Condition, Handler::Victory);
        // It also reads some Types the interpreter shares (Group Destroyed, Enter Volume, ...): those need an entry, of either handler.
        let all = names(Role::Condition, Handler::Interpreter).into_iter().chain(declared.iter().cloned()).collect::<BTreeSet<_>>();
        let missing: Vec<_> = read.difference(&all).collect();
        let stale: Vec<_> = declared.difference(&read).collect();
        assert!(missing.is_empty(), "victory.rs reads Types with no entry in RULE_TYPES: {missing:?}");
        assert!(stale.is_empty(), "RULE_TYPES has Handler::Victory Types victory.rs does not read: {stale:?}");
    }

    #[test]
    fn equivalence_spellings_and_epsilons_are_the_callers_today() {
        for (name, e) in Equivalence::NAMES.iter().zip([Equivalence::GreaterThan, Equivalence::LessThan, Equivalence::EqualTo]) {
            assert_eq!(Equivalence::parse(name), Some(e));
        }
        assert_eq!(Equivalence::parse(">"), None, "only the three retail spellings parse");
        assert!(!cmp_str("greater than", 2.0, 1.0), "case matters, as before");
        // Interpreter epsilon 1e-4, skirmish end rules 1e-3 (each caller passes its own).
        assert!(Equivalence::EqualTo.holds(1.0, 1.00005, 1e-4) && !Equivalence::EqualTo.holds(1.0, 1.0002, 1e-4));
        assert!(Equivalence::EqualTo.holds(1.0, 1.0005, 0.001) && !Equivalence::EqualTo.holds(1.0, 1.002, 0.001));
        assert!(crate::rules::cmp("Equal To", 1.0, 1.00005) && !crate::rules::cmp("Equal To", 1.0, 1.0002));
        assert!(!crate::rules::cmp("Nonsense", 1.0, 1.0));
    }

    fn cmp_str(eq: &str, a: f32, b: f32) -> bool {
        crate::rules::cmp(eq, a, b)
    }

    /// Decided (#537): JSON booleans in a rule's boolean field are rejected. `--check-mission` reports them as errors, and every
    /// reader (this one and victory.rs's `Exists?`) reads only the string "TRUE" (any case) as true, so a JSON `true` reads FALSE.
    #[test]
    fn a_json_true_in_a_rule_reads_false() {
        use serde_json::json;
        assert!(crate::rules::b(&json!({"Boolean Value": "TRUE"}), "Boolean Value"));
        assert!(crate::rules::b(&json!({"Boolean Value": "true"}), "Boolean Value"));
        assert!(!crate::rules::b(&json!({"Boolean Value": true}), "Boolean Value"));
        assert!(!crate::rules::b(&json!({}), "Boolean Value"));
    }

    #[test]
    fn entries_are_unique_and_sane() {
        let mut seen = BTreeSet::new();
        for t in RULE_TYPES {
            assert!(seen.insert((t.role == Role::Condition, t.name)), "duplicate entry {}", t.name);
            let mut fields = BTreeSet::new();
            for f in t.fields {
                assert!(fields.insert(f.name), "{}: duplicate field {}", t.name, f.name);
                assert!(f.range.is_none() || f.kind == Kind::Number, "{}.{}: only numbers have a range", t.name, f.name);
            }
        }
        // Skirmish end conditions are not interpreter arms, so they must not be.
        let cond_arms = arms(include_str!("cond.rs"));
        for t in RULE_TYPES.iter().filter(|t| t.handler == Handler::Victory) {
            assert!(!cond_arms.contains(t.name), "{} is read by victory.rs, not cond.rs", t.name);
        }
    }

    #[test]
    fn json_lists_every_type() {
        let v = to_json();
        assert_eq!(v["types"].as_array().map(Vec::len), Some(RULE_TYPES.len()));
        let hold = v["types"].as_array().unwrap().iter().find(|t| t["name"] == "Set Group/Unit Hold Position").unwrap();
        assert_eq!(hold["role"], "action");
        assert_eq!(hold["fields"][1]["default"], "TRUE");
        assert_eq!(hold["fields"][1]["required"], false);
    }
}
