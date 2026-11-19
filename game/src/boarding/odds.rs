//! Crew strengths, skill levels and the casualty odds of a boarding fight.
use super::*;

/// Casualty interval: (9 + 1.5·r) / (2·weaker strength) seconds.
const TICK_BASE: f32 = 9.0;
const TICK_SPREAD: f32 = 1.5;
/// WOT 'Crew Skill Level'. The per-man combat ability is the generic crew's (Crew_General_*: 1, 3, 4 of 10).
/// PROVISIONAL: 'Players' (the campaign roster) uses Average, and generic crews get no leadership bonus (named crews do).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Skill {
    Green,
    Average,
    Elite,
    Players,
}
impl Skill {
    pub fn parse(s: &str) -> Self {
        match s {
            "Green" => Self::Green,
            "Elite" => Self::Elite,
            "Players" => Self::Players,
            _ => Self::Average,
        }
    }
    /// Engineer/helmsman factor (`Ship::crew_speed`, `crew_turn`) of a generic crew: retail saves show such ships with crew
    /// factors 1.15 / 1.30-1.37 / 1.40-1.47, and speed follows the saved factor / 1.125 (docs/re/crew.md); 'Players' ships take
    /// theirs from their posts instead. PROVISIONAL: the mean of the saved values (1.163 / 1.326 / 1.441).
    pub fn handling(self) -> f32 {
        match self {
            Self::Green => 1.163 / 1.125,
            Self::Average => 1.326 / 1.125,
            Self::Elite => 1.441 / 1.125,
            Self::Players => 1.0,
        }
    }
    /// Crew factor on sight range (`environment::ScanFactor`): retail saves' values for generic crews (Green 1.15, Average
    /// 1.30-1.37, Elite 1.40-1.47); 'Players' ships keep their plain sight. PROVISIONAL: the lower value of each band.
    pub fn scan(self) -> f32 {
        match self {
            Self::Green => 1.15,
            Self::Average => 1.30,
            Self::Elite => 1.40,
            Self::Players => 1.0,
        }
    }
    /// Gunnery accuracy factor relative to an Average crew (1 - 0.8 x gunnery, as for crewed ships in gunnery.rs); below 1 is better.
    /// Gunnery is the generic crewman's (`Crew_General_*`: 3 / 6 / 8 out of 10). PROVISIONAL: that island gunners use these crews.
    #[cfg(test)]
    pub fn gunnery_vs_average(self) -> f32 {
        let gunnery = |s| match s {
            Self::Green => 0.3,
            Self::Average | Self::Players => 0.6,
            Self::Elite => 0.8,
        };
        (1.0 - 0.8 * gunnery(self)) / (1.0 - 0.8 * gunnery(Self::Average))
    }
    pub(super) fn member(self) -> f32 {
        match self {
            Self::Green => 0.1,
            Self::Average | Self::Players => 0.3,
            Self::Elite => 0.4,
        }
    }
}

/// One object's boarding numbers, named as in the original: `fighters` (Fighter Combat Strength) and `crew` (Crew Combat Strength) are the
/// casualty pools; `attack` (Cached Attacking = crew/2 + fighters) is what it boards with, `defend` (Cached Defending = crew + fighters)
/// what it holds with; `full` is the Initial Defending. A base has only `defend` (its 'Combat Strength').
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Strength {
    pub fighters: i32,
    pub crew: i32,
    pub attack: i32,
    pub defend: i32,
    pub full: i32,
    pub(super) base: bool,
}
impl Strength {
    /// `counts`: Gunners, Captains, First Mates, Navigators, Engineers, Riggers, Fighters, Lookouts; `member`: combat ability 0..1.
    pub fn ship(counts: [u32; 8], member: f32) -> Self {
        let sum = |n: u32| (10.0 * member * n as f32).round() as i32;
        let (fighters, crew) = (sum(counts[6]), sum(counts.iter().sum::<u32>() - counts[6]));
        Self { fighters, crew, attack: crew / 2 + fighters, defend: crew + fighters, full: crew + fighters, base: false }
    }
    pub fn base(combat: i32) -> Self {
        Self { fighters: 0, crew: 0, attack: 0, defend: combat, full: combat, base: true }
    }
    /// After a fight: casualties come out of the fighters first, then the crew; the totals are derived again.
    pub(super) fn normalise(&mut self) {
        if self.base {
            return;
        }
        let loss = self.fighters + self.crew - self.defend;
        if self.fighters < loss {
            self.crew -= loss - self.fighters;
            self.fighters = 0;
        } else {
            self.fighters -= loss;
        }
        self.attack = self.crew / 2 + self.fighters;
        self.defend = self.crew + self.fighters;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outcome {
    Won,
    Lost,
    Draw,
}
/// One casualty: the boarder's attack, then its defend, each lose one; or the target's defend, then its attack.
/// Returns whether the fight is over (the casualty's side is out).
pub fn casualty(boarder: &mut Strength, target: &mut Strength, boarder_hit: bool) -> bool {
    if boarder_hit {
        if boarder.attack > 0 {
            boarder.attack -= 1;
            boarder.defend -= (boarder.defend > 0) as i32;
        }
        boarder.attack < 1
    } else {
        if target.defend > 0 {
            target.defend -= 1;
            target.attack -= (target.attack > 0) as i32;
        }
        target.defend < 1
    }
}
/// Result of a finished fight: the boarder's attack against the target's defend, read before the totals are normalised.
pub fn settle(boarder: &mut Strength, target: &mut Strength) -> Outcome {
    let (a, d) = (boarder.attack, target.defend);
    boarder.normalise();
    target.normalise();
    match d.cmp(&a) {
        std::cmp::Ordering::Less => Outcome::Won,
        std::cmp::Ordering::Greater => Outcome::Lost,
        std::cmp::Ordering::Equal => Outcome::Draw,
    }
}
/// Seconds between casualties, fixed when the fight starts (`r` uniform in 0..1).
pub fn interval(boarder: &Strength, target: &Strength, r: f32) -> f32 {
    let weaker = boarder.attack.min(target.defend);
    if weaker <= 0 { 0.0 } else { (TICK_BASE + TICK_SPREAD * r) / (2.0 * weaker as f32) }
}

static DATA: OnceLock<Value> = OnceLock::new();
fn data() -> &'static Value {
    DATA.get_or_init(|| crate::data::try_json(&assets_dir().join("data/boarding.json")).unwrap_or(Value::Null))
}
/// Crew and boarding points of ships `data/boarding.json` does not list (a mod's ships carry theirs in `ship.json` and the model).
static MODDED: std::sync::Mutex<Vec<(String, [u32; 8], Vec<Vec3>)>> = std::sync::Mutex::new(Vec::new());
/// Offers a class's own crew and boarding points; used only for types the retail table lacks.
pub fn register(entity: &str, crew: [u32; 8], points: &[Vec3]) {
    if data()[entity].is_null() {
        let mut m = MODDED.lock().unwrap();
        m.retain(|(n, ..)| n != entity);
        m.push((entity.into(), crew, points.to_vec()));
    }
}
pub fn counts(entity: &str) -> [u32; 8] {
    let mut out = [0; 8];
    for (o, v) in out.iter_mut().zip(data()[entity]["crew"].as_array().into_iter().flatten()) {
        *o = v.as_u64().unwrap_or(0) as u32;
    }
    if let Some((_, crew, _)) = MODDED.lock().unwrap().iter().find(|(n, ..)| n == entity) {
        out = *crew;
    }
    out
}
/// The object's BoardingEffectPoint nodes in its Bevy frame.
pub(super) fn points(entity: &str) -> Vec<Vec3> {
    let f = |v: &Value, i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    if let Some((_, _, p)) = MODDED.lock().unwrap().iter().find(|(n, ..)| n == entity) {
        return p.clone();
    }
    data()[entity]["points"].as_array().into_iter().flatten().map(|p| conv(f(p, 0), f(p, 1), f(p, 2))).collect()
}
