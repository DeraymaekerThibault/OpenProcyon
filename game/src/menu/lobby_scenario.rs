//! Lobby scenarios (`--lobby <spec>`, tools/battle_check.py): a skirmish setup written as text and played through the same
//! model the lobby's pull-downs and Launch button use. The `--ship/--allies/--enemies` shortcut skips the lobby, so allied
//! computer slots, full lobbies and big budgets never reached a test through it.
//!
//! Spec: `map=border_dispute;points=1000;limit=10;slots=human,7*computer` (optionally `;seed=3`, `;fleet=Ship_Navy_ManOWar`)
//! - `map`: skirmish map id. `points`: victory points (50-1000, steps of 10). `limit`: fleet size (1-10).
//! - `seed`: varies the fleets the computers draw (default 0). The game is deterministic, so runs of one spec are identical;
//!   several seeds give several samples.
//! - `fleet`: the human's fleet, ship classes separated by `+` (`Ship_Navy_ManOWar` or `Ship_Navy_Cutter+Ship_Navy_Cutter`), instead
//!   of Auto Assign's. Not checked against the budget here: the lobby's own launch check still applies.
//! - `slots`: the lobby rows in order, `human`, `computer`, `open` or `closed`; `N*kind` repeats. The first row is always the
//!   human's. Rows left out stay as the lobby opens them. Computers take the team and fleet type the lobby gives them.
//!   A later `human` row is a second human player (#369 phase 2 E, replay tests): it takes the team, fleet type and flag a computer
//!   row would, and its fleet is built at launch as a computer's (PROVISIONAL: until phase 4 a remote player has no lobby of its own).
//!   Which human this machine plays is `--as-player <row>` (default the first).
//!
//! A spec starting with `{` is a whole [`SkirmishSetup`] in JSON instead (teams, fleet types, flags, gun swaps, ship names, named
//! crew), the form a command log's header holds.
use super::*;
use crate::scenario::Race;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotSpec {
    Human,
    Computer,
    Open,
    Closed,
}

/// What a tool launch sets the lobby up with: `--lobby <spec>`, or the setup a replayed recording holds.
#[derive(Resource, Clone, PartialEq, Debug)]
pub enum LobbyLaunch {
    Spec(LobbyScenario),
    Setup(SkirmishSetup),
}

impl LobbyLaunch {
    /// A `--lobby` argument: the JSON setup or the short spec.
    pub fn parse(spec: &str) -> Result<Self, String> {
        if spec.trim_start().starts_with('{') { SkirmishSetup::parse(spec).map(Self::Setup) } else { LobbyScenario::parse(spec).map(Self::Spec) }
    }
    /// Sets the lobby up, ready for Launch.
    pub fn apply(&self, sk: &mut Skirmish, lib: &Library) -> Result<(), String> {
        match self {
            Self::Spec(spec) => spec.apply(sk, lib),
            Self::Setup(setup) => setup.apply(sk, lib),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct LobbyScenario {
    pub map: String,
    pub points: u32,
    pub limit: usize,
    pub slots: Vec<SlotSpec>,
    pub seed: u64,
    pub fleet: Vec<String>,
}

impl LobbyScenario {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (mut map, mut points, mut limit, mut slots, mut seed, mut fleet) = (None, VICTORY_LIMIT.default, FLEET_SIZE.default as usize, vec![], 0, vec![]);
        for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=').ok_or_else(|| format!("lobby spec part {part:?} is not key=value"))?;
            let number = |what: &str| value.trim().parse::<u32>().map_err(|_| format!("lobby spec {what} {value:?} is not a number"));
            match key.trim() {
                "map" => map = Some(value.trim().to_string()),
                "points" => points = number("points")?,
                "limit" => limit = number("limit")? as usize,
                "seed" => seed = u64::from(number("seed")?),
                "fleet" => fleet = value.split('+').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect(),
                "slots" => {
                    for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                        let (count, kind) = item
                            .split_once('*')
                            .map_or(Ok((1, item)), |(n, k)| n.trim().parse::<usize>().map(|n| (n, k.trim())).map_err(|_| format!("lobby spec slot count in {item:?}")))?;
                        let kind = match kind {
                            "human" => SlotSpec::Human,
                            "computer" => SlotSpec::Computer,
                            "open" => SlotSpec::Open,
                            "closed" => SlotSpec::Closed,
                            other => return Err(format!("lobby spec slot {other:?}: expected human, computer, open or closed")),
                        };
                        slots.extend(std::iter::repeat_n(kind, count));
                    }
                }
                other => return Err(format!("lobby spec key {other:?}: expected map, points, limit, seed, fleet or slots")),
            }
        }
        let map = map.ok_or("lobby spec needs map=<id>")?;
        if slots.first() != Some(&SlotSpec::Human) {
            return Err("lobby spec slots: the first row is the human".into());
        }
        if !(VICTORY_LIMIT.min..=VICTORY_LIMIT.max).contains(&points) || !(points - VICTORY_LIMIT.min).is_multiple_of(VICTORY_LIMIT.step) {
            return Err(format!("lobby spec points {points}: {}-{} in steps of {}", VICTORY_LIMIT.min, VICTORY_LIMIT.max, VICTORY_LIMIT.step));
        }
        if !(FLEET_SIZE.min as usize..=FLEET_SIZE.max as usize).contains(&limit) {
            return Err(format!("lobby spec limit {limit}: {}-{}", FLEET_SIZE.min, FLEET_SIZE.max));
        }
        Ok(Self { map, points, limit, slots, seed, fleet })
    }

    /// Sets the lobby up like a player would: pick the map, move the sliders, turn rows into computers, Auto Assign the human's
    /// fleet. Computer fleets are built when the battle starts (`Skirmish::plan`), exactly as after the Launch button.
    pub fn apply(&self, sk: &mut Skirmish, lib: &Library) -> Result<(), String> {
        let at = sk.maps.iter().position(|m| m.0 == self.map).ok_or_else(|| format!("lobby scenario: no skirmish map {:?}", self.map))?;
        sk.historical = !sk.maps[at].2;
        sk.selected = at;
        sk.load_map();
        sk.victory_limit = self.points;
        sk.fleet_size = self.limit;
        sk.draw = self.seed;
        if self.slots.len() > sk.slots.len() {
            return Err(format!("lobby scenario: {} rows asked, but {} has {}", self.slots.len(), self.map, sk.slots.len()));
        }
        for (i, want) in self.slots.iter().enumerate() {
            let choice = match want {
                SlotSpec::Human if sk.slots[i].kind == Kind::Human => continue,
                // A second human: the row a computer would be, played by a person.
                SlotSpec::Human => {
                    sk.choose(Cell::Player(i), Choice::Computer);
                    sk.slots[i].kind = Kind::Human;
                    continue;
                }
                SlotSpec::Computer => Choice::Computer,
                SlotSpec::Open => Choice::Open,
                SlotSpec::Closed => Choice::Close,
            };
            sk.choose(Cell::Player(i), choice);
        }
        if sk.open() {
            // The human picks a fleet type when the map leaves it open, then presses Auto Assign.
            if sk.human().is_some_and(|h| h.race == Race::Unchosen) {
                let i = sk.slots.iter().position(|s| s.kind == Kind::Human).unwrap_or(0);
                sk.choose(Cell::Fleet(i), Choice::Fleet(Race::Navy));
            }
            if self.fleet.is_empty() {
                crate::menu::auto_assign(sk, lib);
            } else if let Some(i) = sk.slots.iter().position(|s| s.kind == Kind::Human) {
                for class in &self.fleet {
                    if !lib.classes.iter().any(|c| &c.name == class) {
                        return Err(format!("lobby scenario: no ship class {class:?}"));
                    }
                    sk.slots[i].push_ship(class);
                }
            }
        }
        sk.map = None;
        match sk.launch_error() {
            Some(error) => Err(format!("lobby scenario cannot launch: {}", launch_error_text(error, super::strings()))),
            None => Ok(()),
        }
    }
}

/// A launch error as plain text: a string id (`IDGS_...`) is looked up in the retail string table `strings` (empty or missing, as
/// without an import) and otherwise gets a plain-English fallback; text that is not an id is returned as it is.
pub fn launch_error_text(error: &str, strings: &serde_json::Value) -> String {
    if !error.starts_with("IDGS_") {
        return error.to_string();
    }
    match strings[error].as_str().filter(|t| !t.trim().is_empty()) {
        Some(text) => text.trim().to_string(),
        None => match error {
            "IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE" => "a team or player has no race (pick a fleet type)".into(),
            "IDGS_TPFRONTENDTEXT_SCREENS_ERROR_FLEET" => "the human player has no ships (assemble a fleet)".into(),
            other => format!("launch refused ({other})"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_error_id_becomes_text() {
        let strings = serde_json::json!({"IDGS_X": " Pick a race. "});
        assert_eq!(launch_error_text("IDGS_X", &strings), "Pick a race.");
        let none = serde_json::Value::Null;
        assert!(launch_error_text("IDGS_TPFRONTENDTEXT01_PLEASE_SELECT_A_RACE", &none).contains("has no race"));
        assert_eq!(launch_error_text("IDGS_UNKNOWN", &none), "launch refused (IDGS_UNKNOWN)");
        assert_eq!(launch_error_text("No opponent: open another slot.", &none), "No opponent: open another slot.");
    }

    #[test]
    fn the_owners_game_parses() {
        let s = LobbyScenario::parse("map=border_dispute; points=1000; limit=10; slots=human,7*computer").unwrap();
        assert_eq!((s.map.as_str(), s.points, s.limit), ("border_dispute", 1000, 10));
        assert_eq!(s.slots.len(), 8);
        assert_eq!(s.slots[0], SlotSpec::Human);
        assert!(s.slots[1..].iter().all(|k| *k == SlotSpec::Computer));
        assert_eq!(s.seed, 0);
        assert_eq!(LobbyScenario::parse("map=border_dispute;seed=3;slots=human,computer").unwrap().seed, 3);
        assert!(s.fleet.is_empty());
        let f = LobbyScenario::parse("map=a;fleet=Ship_Navy_Cutter + Ship_Navy_ManOWar;slots=human,computer").unwrap();
        assert_eq!(f.fleet, ["Ship_Navy_Cutter", "Ship_Navy_ManOWar"]);
    }

    #[test]
    fn a_spec_starting_with_a_brace_is_a_whole_setup() {
        assert!(matches!(LobbyLaunch::parse("map=a;slots=human"), Ok(LobbyLaunch::Spec(_))));
        let json = r#" {"map":"a","points":300,"limit":5,"slots":[{"kind":"human","team":0,"flag":0}]}"#;
        assert!(matches!(LobbyLaunch::parse(json), Ok(LobbyLaunch::Setup(s)) if s.map == "a"));
        assert!(LobbyLaunch::parse("{not json}").unwrap_err().contains("lobby setup"));
    }

    #[test]
    fn bad_specs_say_what_is_wrong() {
        for (spec, needle) in [
            ("points=300;slots=human", "needs map"),
            ("map=a;slots=computer", "first row is the human"),
            ("map=a;slots=human,2*pirate", "expected human, computer"),
            ("map=a;points=1005;slots=human", "steps of 10"),
            ("map=a;points=5000;slots=human", "points 5000"),
            ("map=a;limit=11;slots=human", "limit 11"),
            ("map=a;speed=3;slots=human", "key \"speed\""),
            ("map=a;slots=human,x*computer", "slot count"),
            ("map", "not key=value"),
        ] {
            let err = LobbyScenario::parse(spec).unwrap_err();
            assert!(err.contains(needle), "{spec:?}: {err}");
        }
    }

    /// #369 phase 2 E: a later `human` row is a second human player, on its own side (the first human's is the player's), with a
    /// fleet; the setup it leaves is what a command log's header keeps, and it sets the lobby up again the same.
    #[test]
    #[ignore = "needs imported assets"]
    fn a_second_human_row_plays_its_own_side_and_survives_the_log_header() {
        let lib = Library::load(crate::assets_dir());
        let mut sk = crate::menu::tests::skirmish("border_dispute");
        LobbyScenario::parse("map=border_dispute;slots=human,human,computer").unwrap().apply(&mut sk, &lib).unwrap();
        let groups = sk.plan(&lib);
        let humans: Vec<_> = groups.iter().filter(|g| g.human).collect();
        assert_eq!(humans.iter().map(|g| g.slot).collect::<Vec<_>>(), [Some(0), Some(1)]);
        assert_eq!(humans[0].side, crate::Side::Player);
        assert!(humans[1].side != crate::Side::Player && !humans[1].ships.is_empty(), "the second human: its own side and a fleet");
        let setup = crate::menu::SkirmishSetup::capture(&sk, &lib).unwrap();
        let mut again = crate::menu::tests::skirmish("border_dispute");
        setup.apply(&mut again, &lib).unwrap();
        assert_eq!(crate::menu::SkirmishSetup::capture(&again, &lib), Some(setup), "the header's setup sets the lobby up again");
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_seed_changes_the_computers_fleet_draw() {
        let lib = Library::load(crate::assets_dir());
        let fleet = |spec: &str| {
            let mut sk = crate::menu::tests::skirmish("border_dispute");
            LobbyScenario::parse(spec).unwrap().apply(&mut sk, &lib).unwrap();
            sk.plan(&lib).into_iter().filter(|g| !g.human).map(|g| g.ships.iter().map(|s| s.0.clone()).collect::<Vec<_>>()).collect::<Vec<_>>()
        };
        let base = "map=border_dispute;points=1000;limit=10;slots=human,7*computer";
        assert_eq!(fleet(base), fleet(&format!("{base};seed=0")), "seed 0 is the usual draw");
        assert_ne!(fleet(base), fleet(&format!("{base};seed=1")));
    }

    #[test]
    #[ignore = "needs imported assets"]
    fn a_full_lobby_plans_allied_and_hostile_computer_fleets() {
        let lib = Library::load(crate::assets_dir());
        let s = LobbyScenario::parse("map=border_dispute;points=1000;limit=10;slots=human,7*computer").unwrap();
        let mut sk = crate::menu::tests::skirmish("border_dispute");
        s.apply(&mut sk, &lib).unwrap();
        let plan = sk.plan(&lib);
        assert_eq!(plan.len(), 8, "every row plays");
        assert_eq!(plan.iter().filter(|g| g.human).count(), 1);
        let table = crate::scenario::battle_alliances(&plan, &[], None);
        let allies = plan.iter().filter(|g| !g.human && table.with_human(g.side)).count();
        let foes = plan.iter().filter(|g| !table.with_human(g.side)).count();
        assert!(allies >= 1 && foes >= 1, "Border Dispute is teams: {allies} allied computers, {foes} hostile");
        for g in plan.iter().filter(|g| !g.human) {
            assert!(!g.ships.is_empty(), "computer row {} has a fleet", g.player);
        }
    }
}
