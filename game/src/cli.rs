//! The command-line flags that print something and exit, before any window, asset or mod work: `--version`, `--help` / `-h`,
//! `--dump-schemas <dir>` and `--check-mod <dir> [--json]`. (The mission-authoring flags are in `rules/cli.rs`.)

use std::io::Write;
use std::path::Path;

/// What the game knows about one command-line flag besides its help text (`HELP`, which a test keeps in step with this table).
#[derive(Debug, Clone, Copy)]
pub struct Flag {
    pub name: &'static str,
    /// The next argument is the flag's value.
    pub value: bool,
    /// The flag starts a battle, a menu screen or a screenshot on its own, so no player is there to answer the first-run import screen.
    pub scripted: bool,
    /// Kept in a command recording's scenario arguments (`commands::scenario_args`); the others are the run's presentation, timing,
    /// scripts, paths and tools, which a recording leaves out.
    pub recorded: bool,
}
const fn flag(name: &'static str, value: bool, scripted: bool, recorded: bool) -> Flag {
    Flag { name, value, scripted, recorded }
}
/// The flags the game knows by kind. Flags that only need their text in `HELP` and are read where they are used are not listed.
pub const FLAGS: [Flag; 28] = [
    flag("--script", true, true, false),
    flag("--replay", true, false, false),
    flag("--as-player", true, false, false),
    flag("--speed", true, false, false),
    flag("--shot", true, true, false),
    flag("--shot-frame", true, false, false),
    flag("--late-frame", true, false, false),
    flag("--dist", true, false, false),
    flag("--yaw", true, false, false),
    flag("--pitch", true, false, false),
    flag("--cam", true, false, false),
    flag("--look", true, false, false),
    flag("--mod", true, false, false),
    flag("--rule-report", true, false, false),
    flag("--game", true, false, false),
    flag("--out", true, false, false),
    flag("--assets", true, false, false),
    flag("--crashed", true, false, false),
    flag("--setup-shot", true, false, false),
    flag("--check-mod", true, false, false),
    flag("--fixed-step", false, false, false),
    flag("--autolaunch", false, false, false),
    flag("--console", false, false, false),
    flag("--starmap", false, false, false),
    flag("--menu", true, true, true),
    flag("--map", true, true, true),
    flag("--mission", true, true, true),
    flag("--seed", true, false, true),
];

/// The table entry of an argument, if it is one of the listed flags.
pub fn flag_named(arg: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.name == arg)
}

/// The environment variables the game reads, with what they do (a test checks each is still read somewhere in the sources).
/// Reads stay where they are used; this is the one list.
pub const ENV_VARS: [(&str, &str); 15] = [
    ("PROCYON_ASSETS", "folder of the imported assets (default: the checkout's assets/, else the per-user folder)"),
    ("PROCYON_OVERLAY", "where this launch builds its overlay of assets, content and mods"),
    ("PROCYON_FIXED_STEP", "1 or 0: one 33 ms tick per frame (also --fixed-step); 0 switches it off even for --shot"),
    ("PROCYON_AUTOPLAY", "<game seconds>: an unattended mission run (tools/autoplay.py); also lets the speed cap grow with the speed"),
    ("PROCYON_PROFILE", "frame-time logging, and vsync off"),
    ("PROCYON_SEED", "the battle's random seed when --seed is not given"),
    ("PROCYON_COMMANDS", "<file>: record the battle's commands there (replay with --replay)"),
    ("PROCYON_CONSOLE", "open the developer console in a release build (also --console)"),
    ("PROCYON_RULE_TRACE", "1: log why rules have not fired (also --rule-trace)"),
    ("PROCYON_TRACE", "<file.csv>: log every ship and shell once per simulation step"),
    ("PROCYON_REPORT", "<game seconds>: watch a skirmish and log one summary line when a side is gone or the time is up"),
    ("PROCYON_END_SHOT", "<png>: a screenshot 3 s after the mission ends, then quit"),
    ("PROCYON_NIS_SHOT", "<png>: a screenshot 4 s into a cutscene, then quit"),
    ("PROCYON_AUTOINVULN", "autopilot: the player's fleet cannot be sunk or boarded"),
    ("PROCYON_AUTOKILL", "autopilot: sink the pirate once the player is close"),
];

/// The process's arguments, read once. The readers of single flags below use it instead of scanning `std::env::args()` each.
pub fn args() -> &'static [String] {
    static ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    ARGS.get_or_init(|| std::env::args().collect())
}
/// The argument after the first `flag`, if any (the lookup `--seed N`, `--look x,y`, `--lobby spec` use).
pub fn value_of(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}
/// `value_of` over the process's arguments.
pub fn value(flag: &str) -> Option<String> {
    value_of(args(), flag)
}
/// Whether the process was given `flag`.
pub fn has(flag: &str) -> bool {
    args().iter().any(|a| a == flag)
}

/// What `--help` / `-h` print: every flag the game reads, grouped by use (`the_help_names_every_flag_the_game_reads` fails when a flag
/// in the source is missing here; the importer's own helper flags are `INTERNAL`). The developer flags are explained in docs/development.md.
const HELP: &str = "\
OpenProcyon: an open-source engine for Treasure Planet: Battle at Procyon (it needs your own imported assets).

Usage: openprocyon [FLAGS]

Play:
  --map <id>                  start a skirmish battle on the map <id>
  --mission <n>               start campaign mission <n>
  --menu <name>               open the front end at the screen <name> (skirmish, ...)
  --mod <folder>              load a mod folder over the assets (repeatable, later ones win)
  --only-mod                  with --mod: load only the --mod folders, not the installed mods
  --lobby <spec>              set the skirmish lobby up from <spec> (map=<id>;points=N;limit=N;seed=N;slots=human,2*computer)
                                 and launch; leaving the battle (the summary's continue arrow, F10) then exits the process instead of
                                 opening the main menu, so a tool that started the game sees it end

Mod authoring (no window, no assets needed unless said):
  --check-mod <folder> [--json] [--verbose]  check a mod folder (the imported assets, when found, are used); exit 1 on errors;
                                 warnings the base game's own files have too are counted apart (--verbose lists them)
  --check-mission <map id|all>   check the mission rules of a map (needs assets; takes --mod)
  --dump-schemas <folder>        write the JSON Schemas of ship, guns, bullets, crew nodes, mod, map, mission, script and media files
  --dump-rule-schema [file]      write the mission rule schema (stdout without a file)
  --rule-trace, --why <rule>     while a mission plays, log why rules have not fired
  --rule-report <file>           when the battle ends, write what each rule of the mission did to <file> (JSON)
  --console                      open the developer console (~) in a release build

Developer and smoke-test flags (docs/development.md):
  --shot <file.png>           play the battle and save a screenshot (1280x720), then exit; implies --fixed-step
  --shot-frame <n[,m,...]>    the frame(s) to picture (default 500); --late-frame <n> adds a _late picture
  --script <name>             run a scripted smoke scenario (fxtest, victory, defeat, restart, mission, console, ...)
  --ship <class>              the human's ship of a --map battle without a lobby (a class name such as Ship_Navy_Frigate)
  --allies <a,b,...>          allied computer ships of that battle; --enemies <a,b,...|none> the enemy ships
  --speed <x>                 game speed multiplier (default 1)
  --seed <n>                  replay a battle's random seed (also PROCYON_SEED); --fixed-step: one 33 ms tick per frame (PROCYON_FIXED_STEP=1)
  --replay <file.jsonl>       play back a command log recorded with PROCYON_COMMANDS=<file> (same scenario arguments, seed, mods)
  --as-player <n>             play lobby slot n (a human row) on this machine; with --replay, n's orders come live, the others' from the log
  --cam <yaw,pitch,dist>      camera orbit (also --yaw, --pitch, --dist); --look <x,y[,z]> aims it at a point (game axes)
  --starmap                   open the star map on the first frame
  --autolaunch                with --menu lobby: fill the human fleet and launch
  --fire-rule <name>          start a mission at a later beat: run that rule's actions after the world-initialise rules (repeatable)
  --start-at <x,y>            with --fire-rule on a campaign or custom mission: move the player's ship to game position x,y
  --json, --verbose           with --check-mod: machine-readable output; list the warnings of the base game's own files too

Internal (the first-run import screen and the tools the game starts; not for typing):
  --import, --game <dir>, --out <dir>, --output, --assets <dir>, --crashed <log>, --setup-shot <file.png>, --bake-skies,
  --watch-stdin, --split-stars, --clipboard, --no-newline

Other:
  --version                   print the version and exit
  -h, --help                  print this help and exit

Environment: PROCYON_ASSETS is the folder of the imported assets (default: the per-user folder). Docs: docs/modding.md.
";

/// Runs `--help`, `--version` or `--dump-schemas <dir>` when given and returns the exit status; `None` when neither is.
pub fn quick(args: &[String]) -> Option<i32> {
    if args.iter().skip(1).any(|a| a == "--version") {
        println!("OpenProcyon {}", crate::authoring::ENGINE_VERSION);
        return Some(0);
    }
    if args.iter().skip(1).any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Some(0);
    }
    if let Some(i) = args.iter().position(|a| a == "--check-mod") {
        return Some(check_mod(args.get(i + 1).filter(|a| !a.starts_with("--")), args.iter().any(|a| a == "--json")));
    }
    let i = args.iter().position(|a| a == "--dump-schemas")?;
    let Some(dir) = args.get(i + 1).filter(|a| !a.starts_with("--")) else {
        eprintln!("--dump-schemas takes a folder to write the schema files into");
        return Some(2);
    };
    match write_schemas(Path::new(dir)) {
        Ok(names) => {
            eprintln!("wrote {} to {dir}", names.join(", "));
            Some(0)
        }
        Err(e) => {
            eprintln!("cannot write the schemas to {dir}: {e}");
            Some(2)
        }
    }
}

/// `--check-mod <dir> [--json]`: prints the findings of `authoring::check_mod` and returns 1 when there are errors, else 0 (2: no folder
/// given). Plain output is one line per finding, `<file>: <level>: <path>: <message>`, then `<mod>: N errors, M warnings`; `--json` prints one
/// object `{"mod", "engine", "errors", "warnings", "findings": [...]}` (a finding's fields are those of `authoring::Finding`).
fn check_mod(dir: Option<&String>, json: bool) -> i32 {
    let Some(dir) = dir else {
        eprintln!("--check-mod takes the mod's folder");
        return 2;
    };
    let base = crate::authoring::base_assets();
    let findings = crate::authoring::check_mod(Path::new(dir), base.as_deref());
    let errors = findings.iter().filter(|f| f.level == crate::authoring::Level::Error).count();
    // Warnings the base game's own files have too are not the mod's: they do not count and are listed only with --verbose.
    let inherited = findings.iter().filter(|f| f.inherited).count();
    let warnings = findings.len() - errors - inherited;
    let verbose = has("--verbose");
    // A closed pipe (`| head`) is not an error.
    let out = &mut std::io::stdout();
    if json {
        let doc = serde_json::json!({"mod": dir, "engine": crate::authoring::ENGINE_VERSION, "errors": errors, "warnings": warnings, "inherited": inherited, "findings": findings});
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&doc).expect("findings are JSON"));
    } else {
        for f in findings.iter().filter(|f| !f.inherited) {
            let _ = if f.file.is_empty() { writeln!(out, "{f}") } else { writeln!(out, "{}: {f}", f.file) };
        }
        if inherited > 0 {
            let _ = writeln!(out, "{inherited} warnings come from the base game's own files (not this mod){}", if verbose { ":" } else { "; --verbose to list" });
            for f in findings.iter().filter(|f| f.inherited && verbose) {
                let _ = writeln!(out, "  {}: {f}", f.file);
            }
        }
        let name = Path::new(dir).file_name().map_or_else(|| dir.clone(), |n| n.to_string_lossy().into_owned());
        let _ = writeln!(out, "{name}: {errors} errors, {warnings} warnings");
    }
    i32::from(errors > 0)
}

/// Writes every schema of `authoring::schemas` as `<dir>/<name>` (the folder is created) and returns the file names.
pub fn write_schemas(dir: &Path) -> std::io::Result<Vec<&'static str>> {
    std::fs::create_dir_all(dir)?;
    let mut names = vec![];
    for (name, schema) in crate::authoring::schemas() {
        let text = serde_json::to_string_pretty(&schema).expect("schema is JSON");
        std::fs::write(dir.join(name), text + "\n")?;
        names.push(name);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn version_and_schema_flags_exit_before_anything_else() {
        assert_eq!(quick(&args(&["game", "--version"])), Some(0));
        assert_eq!(quick(&args(&["game", "--help"])), Some(0));
        assert_eq!(quick(&args(&["game", "-h"])), Some(0));
        assert_eq!(quick(&args(&["game", "--map", "x"])), None, "other runs go on");
        assert_eq!(quick(&args(&["game", "--dump-schemas"])), Some(2), "a folder is needed");
        assert_eq!(quick(&args(&["game", "--dump-schemas", "--mod"])), Some(2));
    }

    #[test]
    fn the_help_names_every_flag_it_lists_and_stays_in_the_flags_the_game_reads() {
        for flag in [
            "--map",
            "--mission",
            "--menu",
            "--mod",
            "--only-mod",
            "--check-mod",
            "--check-mission",
            "--dump-schemas",
            "--dump-rule-schema",
            "--rule-trace",
            "--why",
            "--rule-report",
            "--console",
            "--version",
            "--help",
        ] {
            assert!(HELP.contains(flag), "{flag}");
        }
    }

    /// Flags the game reads for its own helper processes and the first-run screen; `HELP` lists them as a group.
    const INTERNAL: [&str; 12] =
        ["--import", "--game", "--out", "--output", "--assets", "--crashed", "--setup-shot", "--bake-skies", "--watch-stdin", "--split-stars", "--clipboard", "--no-newline"];

    /// Every quoted flag literal in the game's sources must be in the help, so a new flag cannot be forgotten.
    #[test]
    fn the_help_names_every_flag_the_game_reads() {
        fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = vec![];
        sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(files.len() > 50, "found the sources");
        let mut missing = std::collections::BTreeSet::new();
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for part in text.split("\"--").skip(1) {
                let name: String = part.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-').collect();
                let flag = format!("--{name}");
                if !name.is_empty() && part[name.len()..].starts_with('"') && !HELP.contains(&flag) && !INTERNAL.contains(&flag.as_str()) {
                    missing.insert(format!("{flag} ({})", file.file_name().unwrap().to_string_lossy()));
                }
            }
        }
        assert!(missing.is_empty(), "flags read by the game but not in --help: {missing:?}");
    }

    #[test]
    fn the_flag_table_agrees_with_the_help_and_the_scripted_flags() {
        for f in FLAGS {
            assert!(HELP.contains(f.name) || INTERNAL.contains(&f.name), "{} is in the table but neither in --help nor internal", f.name);
        }
        let scripted: Vec<&str> = FLAGS.iter().filter(|f| f.scripted).map(|f| f.name).collect();
        assert_eq!(scripted, ["--script", "--shot", "--menu", "--map", "--mission"]);
        assert_eq!(flag_named("--seed").map(|f| (f.value, f.recorded)), Some((true, true)));
        assert!(flag_named("nonsense").is_none());
    }

    #[test]
    fn every_listed_environment_variable_is_read_somewhere() {
        fn sources(dir: &Path, out: &mut String) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "cli.rs") {
                    out.push_str(&std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let mut all = String::new();
        sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut all);
        for (name, _) in ENV_VARS {
            assert!(all.contains(&format!("\"{name}\"")), "{name} is listed but no longer read");
        }
    }

    #[test]
    fn value_of_reads_the_argument_after_the_first_flag() {
        let a = args(&["game", "--seed", "7", "--seed", "9"]);
        assert_eq!(value_of(&a, "--seed").as_deref(), Some("7"));
        assert_eq!(value_of(&a, "--map"), None);
        assert_eq!(value_of(&args(&["game", "--seed"]), "--seed"), None, "a flag last has no value");
    }

    #[test]
    fn dump_schemas_writes_one_file_per_type() {
        let dir = std::env::temp_dir().join(format!("procyon-schemas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("nested");
        assert_eq!(quick(&args(&["game", "--dump-schemas", &out.to_string_lossy()])), Some(0));
        let mut files: Vec<String> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        files.sort();
        assert_eq!(
            files,
            [
                "bullets.schema.json",
                "crew_nodes.schema.json",
                "guns.schema.json",
                "map.schema.json",
                "media.schema.json",
                "mission.schema.json",
                "mod.schema.json",
                "script.schema.json",
                "ship.schema.json"
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
