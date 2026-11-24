//! What OpenProcyon adds to the original: [`extensions`] lists the table "Engine extensions" of `docs/modding.md`.
use serde::Serialize;

/// What an extension can change (the table's *Changes* column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionKind {
    /// It can change what happens in a battle (who hits, moves, wins).
    Simulation,
    /// It changes only what is drawn or heard.
    Presentation,
    /// It adds a way for the player to act (keys, menus, the clock) without new rules.
    Control,
}

/// One row of the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Extension {
    /// What it is, in the table's words.
    pub feature: String,
    /// What switches it on: a file in the overlay, a key in a data file, or "always".
    pub gate: String,
    pub kind: ExtensionKind,
    /// The code that owns it (`game/src/...` paths; a directory ends in `/`).
    pub code: Vec<String>,
}

/// `docs/modding.md`, compiled in: the table is the one list of extensions, which `tests/test_extensions.py` keeps equal to the
/// `Not retail:` markers of the code. A comment is not available at run time, so the table is what a tool reads.
const MODDING: &str = include_str!("../../../docs/modding.md");

/// The engine's extensions to the original, one per row of "Engine extensions" in `docs/modding.md` (the source the
/// documentation test checks against the code's `Not retail:` markers).
pub fn extensions() -> Vec<Extension> {
    parse(MODDING)
}

fn parse(text: &str) -> Vec<Extension> {
    let Some(start) = text.find("\n## Engine extensions (not retail)\n") else { return vec![] };
    let section = &text[start + 1..];
    let section = section[3..].find("\n## ").map_or(section, |end| &section[..end + 3]);
    section
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Extension"))
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim_start_matches("| ").trim_end_matches(" |").split(" | ").collect();
            let [feature, gate, changes, code] = cells[..] else { return None };
            let kind = match changes.split_whitespace().next()? {
                "simulation" => ExtensionKind::Simulation,
                "presentation" => ExtensionKind::Presentation,
                "control" => ExtensionKind::Control,
                _ => return None,
            };
            let code = code.split('`').skip(1).step_by(2).filter(|p| p.starts_with("game/src/")).map(String::from).collect();
            Some(Extension { feature: feature.to_string(), gate: gate.to_string(), kind, code })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_of_the_table_is_an_extension_with_a_kind_and_code() {
        let rows = MODDING
            .lines()
            .skip_while(|l| !l.starts_with("## Engine extensions (not retail)"))
            .skip(1)
            .take_while(|l| !l.starts_with("## "))
            .filter(|l| l.starts_with("| ") && !l.starts_with("| Extension"))
            .count();
        let all = extensions();
        assert!(rows > 10, "{rows}");
        assert_eq!(all.len(), rows, "a row did not parse");
        assert!(all.iter().all(|e| !e.feature.is_empty() && !e.gate.is_empty() && !e.code.is_empty()), "{all:?}");
        let beam = all.iter().find(|e| e.feature.starts_with("Beam guns")).unwrap();
        assert_eq!(beam.kind, ExtensionKind::Simulation);
        assert!(beam.gate.contains("beam") && beam.code.contains(&"game/src/combat/beam.rs".to_string()));
        assert!(all.iter().any(|e| e.kind == ExtensionKind::Control) && all.iter().any(|e| e.kind == ExtensionKind::Presentation));
    }

    #[test]
    fn a_made_up_table_parses_the_same_way() {
        let text = "\n## Engine extensions (not retail)\n\n| Extension | Gate | Changes | Code |\n|---|---|---|---|\n| A thing | a key | presentation | `game/src/a.rs`, `game/src/b/` |\n| Bad | x | whatever | `game/src/c.rs` |\n\n## Next\n| Z | y | control | `game/src/z.rs` |\n";
        let all = parse(text);
        assert_eq!(
            all,
            [Extension { feature: "A thing".into(), gate: "a key".into(), kind: ExtensionKind::Presentation, code: vec!["game/src/a.rs".into(), "game/src/b/".into()] }]
        );
    }
}
