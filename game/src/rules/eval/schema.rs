//! The rule language as data: every condition and action `Type` the mission rules can use, with its fields, the kind of each field
//! and the value the interpreter reads when a field is missing.
//!
//! The vocabulary is the `rule_types!` list below: one `RuleType` variant per Type, with its role and exact spelling. The interpreter
//! (`act.rs`, `cond.rs`, `ext.rs`) matches on the variants; this table describes them (`--dump-rule-schema`) and is what
//! `--check-mission` validates a mission against (`check.rs`). It does not drive the interpreter: `act.rs` / `cond.rs` stay the
//! authority on behaviour, and the unit tests `every_type_has_one_entry` and `the_interpreter_handles_every_interpreter_type` fail when
//! a variant has no entry here or the interpreter has no arm for an entry.
//!
//! To add a `Type`: add its line to `rule_types!`, write its arm in `act.rs` / `cond.rs`, then append one `cond(..)` / `act(..)` entry
//! to `RULE_TYPES` (engine-added Types use `.engine()`). Retail field names are kept exactly as the TWT exports them, typos included.

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
    /// Whether a name of this kind comes into being by being written (flags, timers, counters): an unknown one in a rule is not an error.
    pub fn creatable(self) -> bool {
        matches!(self, Kind::Flag | Kind::Timer | Kind::Counter)
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

/// One variant per Type of the rule language, with its role and its exact spelling. This list is the only place a Type's name is
/// written as a string in the interpreter: `act.rs`, `cond.rs` and `ext.rs` match on the variants, the table below describes them.
macro_rules! rule_types {
    ($($variant:ident = $role:ident $name:literal,)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum RuleType {
            $($variant,)*
        }
        impl RuleType {
            /// Every Type, conditions first, in the order of the list.
            pub const ALL: &'static [RuleType] = &[$(RuleType::$variant,)*];
            /// The spelling the mission files use.
            pub const fn name(self) -> &'static str {
                match self {
                    $(RuleType::$variant => $name,)*
                }
            }
            pub const fn role(self) -> Role {
                match self {
                    $(RuleType::$variant => Role::$role,)*
                }
            }
            /// The Type a name spells in the given role; `None` for anything the language does not have (the caller warns, as before).
            pub fn parse(role: Role, name: &str) -> Option<RuleType> {
                match (role, name) {
                    $((Role::$role, $name) => Some(RuleType::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

rule_types! {
    WorldInitialize = Condition "World Initialize",
    FlagCondition = Condition "Flag Condition",
    TimerCondition = Condition "Timer Condition",
    SpeechEventPlayedOnce = Condition "Speech Event Played Once",
    SpeechEventNotPlayedYet = Condition "Speech Event Not Played Yet",
    IsGroupInVolume = Condition "Is Group In Volume",
    EnterVolume = Condition "Enter Volume",
    ExitVolume = Condition "Exit Volume",
    TeamMemberEntersVolume = Condition "Team Member Enters Volume",
    GroupDestroyed = Condition "Group Destroyed",
    GroupUnderAttack = Condition "Group Under Attack",
    GroupUnitFiredXShots = Condition "Group/Unit Fired X Shots",
    GroupUnitVitalSectionHasXDamage = Condition "Group/Unit Vital Section has >,<,= X damage",
    DoesGroupContainUnitName = Condition "Does Group Contain Unit Name",
    IsShipInTow = Condition "Is Ship In Tow",
    IsGroupAAttackingGroupB = Condition "Is Group A attacking Group B",
    GroupUnitHitAtLeastXTimes = Condition "Group/Unit Hit at least x Times",
    GroupUnitHitAtLeastXTimesByPlayerWithEqivalence = Condition "Group/Unit Hit at least x Times by Player ( with eqivalence )",
    PlayerHasHitGroupUnitAtLeastXTimes = Condition "Player has hit Group/Unit at least x Times",
    GroupHasXMembers = Condition "Group has X members",
    GroupUnitHasXDamage = Condition "Group/Unit has >,<,= X damage",
    GroupUnitContainsNoMissionEssentialShips = Condition "Group/Unit contains no mission essential ships",
    UnitFlagTexture = Condition "Unit Flag Texture",
    GroupToGroupDistance = Condition "Group to Group Distance",
    GroupUnitDocked = Condition "Group/Unit Docked",
    GroupUnitIsDocked = Condition "Group/Unit Is Docked",
    UnitIsWithinAnyNebula = Condition "Unit is Within any Nebula",
    AllUnitsFromGroupHaveEnteredTriggerVolumeOnce = Condition "All Units from Group have entered trigger volume Once",
    PlayerVsPlayerCaptureCount = Condition "Player Vs Player capture count",
    TeamHasCapturedAShipFromGroupUnit = Condition "Team has captured a ship from Group/Unit",
    TeamHasDestroyedAShipFromGroupUnit = Condition "Team has destroyed a ship from Group/Unit",
    UnitFromGroupEntersTriggerVolumeOncePerUnit = Condition "Unit from Group enters trigger volume ( Once per Unit )",
    TeamHasXPoints = Condition "Team has X points",
    PlayerHasNoLifeboats = Condition "Player has no lifeboats",
    IsStarmapOpen = Condition "Is Starmap Open",
    Mission9IfMortarExplodesWithinArea = Condition "Mission 9 - If Mortar explodes within Area",
    PlayerKilledAObject = Condition "Player Killed A Object",
    NoHumanControlledShipsRemain = Condition "No Human Controlled Ships Remain",
    NoTeamHasShips = Condition "No Team Has Ships",
    SkirmishGameComplete = Condition "Skirmish Game Complete",
    TeamGameComplete = Condition "Team Game Complete",
    TeamXHasNoShips = Condition "Team X has no ships",
    OPAnyOf = Condition "OP Any Of",
    OPNot = Condition "OP Not",
    OPCounterCondition = Condition "OP Counter Condition",
    OPCampaignCounterCondition = Condition "OP Campaign Counter Condition",
    OPGroupIsCloaked = Condition "OP Group Is Cloaked",
    OPGroupSpeed = Condition "OP Group Speed",
    OPTaskState = Condition "OP Task State",
    StartTimer = Action "Start Timer",
    StopTimer = Action "Stop Timer",
    SetFlagAction = Action "Set Flag Action",
    PlaySpeechEvent = Action "Play speech event",
    PlayMusicTrack = Action "Play Music Track",
    SetObjectiveTaskActiveState = Action "Set Objective Task Active State",
    SetObjectiveTaskFailedState = Action "Set Objective Task Failed State",
    SetObjectiveTaskCompleteState = Action "Set Objective Task Complete State",
    SetCurrentObjectivePoint = Action "Set Current Objective Point",
    SetCurrentObjectivePointOnShip = Action "Set Current Objective Point On Ship",
    SetCurrentObjectivePointVisibleOnStarmap = Action "Set Current Objective Point Visible On Starmap",
    SetMapTextVisibility = Action "Set Map Text Visibility",
    GroupToFollowPath = Action "Group to follow path",
    SetupSpaceAnimalFlock = Action "Setup Space Animal Flock",
    SetupAsteroidBelt = Action "Setup Asteroid Belt",
    SetGroupThrottlePercent = Action "Set Group Throttle Percent",
    DestroyGroupUnit = Action "Destroy Group/Unit",
    TransferGroupUnitToGroup = Action "Transfer Group/Unit to Group",
    GroupAToRamGroupB = Action "GroupA to Ram GroupB",
    TeleportGroupUnit = Action "Teleport Group/Unit",
    DockShips = Action "Dock Ships",
    SetGroupUnitDockable = Action "Set Group/Unit Dockable",
    SetGroupUnitBoardable = Action "Set Group/Unit Boardable",
    ToggleIslandRepairWhenDocked = Action "Toggle Island Repair When Docked",
    SetDockTime = Action "Set Dock Time",
    SetGroupSpaceObjectsVelocity = Action "Set Group Space Objects velocity",
    SetMaxThrottlePercentMaxUserSettable = Action "Set Max Throttle Percent (Max user settable)",
    SetShipTopSpeedPercentage = Action "Set Ship Top Speed Percentage",
    SetShipName = Action "Set Ship Name",
    SetGroupUnitAIStance = Action "Set Group/Unit AI Stance",
    SetGroupUnitOwner = Action "Set Group/Unit Owner",
    SetGroupUnitVisibility = Action "Set Group/Unit Visibility",
    SetGroupUnitVulnerability = Action "Set Group/Unit Vulnerability",
    SetShipsFlagTexture = Action "Set Ships Flag Texture",
    SetShipBannerType = Action "Set Ship Banner Type",
    GroupAToAttackGroupB = Action "GroupA to attack GroupB",
    ClearAllAICommands = Action "Clear all AI commands",
    DamageGroupUnitByXPercent = Action "Damage Group/Unit by X percent",
    SetGroupUnitHoldPosition = Action "Set Group/Unit Hold Position",
    ResetHitCount = Action "Reset Hit Count",
    SetFleetFormationType = Action "Set FleetFormation Type",
    SetFleetHoldFormation = Action "Set Fleet Hold Formation",
    SetFleetPrimaryShip = Action "Set Fleet Primary Ship",
    SetFleetHoldFire = Action "Set Fleet Hold Fire",
    SetAllianceBetweenPlayerAAndPlayerBToTRUEFALSE = Action "Set alliance between PlayerA and PlayerB to TRUE/FALSE",
    TowShip = Action "Tow Ship",
    BreakTow = Action "Break Tow",
    NISStart = Action "NIS Start",
    NISEnd = Action "NIS End",
    NISSetCameraPath = Action "NIS Set Camera Path",
    NISSetCameraSpeed = Action "NIS Set Camera Speed",
    NISSetTransitionCameraSpeed = Action "NIS Set Transition Camera Speed",
    NISFocusCameraOnGroupUnit = Action "NIS Focus camera on Group/Unit",
    NISFocusCameraOnPoint = Action "NIS Focus camera on Point",
    NISAttachCamera = Action "NIS Attach Camera",
    NISPositionCameraRelativeToObject = Action "NIS Position Camera Relative to Object",
    NISFocusOnMainShip = Action "NIS Focus on Main Ship",
    SetGroupUnitCloakState = Action "Set Group/Unit Cloak State",
    FocusCameraOnGroup = Action "Focus Camera On Group",
    AddVictoryPointsForSinglePlayer = Action "Add Victory Points for SinglePlayer",
    GotoNextLevel = Action "Goto Next Level",
    EndGame = Action "End Game",
    CreateReleaseEventEffect = Action "Create/Release Event Effect",
    OpenHUDTextureOverlay = Action "Open HUD Texture Overlay",
    CloseHUDTextureOverlay = Action "Close HUD Texture Overlay",
    OpenWeaponBar = Action "Open Weapon Bar",
    PlaySpecialEffect = Action "Play Special Effect",
    SetLifeboatCreationState = Action "Set Lifeboat Creation State",
    StateInitSetupShip = Action "*State Init* Setup Ship",
    StateInitSetupIsland = Action "*State Init* Setup Island",
    SetGroupUnitMissionEssential = Action "Set Group/Unit Mission Essential",
    SetRadarActiveState = Action "Set Radar Active State",
    SetGroupUnitTowable = Action "Set Group/Unit Towable",
    SetGroupUnitMovable = Action "Set Group/Unit Movable",
    SetCollidable = Action "SetCollidable",
    SetGroupUnitBorderZone = Action "Set Group/Unit Border Zone",
    ClearGroupUnitBorderZone = Action "Clear Group/Unit Border Zone",
    SetGroupUnitBorderZoneReturnPoint = Action "Set Group/Unit Border Zone Return Point",
    CrewSpeechToggleOnOff = Action "Crew Speech - Toggle On Off",
    CrewSpeechHelmOffCourse = Action "Crew Speech - Helm Off Course",
    OpenCrewAndArmsScreens = Action "Open Crew and Arms Screens",
    SetIsInCalyanAbyss = Action "Set Is In Calyan Abyss",
    SetGroupUnitAICaptain = Action "Set Group/Unit AI Captain",
    NISZoom = Action "NIS Zoom",
    NISToggleNISModeGunAccuracy = Action "NIS Toggle NIS mode Gun Accuracy",
    NISToggleAllObjectsVisibility = Action "NIS Toggle All Objects Visibility",
    SetNebulaFadeoutDistance = Action "Set Nebula Fadeout Distance",
    SetNebulaToLockObjects = Action "Set Nebula to Lock Objects",
    SetGroupUnitWarningShotMode = Action "Set Group/Unit Warning Shot Mode",
    TUTORIALPauseWhenStarmapOpens = Action "TUTORIAL - Pause when starmap opens?",
    StateInitSetupEtheriumCurrent = Action "*State Init* Setup Etherium Current",
    Mission9DoDarkMatterExplosion = Action "Mission 9 - Do Dark Matter Explosion",
    Mission9TeleportLongboat = Action "Mission 9 - Teleport Longboat",
    DragonSetAIStance = Action "Dragon - Set AI Stance",
    DragonSetDamageThreshold = Action "Dragon - Set Damage Threshold",
    GrantTeamXPoints = Action "Grant Team X Points",
    SetupTeamObjective = Action "Setup Team Objective",
    TeamXWins = Action "Team X Wins",
    RemainingTeamWins = Action "Remaining Team Wins",
    ResetShotsFiredCount = Action "Reset Shots Fired Count",
    OPWait = Action "OP Wait",
    OPWaitUntilSpeechDone = Action "OP Wait Until Speech Done",
    OPSetCounter = Action "OP Set Counter",
    OPAddToCounter = Action "OP Add To Counter",
    OPSetRuleActive = Action "OP Set Rule Active",
    OPSetCampaignCounter = Action "OP Set Campaign Counter",
    OPAddToCampaignCounter = Action "OP Add To Campaign Counter",
    OPBoardShip = Action "OP Board Ship",
    OPAbandonShip = Action "OP Abandon Ship",
    OPTeleport = Action "OP Teleport",
    OPEtheriumCurrent = Action "OP Etherium Current",
    OPFade = Action "OP Fade",
    OPPlaceMines = Action "OP Place Mines",
    OPFollowGroup = Action "OP Follow Group",
    StateInitSetupNebula = Action "*State Init* Setup Nebula",
}

use RuleType as T;

impl RuleType {
    /// Added by OpenProcyon (`Origin::Engine`): interpreted in `ext.rs`.
    pub fn is_engine(self) -> bool {
        schema_def(self).origin == Origin::Engine
    }
    /// The Type of a condition or action object, read from its `"Type"` field.
    pub fn of(role: Role, entry: &Value) -> Option<RuleType> {
        RuleType::parse(role, crate::rules::s(entry, "Type"))
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
    pub ty: RuleType,
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
    const fn new(ty: RuleType, fields: &'static [Field]) -> TypeDef {
        TypeDef { ty, name: ty.name(), role: ty.role(), origin: Origin::Retail, handler: Handler::Interpreter, campaign_only: false, fields }
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

const fn cond(ty: RuleType, fields: &'static [Field]) -> TypeDef {
    TypeDef::new(ty, fields)
}
const fn act(ty: RuleType, fields: &'static [Field]) -> TypeDef {
    TypeDef::new(ty, fields)
}

/// Every Type of the rule language. Append a new Type's entry at the end of its section.
pub static RULE_TYPES: &[TypeDef] = &[
    // ---- Conditions -------------------------------------------------------------------------------------------------
    cond(T::WorldInitialize, &[]),
    cond(T::FlagCondition, &[flag("Flag Name"), boolean("Boolean Value")]),
    cond(T::TimerCondition, &[timer("Timer Name"), choice("Equivalence", COMPARE), num("Time in seconds")]),
    cond(T::SpeechEventPlayedOnce, &[speech("Speech Event Name")]),
    cond(T::SpeechEventNotPlayedYet, &[speech("Speech Event Name")]),
    cond(T::IsGroupInVolume, &[grp("Group Name"), vol("Volume Name"), boolean("Entire Group").opt()]),
    cond(T::EnterVolume, &[grp("Group Name"), vol("Volume Name")]),
    cond(T::ExitVolume, &[grp("Group Name"), vol("Volume Name")]),
    cond(T::TeamMemberEntersVolume, &[team("Team Name"), vol("Volume Name")]),
    cond(T::GroupDestroyed, &[grp("Group Name")]),
    cond(T::GroupUnderAttack, &[grp("Group Name")]),
    cond(T::GroupUnitFiredXShots, &[grp("Group/Unit Name"), choice("Equivalence", COMPARE), num("Number Of Rounds Fired")]),
    cond(T::GroupUnitVitalSectionHasXDamage, &[grp("Group/Unit Name"), text("Vital Section"), choice("Equivalence", COMPARE), frac("Damage Percent")]),
    cond(T::DoesGroupContainUnitName, &[grp("Unit Name"), boolean("Exists?")]),
    cond(T::IsShipInTow, &[grp("Towing Group"), grp("Target to Tow"), boolean("Boolean")]),
    cond(T::IsGroupAAttackingGroupB, &[grp("GroupA Name"), grp("GroupB Name")]),
    cond(T::GroupUnitHitAtLeastXTimes, &[grp("Group/Unit Name"), num("Number Of Times Hit")]),
    cond(T::GroupUnitHitAtLeastXTimesByPlayerWithEqivalence, &[grp("Group/Unit Name"), plr("Player Name"), choice("Equivalence", COMPARE), num("Number Of Times Hit")]),
    cond(T::PlayerHasHitGroupUnitAtLeastXTimes, &[plr("Player Name"), grp("Group/Unit Name"), num("Number Of Times Hit")]),
    cond(T::GroupHasXMembers, &[grp("Group Name"), choice("Equivalence", COMPARE), num("number")]),
    cond(T::GroupUnitHasXDamage, &[grp("Group/Unit Name"), choice("Equivalence", COMPARE), frac("Damage Percent")]),
    cond(T::GroupUnitContainsNoMissionEssentialShips, &[grp("Group Name")]),
    cond(T::UnitFlagTexture, &[grp("Unit Name"), text("Flag Type"), boolean("Boolean")]),
    cond(T::GroupToGroupDistance, &[grp("GroupA Name"), grp("GroupB Name"), choice("Equivalence", COMPARE), num("Distance")]),
    cond(T::GroupUnitDocked, &[grp("Dockers Group/Unit Name"), grp("Targets Group/Unit Name")]),
    cond(T::GroupUnitIsDocked, &[grp("Dockers Group/Unit Name"), grp("Targets Group/Unit Name")]),
    cond(T::UnitIsWithinAnyNebula, &[grp("Unit Name")]),
    cond(
        T::AllUnitsFromGroupHaveEnteredTriggerVolumeOnce,
        &[grp("Group Name"), vol("Volume Name"), plr("Player Name").ignored(), num("Object that have already entered - Size").ignored()],
    ),
    cond(T::PlayerVsPlayerCaptureCount, &[plr("Player Name A"), plr("Player Name B"), choice("Equivalence", COMPARE), num("Number Of Captures")]),
    cond(T::TeamHasCapturedAShipFromGroupUnit, &[team("Team Name"), grp("Group/Unit"), num("Number Of Captures")]),
    cond(T::TeamHasDestroyedAShipFromGroupUnit, &[team("Team Name"), grp("Group/Unit"), num("Number Of Ships Destroyed")]),
    cond(
        T::UnitFromGroupEntersTriggerVolumeOncePerUnit,
        &[
            grp("Group Name"),
            vol("Volume Name"),
            team("Team Name"),
            num("Number of objects entered, waiting to report").ignored(),
            num("Object that have already entered - Size").ignored(),
        ],
    ),
    cond(T::TeamHasXPoints, &[team("Team Name"), num("Points")]),
    cond(T::PlayerHasNoLifeboats, &[plr("Player Name")]),
    cond(T::IsStarmapOpen, &[]),
    cond(T::Mission9IfMortarExplodesWithinArea, &[vol("Volume Name")]).campaign(),
    cond(T::PlayerKilledAObject, &[plr("Player Name"), choice("World Object Type", &["Asteroid", "Ship"])]),
    // Skirmish end conditions: read by victory.rs from the map's `victory` rules.
    cond(T::NoHumanControlledShipsRemain, &[]).victory(),
    cond(T::NoTeamHasShips, &[]).victory(),
    cond(T::SkirmishGameComplete, &[]).victory(),
    cond(T::TeamGameComplete, &[]).victory(),
    cond(T::TeamXHasNoShips, &[team("Team Name")]).victory(),
    // Engine-added (not retail): combinators and counters, interpreted in `ext.rs`.
    cond(T::OPAnyOf, &[conditions("Conditions")]).engine(),
    cond(T::OPNot, &[condition("Condition")]).engine(),
    cond(T::OPCounterCondition, &[counter("Counter Name"), choice("Equivalence", COMPARE), num("Value")]).engine(),
    cond(T::OPCampaignCounterCondition, &[campaign_counter("Counter Name"), choice("Equivalence", COMPARE), num("Value")]).engine(),
    cond(T::OPGroupIsCloaked, &[grp("Group Name"), boolean("Boolean").dflt("TRUE").opt()]).engine(),
    cond(T::OPGroupSpeed, &[grp("Group Name"), choice("Which", &["Fastest", "Slowest"]), choice("Equivalence", COMPARE), num("Speed")]).engine(),
    cond(T::OPTaskState, &[task("Objective Task"), choice("State", &["Active", "Complete", "Failed"])]).engine(),
    // ---- Actions ----------------------------------------------------------------------------------------------------
    act(T::StartTimer, &[timer("Timer Name")]),
    act(T::StopTimer, &[timer("Timer Name")]),
    act(T::SetFlagAction, &[flag("Flag Name"), boolean("Boolean Value")]),
    act(T::PlaySpeechEvent, &[speech("Speech Event Name")]),
    act(
        T::PlayMusicTrack,
        &[text("File Name"), boolean("Crossfade transition").ignored(), num("Fade In Time ( secs )"), num("Fade Out Time ( secs )"), frac("New Volume ( 0 to 1 )")],
    ),
    act(T::SetObjectiveTaskActiveState, &[task("Objective Task"), boolean("Active State")]),
    act(T::SetObjectiveTaskFailedState, &[task("Objective Task"), boolean("Failed State")]),
    act(T::SetObjectiveTaskCompleteState, &[task("Objective Task"), boolean("Complete State")]),
    act(T::SetCurrentObjectivePoint, &[point("Objective Point")]).campaign(),
    act(T::SetCurrentObjectivePointOnShip, &[grp("Group Name")]).campaign(),
    act(T::SetCurrentObjectivePointVisibleOnStarmap, &[boolean("Visible On Starmap")]).campaign(),
    act(T::SetMapTextVisibility, &[maptext("Map Text Name"), boolean("Visible On Starmap")]),
    act(T::GroupToFollowPath, &[grp("Group Name"), path("Path Name"), boolean("Find Closest Point"), choice("Follow Mode", FOLLOW)]),
    act(T::SetupSpaceAnimalFlock, &[grp("Group Name"), path("Path Name"), boolean("Find Closest Point"), choice("Follow Mode", FOLLOW), num("Velocity")]),
    act(
        T::SetupAsteroidBelt,
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
    act(T::SetGroupThrottlePercent, &[grp("Group Name"), frac("Throttle Percent")]),
    act(T::DestroyGroupUnit, &[grp("Group Name")]),
    act(T::TransferGroupUnitToGroup, &[grp("Group/Unit to transfer"), grp("Target Group")]),
    act(T::GroupAToRamGroupB, &[grp("Group Name"), grp("Group Unit Target")]),
    act(T::TeleportGroupUnit, &[grp("Group/Unit Name"), pset("Point Set Name")]),
    act(T::DockShips, &[grp("Docking Group"), grp("Target to Dock to")]),
    act(T::SetGroupUnitDockable, &[grp("Group Name"), boolean("Boolean")]),
    act(T::SetGroupUnitBoardable, &[grp("Group Name"), boolean("Boolean")]),
    act(T::ToggleIslandRepairWhenDocked, &[grp("Group Name"), boolean("Repair when docked")]),
    act(T::SetDockTime, &[grp("Docking GroupUnit"), num("Dock Time")]),
    act(T::SetGroupSpaceObjectsVelocity, &[grp("Group Name"), num("Velocity")]),
    act(T::SetMaxThrottlePercentMaxUserSettable, &[grp("Group Name"), frac("Throttle Percent")]),
    act(T::SetShipTopSpeedPercentage, &[grp("Group Name"), frac("Throttle Speed")]),
    act(T::SetShipName, &[grp("Group/Unit Name"), shipname("New Ship Name")]),
    act(T::SetGroupUnitAIStance, &[grp("Group/Unit Name"), choice("AI Stance", STANCE)]),
    act(T::SetGroupUnitOwner, &[grp("Group/Unit Name"), plr("New Owner")]),
    act(T::SetGroupUnitVisibility, &[grp("Group/Unit Name"), boolean("Boolean")]),
    act(T::SetGroupUnitVulnerability, &[grp("Group Name"), frac("Invulnerable Percent")]),
    act(T::SetShipsFlagTexture, &[grp("Group Name"), text("Flag Texture")]),
    act(T::SetShipBannerType, &[grp("Group/Unit Name"), text("Banner Type")]),
    act(T::GroupAToAttackGroupB, &[grp("Attack Group"), grp("Target Group")]),
    act(T::ClearAllAICommands, &[grp("Group/Unit Name")]),
    act(T::DamageGroupUnitByXPercent, &[grp("Group Name"), frac("Percent")]),
    act(T::SetGroupUnitHoldPosition, &[grp("Group/Unit Name"), boolean("Boolean").opt().dflt("TRUE")]),
    act(T::ResetHitCount, &[grp("Group/Unit Name")]),
    act(T::SetFleetFormationType, &[plr("Player Name"), choice("Formation Type", FORMATION), boolean("Hold Formation")]),
    act(T::SetFleetHoldFormation, &[plr("Player Name"), boolean("Hold Formation")]),
    act(T::SetFleetPrimaryShip, &[grp("Group/Unit Name")]).campaign(),
    act(T::SetFleetHoldFire, &[plr("PLAYER FLEET"), boolean("HOLD FIRE")]),
    act(T::SetAllianceBetweenPlayerAAndPlayerBToTRUEFALSE, &[plr("PLAYER A"), plr("PLAYER B"), boolean("SET ALLIANCE TO")]),
    act(T::TowShip, &[grp("Docking Group"), grp("Target to Tow")]),
    act(T::BreakTow, &[grp("TowerGroupA"), grp("TowerGroupB")]),
    act(T::NISStart, &[boolean("All Objects Visible").ignored(), boolean("Open NIS bars instantly").ignored()]).campaign(),
    act(T::NISEnd, &[]).campaign(),
    act(T::NISSetCameraPath, &[path("Path Name"), boolean("Jump to start").ignored()]).campaign(),
    act(T::NISSetCameraSpeed, &[num("Max Velocity"), num("Acceleration").ignored()]).campaign(),
    act(T::NISSetTransitionCameraSpeed, &[num("Max Velocity"), num("Acceleration").ignored()]).campaign(),
    act(T::NISFocusCameraOnGroupUnit, &[grp("GroupUnit Name")]).campaign(),
    act(T::NISFocusCameraOnPoint, &[pset("Point Set Name")]).campaign(),
    act(T::NISAttachCamera, &[grp("Group/Unit Name"), num("Distance"), num("Angle YZ"), num("Angle XY")]).campaign(),
    act(T::NISPositionCameraRelativeToObject, &[grp("Unit Name"), num("Distance"), num("Angle YZ"), num("Angle XY"), boolean("Jump to point").ignored()]).campaign(),
    act(T::NISFocusOnMainShip, &[]).campaign(),
    act(T::SetGroupUnitCloakState, &[grp("Group Name"), boolean("Cloak State"), boolean("AI Can Override")]),
    act(T::FocusCameraOnGroup, &[grp("GroupUnit Name"), num("Distance"), num("Relative Angle"), boolean("Use Transition").ignored()]).campaign(),
    act(T::AddVictoryPointsForSinglePlayer, &[num("Victory points to be added")]).campaign(),
    act(T::GotoNextLevel, &[text("World"), boolean("Display Loading String").ignored()]).campaign(),
    act(
        T::EndGame,
        &[
            message("Loser - Custom Message String ID"),
            message("Winner - Custom Message String ID").ignored(),
            boolean("Use Custom Message").ignored(),
            boolean("Show Stats Screen").ignored(),
        ],
    ),
    act(T::CreateReleaseEventEffect, &[pset("Point Set"), text("Effect Name"), boolean("Boolean State")]),
    act(T::OpenHUDTextureOverlay, &[text("Texture Base Name")]).campaign(),
    act(T::CloseHUDTextureOverlay, &[]).campaign(),
    act(T::OpenWeaponBar, &[boolean("OpenState")]).campaign(),
    act(T::PlaySpecialEffect, &[text("File Name"), boolean("Play As Dialog").ignored()]),
    act(T::SetLifeboatCreationState, &[grp("Group Name"), boolean("Boolean")]),
    act(
        T::StateInitSetupShip,
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
        T::StateInitSetupIsland,
        &[num("World Object ID"), num("Combat Strength").opt(), plr("Player/Owner").opt(), choice("Gunnery Level", GUNNERY).opt(), choice("AI Stance", STANCE).opt()],
    ),
    act(T::SetGroupUnitMissionEssential, &[grp("Group Name"), boolean("Boolean")]),
    act(T::SetRadarActiveState, &[boolean("Radar Active")]).campaign(),
    act(T::SetGroupUnitTowable, &[grp("Group Name"), boolean("Boolean")]),
    act(T::SetGroupUnitMovable, &[grp("Group Name"), boolean("Boolean")]),
    act(T::SetCollidable, &[grp("Group/Unit Name"), boolean("Boolean")]),
    act(T::SetGroupUnitBorderZone, &[grp("Group/Unit Name"), vol("Polygon Name")]),
    act(T::ClearGroupUnitBorderZone, &[grp("Group/Unit Name")]),
    act(T::SetGroupUnitBorderZoneReturnPoint, &[grp("Group/Unit Name"), pset("Pointset Name")]),
    act(T::CrewSpeechToggleOnOff, &[boolean("Crew Speech State State")]).campaign(),
    act(T::CrewSpeechHelmOffCourse, &[]).campaign(),
    act(T::OpenCrewAndArmsScreens, &[]).campaign(),
    act(T::SetIsInCalyanAbyss, &[boolean("Boolean Value")]).campaign(),
    act(T::SetGroupUnitAICaptain, &[grp("Group/Unit Name"), choice("AI Stance", &["Captain_Human"])]).campaign(),
    act(T::NISZoom, &[num("FOV"), num("Speed")]).campaign(),
    act(T::NISToggleNISModeGunAccuracy, &[boolean("Use NIS Gun Accuracy")]).campaign(),
    act(T::NISToggleAllObjectsVisibility, &[boolean("All Objects Visible")]).campaign(),
    act(T::SetNebulaFadeoutDistance, &[num("Fadeout Distance")]).campaign(),
    act(T::SetNebulaToLockObjects, &[boolean("Trap")]).campaign(),
    act(T::SetGroupUnitWarningShotMode, &[grp("GroupUnit Name"), boolean("Boolean")]),
    act(T::TUTORIALPauseWhenStarmapOpens, &[boolean("Pause when starmap opens")]).campaign(),
    act(T::StateInitSetupEtheriumCurrent, &[num("World Object ID").ignored(), text("Etherium Name").ignored(), path("Etherium Path").soft()]),
    act(T::Mission9DoDarkMatterExplosion, &[vol("Affected Area")]),
    act(T::Mission9TeleportLongboat, &[grp("Longboat Group Name")]).campaign(),
    act(T::DragonSetAIStance, &[grp("Group/Unit Name"), choice("AI Stance", STANCE)]),
    act(T::DragonSetDamageThreshold, &[grp("Group Name"), frac("Damage Threshold")]),
    act(T::GrantTeamXPoints, &[team("Team Name"), num("Points")]),
    act(T::SetupTeamObjective, &[team("Team Name"), point("Objective Point"), task("Objective Task").opt()]),
    act(T::TeamXWins, &[team("Team Name").opt()]),
    act(T::RemainingTeamWins, &[]),
    act(T::ResetShotsFiredCount, &[grp("Group/Unit Name")]),
    // Engine-added (not retail): sequences, counters and rule activation, interpreted in `ext.rs`.
    act(T::OPWait, &[num("Seconds")]).engine(),
    // An empty name waits for the whole speech queue.
    act(T::OPWaitUntilSpeechDone, &[speech("Speech Event Name").opt()]).engine(),
    act(T::OPSetCounter, &[counter("Counter Name"), num("Value")]).engine(),
    act(T::OPAddToCounter, &[counter("Counter Name"), num("Value")]).engine(),
    act(T::OPSetRuleActive, &[rule_name("Rule Name"), boolean("Boolean")]).engine(),
    act(T::OPSetCampaignCounter, &[campaign_counter("Counter Name"), num("Value")]).engine(),
    act(T::OPAddToCampaignCounter, &[campaign_counter("Counter Name"), num("Value")]).engine(),
    // Orders that act on ships: applied by the module that owns the behaviour (boarding.rs, lifeboats.rs, ...), see docs/modding.md.
    act(T::OPBoardShip, &[grp("Group Name"), grp("Target Group")]).engine(),
    act(T::OPAbandonShip, &[grp("Group Name")]).engine(),
    // `Heading` is a compass bearing in degrees in game axes (0 up the map along +y, 90 toward +x); absent, ships keep their headings.
    act(T::OPTeleport, &[grp("Group/Unit Name"), pset("Point Set Name"), num("Point Index").opt(), num("Heading").opt().dflt("")]).engine(),
    act(
        T::OPEtheriumCurrent,
        &[path("Etherium Path"), num("Strength").opt().dflt("").range(0.0, 1000.0), num("Width").opt().dflt("").range(0.0, 100000.0), boolean("Boolean").opt().dflt("")],
    )
    .engine(),
    act(T::OPFade, &[num("Seconds").range(0.0, 600.0)]).engine().campaign(),
    act(T::OPPlaceMines, &[pset("Point Set Name"), text("Bullet").opt().dflt("Bullet_StarMine_Med"), plr("Player/Owner").opt()]).engine(),
    act(T::OPFollowGroup, &[grp("Group Name"), grp("Target Group"), choice("Station", STATION), num("Distance").range(0.0, 100000.0)]).engine(),
    act(
        T::StateInitSetupNebula,
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
    RuleType::parse(role, name).map(schema_def)
}

/// The table entry of a Type (every variant has exactly one: `every_type_has_one_entry`).
pub fn schema_def(ty: RuleType) -> &'static TypeDef {
    RULE_TYPES.iter().find(|t| t.ty == ty).expect("every RuleType has an entry in RULE_TYPES")
}

/// One key of a rule or of a task (the object around the conditions and actions), as `--dump-rule-schema` lists it under `rule` and
/// `task`. Unlike the `Type` fields of conditions and actions, whose booleans are the strings `"TRUE"` / `"FALSE"`, these keys take
/// JSON booleans.
pub struct EnvelopeKey {
    pub name: &'static str,
    /// The JSON type the interpreter reads: `string`, `boolean` or `array`.
    pub json: &'static str,
    /// Other spellings it also reads (`"TRUE"` / `"FALSE"` strings for `Is Active`).
    pub also: &'static [&'static str],
    /// For an array: whether its items are `condition` or `action` objects.
    pub items: Option<Role>,
    /// What the interpreter reads when the key is missing, as JSON text.
    pub default: &'static str,
    pub origin: Origin,
    pub note: &'static str,
}

/// The keys of a rule in `mission.json` / `script.json` `rules`.
pub const RULE_KEYS: &[EnvelopeKey] = &[
    EnvelopeKey {
        name: "name",
        json: "string",
        also: &[],
        items: None,
        default: "\"\"",
        origin: Origin::Retail,
        note: "The rule's name, trimmed. `--fire-rule`, `--why` and `OP Set Rule Active` find a rule by it, so name every rule once.",
    },
    EnvelopeKey {
        name: "Run Once",
        json: "boolean",
        also: &[],
        items: None,
        default: "true",
        origin: Origin::Retail,
        note: "A JSON boolean: only `false` lets the rule fire again every time its conditions hold; anything else (or missing) fires it once.",
    },
    EnvelopeKey {
        name: "Is Active",
        json: "boolean",
        also: &["\"TRUE\"", "\"FALSE\""],
        items: None,
        default: "true",
        origin: Origin::Engine,
        note: "false: the rule is not evaluated until `OP Set Rule Active` turns it on. Retail writes true on every rule.",
    },
    EnvelopeKey {
        name: "conditions",
        json: "array",
        also: &[],
        items: Some(Role::Condition),
        default: "[]",
        origin: Origin::Retail,
        note: "Condition objects, all of which must hold; an empty list holds at once.",
    },
    EnvelopeKey {
        name: "actions",
        json: "array",
        also: &[],
        items: Some(Role::Action),
        default: "[]",
        origin: Origin::Retail,
        note: "Action objects, run in order when the conditions hold.",
    },
];

/// The keys of an objective task in `mission.json` `tasks`.
pub const TASK_KEYS: &[EnvelopeKey] = &[
    EnvelopeKey {
        name: "Name",
        json: "string",
        also: &[],
        items: None,
        default: "\"\"",
        origin: Origin::Retail,
        note: "What `Set Objective Task Active State` and the other task Types name.",
    },
    EnvelopeKey {
        name: "text",
        json: "string",
        also: &[],
        items: None,
        default: "\"\"",
        origin: Origin::Retail,
        note: "The objective's text (a string id of the mission's text).",
    },
    EnvelopeKey {
        name: "Active",
        json: "boolean",
        also: &[],
        items: None,
        default: "false",
        origin: Origin::Retail,
        note: "A JSON boolean: the task starts active. Only `true` counts.",
    },
    EnvelopeKey {
        name: "required",
        json: "boolean",
        also: &[],
        items: None,
        default: "false",
        origin: Origin::Engine,
        note: "With `\"victory\": \"objectives\"`: the mission is lost when a required task fails and won when all are complete.",
    },
    EnvelopeKey {
        name: "team",
        json: "string",
        also: &[],
        items: None,
        default: "\"\"",
        origin: Origin::Engine,
        note: "Skirmish script only: the team (`name_id`) a required task decides for.",
    },
    EnvelopeKey {
        name: "complete_when",
        json: "array",
        also: &[],
        items: Some(Role::Condition),
        default: "[]",
        origin: Origin::Engine,
        note: "Conditions that mark the task complete while it is active.",
    },
    EnvelopeKey {
        name: "fail_when",
        json: "array",
        also: &[],
        items: Some(Role::Condition),
        default: "[]",
        origin: Origin::Engine,
        note: "Conditions that mark the task failed (wins over `complete_when`).",
    },
];

fn envelope_json(keys: &[EnvelopeKey]) -> Value {
    keys.iter()
        .map(|k| {
            let mut v = json!({
                "name": k.name,
                "json": k.json,
                "default": serde_json::from_str::<Value>(k.default).unwrap_or(Value::Null),
                "origin": if k.origin == Origin::Retail { "retail" } else { "engine" },
                "note": k.note,
            });
            if !k.also.is_empty() {
                v["also"] = k.also.iter().filter_map(|a| serde_json::from_str::<Value>(a).ok()).collect();
            }
            if let Some(role) = k.items {
                v["items"] = json!(if role == Role::Condition { "condition" } else { "action" });
            }
            v
        })
        .collect()
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
        "rule": {"keys": envelope_json(RULE_KEYS)},
        "task": {"keys": envelope_json(TASK_KEYS)},
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
    fn the_dump_lists_the_keys_of_a_rule_and_of_a_task_with_their_json_types() {
        let v = to_json();
        let keys =
            |what: &str| v[what]["keys"].as_array().unwrap().iter().map(|k| (k["name"].as_str().unwrap().to_string(), k["json"].as_str().unwrap().to_string())).collect::<Vec<_>>();
        let pairs = |list: &[(&str, &str)]| list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(keys("rule"), pairs(&[("name", "string"), ("Run Once", "boolean"), ("Is Active", "boolean"), ("conditions", "array"), ("actions", "array")]));
        assert_eq!(
            keys("task"),
            pairs(&[
                ("Name", "string"),
                ("text", "string"),
                ("Active", "boolean"),
                ("required", "boolean"),
                ("team", "string"),
                ("complete_when", "array"),
                ("fail_when", "array")
            ])
        );
        let rule = |name: &str| v["rule"]["keys"].as_array().unwrap().iter().find(|k| k["name"] == name).unwrap().clone();
        assert_eq!(
            (rule("Run Once")["default"].clone(), rule("Is Active")["origin"].clone(), rule("Is Active")["also"].clone()),
            (json!(true), json!("engine"), json!(["TRUE", "FALSE"]))
        );
        assert_eq!((rule("conditions")["items"].clone(), rule("actions")["items"].clone()), (json!("condition"), json!("action")));
        // The Type tables are untouched by the addition.
        assert_eq!(v["types"].as_array().unwrap().len(), RULE_TYPES.len());
    }

    /// Fields an arm reads that its schema entry does not declare. The schema is the source of truth (retail spells some fields oddly on
    /// purpose); each pair here is a known mismatch to settle with the maintainer, not to be silently extended.
    const KNOWN_UNDECLARED_READS: &[(&str, &str)] = &[];

    /// Every interpreter arm reads only fields its schema entry declares. The accessors (`s`, `f`, `b`, `given`) log the keys they
    /// read; each Type runs once with every declared field filled in. Direct `v["Key"]` reads are not logged, and a read behind a guard
    /// that the filled values do not reach is not seen: this finds typos, it does not prove there are none.
    #[test]
    fn every_key_an_arm_reads_is_declared_in_its_schema_entry() {
        use serde_json::json;
        let w = vec![];
        let mut bad = vec![];
        let mut seen = BTreeSet::new();
        for t in RULE_TYPES.iter().filter(|t| t.handler == Handler::Interpreter) {
            let mut entry = serde_json::Map::new();
            entry.insert("Type".into(), json!(t.name));
            for fld in t.fields {
                let v = match fld.kind {
                    Kind::Bool => json!("TRUE"),
                    Kind::Number => json!(fld.range.map_or(1.0, |r| r.0 as f64)),
                    Kind::Enum(list) => json!(list.first().copied().unwrap_or("")),
                    Kind::Condition | Kind::ConditionList => continue,
                    _ => json!("Sample"),
                };
                entry.insert(fld.name.into(), v);
            }
            let entry = Value::Object(entry);
            let data = json!({"rules": [], "paths": {"Sample": [[0, 0, 0]]}, "volumes": [], "environment": {"point_sets": {"Sample": [{"pos": [0, 0, 0]}]}}});
            let mut sc = crate::rules::Script::new(&data, Value::Null);
            let read = crate::rules::read_log::record(|| match t.role {
                Role::Condition => {
                    sc.cond(&entry, &w, false);
                }
                Role::Action => sc.act(&entry, &w, &mut vec![], "t"),
            });
            seen.extend(read.iter().cloned());
            for key in read {
                if key != "Type" && t.field(&key).is_none() && !KNOWN_UNDECLARED_READS.contains(&(t.name, key.as_str())) {
                    bad.push(format!("{} reads undeclared field '{key}'", t.name));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
        assert!(seen.contains("Group Name") && seen.contains("Flag Name") && seen.len() > 100, "the read log works: {} keys", seen.len());
    }

    #[test]
    fn every_type_has_one_entry() {
        // The enum is the vocabulary, the table describes it: one entry per variant, in its own role, spelled as the enum says.
        assert_eq!(RuleType::ALL.len(), RULE_TYPES.len());
        let mut seen = BTreeSet::new();
        for t in RULE_TYPES {
            assert!(seen.insert(t.ty), "{} has two entries", t.name);
            assert_eq!((t.ty.name(), t.ty.role()), (t.name, t.role));
            assert_eq!(RuleType::parse(t.role, t.name), Some(t.ty), "{} does not parse back", t.name);
        }
        for ty in RuleType::ALL {
            assert!(seen.contains(ty), "{} has no entry in RULE_TYPES", ty.name());
        }
        assert_eq!(RuleType::parse(Role::Action, "Flag Condition"), None, "a name belongs to its own role");
        assert_eq!(RuleType::parse(Role::Condition, "flag condition"), None, "case matters, as before");
    }

    #[test]
    fn engine_types_are_the_op_types() {
        for t in RULE_TYPES {
            assert_eq!(t.ty.is_engine(), t.name.starts_with("OP "), "{}", t.name);
        }
    }

    /// The interpreter reaches every Type the table gives it (a Type without an arm would fall to "unsupported"), and warns about
    /// the ones it does not have, with the text it always used. Replaces the old source-text comparison of the match arms.
    #[test]
    fn the_interpreter_handles_every_interpreter_type() {
        use serde_json::json;
        let data = json!({"rules": [], "paths": [], "volumes": []});
        let mut s = crate::rules::Script::new(&data, Value::Null);
        let w = vec![];
        for t in RULE_TYPES.iter().filter(|t| t.handler == Handler::Interpreter) {
            let entry = json!({"Type": t.name});
            match t.role {
                Role::Condition => {
                    s.cond(&entry, &w, false);
                }
                Role::Action => s.act(&entry, &w, &mut vec![], "t"),
            }
        }
        // Empty fields may warn about a name (an unknown ship name, a rule); only "condition 'X'" / "action 'X'" mean no arm.
        let no_arm: Vec<_> = s.warned.iter().filter(|w| w.starts_with("condition '") || w.starts_with("action '")).collect();
        assert!(no_arm.is_empty(), "the interpreter has no arm for: {no_arm:?}");
        s.warned.clear();
        // The skirmish end conditions are victory.rs's: here they are unsupported, as before.
        s.cond(&json!({"Type": "Skirmish Game Complete"}), &w, false);
        s.cond(&json!({"Type": "OP Bogus"}), &w, false);
        s.act(&json!({"Type": "OP Bogus"}), &w, &mut vec![], "r");
        s.act(&json!({"Type": "Flag Condition"}), &w, &mut vec![], "r");
        s.cond(&json!({"Type": "Set Flag Action"}), &w, false);
        let mut warned: Vec<_> = s.warned.iter().cloned().collect();
        warned.sort();
        assert_eq!(
            warned,
            ["action 'Flag Condition' in rule 'r'", "action 'OP Bogus' in rule 'r'", "condition 'OP Bogus'", "condition 'Set Flag Action'", "condition 'Skirmish Game Complete'"]
        );
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
