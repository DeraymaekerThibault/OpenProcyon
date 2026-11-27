use super::*;

#[test]
fn steam_library_folders_both_formats() {
    let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"360059062"
		}
	}
	"1"
	{
		"path"		"D:\\Games"
	}
}"#;
    assert_eq!(library_folders(vdf), [PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\Games")]);
    let old = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1234\"\n\t\"1\"\t\t\"/home/me/games/Steam\"\n}";
    assert_eq!(library_folders(old), [PathBuf::from("/home/me/games/Steam")]);
    assert!(library_folders("").is_empty());
    assert_eq!(quoted(r#"  "a\"b"  "c""#), ["a\"b", "c"]);
}

#[test]
fn steam_path_from_the_registry_query() {
    let out = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
    assert_eq!(reg_value(out), Some(PathBuf::from("c:/program files (x86)/steam")));
    assert_eq!(reg_value("ERROR: The system was unable to find the specified registry key or value."), None);
}

#[test]
fn disc_installs_from_installed_program_entries() {
    let out = concat!(
        "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Other\r\n",
        "    DisplayName    REG_SZ    Some Other Game\r\n",
        "    InstallLocation    REG_SZ    C:\\Games\\Other\r\n",
        "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{0A1B}\r\n",
        "    DisplayName    REG_SZ    Treasure Planet: Battle at Procyon\r\n",
        "    InstallLocation    REG_SZ    \r\n",
        "    UninstallString    REG_SZ    C:\\WINDOWS\\IsUninst.prog -f\"C:\\Program Files\\Disney\\TP\\Uninst.isu\"\r\n",
        "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\TP\r\n",
        "    DisplayName    REG_SZ    TREASURE PLANET\r\n",
        "    InstallLocation    REG_SZ    \"E:\\Old Games\\Procyon\"\r\n",
        "    DisplayIcon    REG_SZ    E:\\Old Games\\Procyon\\Procyon.ico,0\r\n",
    );
    let dirs = uninstall_dirs(out);
    for want in [r"C:\Program Files\Disney\TP", r"E:\Old Games\Procyon"] {
        assert!(dirs.contains(&PathBuf::from(want)), "{want} in {dirs:?}");
    }
    assert!(!dirs.contains(&PathBuf::from(r"C:\Games\Other")));
    assert_eq!(reg_line("    DisplayName    REG_SZ    A  B"), Some(("DisplayName", "A  B")));
    assert_eq!(reg_line("HKEY_CURRENT_USER\\Software"), None);
}

#[test]
fn disc_installs_by_folder_name() {
    let dir = std::env::temp_dir().join(format!("procyon-scan-test-{}", std::process::id()));
    let direct = dir.join("Program Files (x86)").join("Treasure Planet Battle at Procyon");
    let nested = dir.join("Program Files").join("Disney Interactive").join("TREASURE PLANET").join("Procyon");
    let too_deep = dir.join("Program Files").join("a").join("b").join("Treasure Planet");
    for d in [&direct, &nested, &too_deep] {
        std::fs::create_dir_all(d).unwrap();
    }
    let found = scan(&[dir.join("Program Files (x86)"), dir.join("Program Files"), dir.join("missing")]);
    assert!(found.contains(&direct) && found.contains(&nested), "{found:?}");
    assert!(!found.contains(&too_deep));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn install_validation() {
    let dir = std::env::temp_dir().join(format!("procyon-setup-test-{}", std::process::id()));
    let game = dir.join("steamapps").join("common").join(STEAM_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(check_install("").is_err());
    assert!(check_install(&dir.join("missing").display().to_string()).unwrap_err().contains("no folder"));
    assert!(check_install(&dir.display().to_string()).unwrap_err().contains("no WorldObjects"));
    std::fs::create_dir_all(game.join("WorldObjects")).unwrap();
    std::fs::write(game.join("Procyon.exe"), b"").unwrap();
    for input in [game.display().to_string(), format!("  \"{}\" ", game.display()), game.join("Procyon.exe").display().to_string(), dir.display().to_string()] {
        assert_eq!(check_install(&input), Ok(game.clone()), "{input}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn progress_lines() {
    assert_eq!(parse_progress("[3/27] hud"), Some((3, 27, "hud", None)));
    assert_eq!(parse_progress("[1/28] ships: 36/71 (36%)\r"), Some((1, 28, "ships: 36/71", Some(36))));
    assert_eq!(parse_progress("[2/28] models (74%)"), Some((2, 28, "models", Some(74))));
    assert_eq!(parse_progress("[2/28] odd (name%)"), Some((2, 28, "odd (name%)", None)));
    assert_eq!(parse_progress("[12/27] waypoint art\r"), Some((12, 27, "waypoint art", None)));
    assert_eq!(parse_progress("skip Ship_Wreck: no mesh"), None);
    assert_eq!(parse_progress("[0/27] hud"), None);
    assert_eq!(parse_progress("[x/27] hud"), None);
    assert_eq!(failed_step("import failed at step 'models': tools.mdb_to_glb"), Some("models".into()));
    assert_eq!(failed_step("done: C:/assets"), None);
}

#[test]
fn the_game_gets_the_command_line_without_setup_flags() {
    let args = ["--game", "D:/x", "--import", "--setup-shot", "a.png", "--menu", "--crashed", "c.log", "--shot", "b.png"].map(String::from);
    assert_eq!(game_args(args.into_iter()), ["--game", "D:/x", "--menu", "--shot", "b.png"]);
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("procyon-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_players_import_must_come_from_this_version() {
    let dir = scratch("stamp");
    let assets = dir.join("assets");
    assert_eq!(imported(&assets, "0.1.4"), Imported::Missing);
    std::fs::create_dir_all(assets.join("ships")).unwrap();
    assert_eq!(imported(&assets, "0.1.4"), Imported::Outdated { version: None, game: None }, "imports from before the stamp");
    write_stamp(&assets, "0.1.3", Path::new("D:/Games/TP")).unwrap();
    assert_eq!(imported(&assets, "0.1.4"), Imported::Outdated { version: Some("0.1.3".into()), game: Some(PathBuf::from("D:/Games/TP")) });
    write_stamp(&assets, "0.1.4", Path::new("D:/Games/TP")).unwrap();
    assert_eq!(imported(&assets, "0.1.4"), Imported::Ready);
    std::fs::write(assets.join(STAMP), "not json").unwrap();
    assert!(matches!(imported(&assets, "0.1.4"), Imported::Outdated { version: None, .. }));
    assert!(outdated_reason(Some("0.1.3")).contains("OpenProcyon 0.1.3"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// #522: the terminal importer (`tools/import_assets.py`) writes the stamp itself, in this exact text, and the game must take it as current.
#[test]
fn a_stamp_written_by_the_python_importer_is_current() {
    let dir = scratch("py_stamp");
    std::fs::create_dir_all(dir.join("ships")).unwrap();
    std::fs::write(dir.join(STAMP), "{\n  \"version\": \"0.2.0\",\n  \"game\": \"/games/tp\"\n}\n").unwrap();
    assert_eq!(imported(&dir, "0.2.0"), Imported::Ready);
    assert!(matches!(imported(&dir, "0.2.1"), Imported::Outdated { version: Some(v), .. } if v == "0.2.0"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_finished_import_replaces_the_old_folder_whole() {
    let dir = scratch("swap");
    let (out, staged) = (dir.join("assets"), dir.join("assets.importing"));
    assert_eq!(staging(&out), staged);
    std::fs::create_dir_all(out.join("ships")).unwrap();
    std::fs::write(out.join("ships/leftover.json"), "old").unwrap();
    std::fs::create_dir_all(staged.join("ships")).unwrap();
    std::fs::write(staged.join("ships/new.json"), "new").unwrap();
    swap_in(&staged, &out).unwrap();
    assert!(out.join("ships/new.json").is_file() && !out.join("ships/leftover.json").exists(), "no files left from the old import");
    assert!(!staged.exists() && !dir.join("assets.replaced").exists());
    // First import: nothing to replace.
    std::fs::remove_dir_all(&out).unwrap();
    std::fs::create_dir_all(staged.join("ships")).unwrap();
    swap_in(&staged, &out).unwrap();
    assert!(out.join("ships").is_dir());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_staged_import_stamps_and_swaps_only_when_it_succeeds() {
    let dir = scratch("staged");
    let out = dir.join("assets");
    std::fs::create_dir_all(out.join("ships")).unwrap();
    let mut setup = Setup::new(out.clone(), Some(String::new()));
    setup.staged = true;
    setup.from = Some(PathBuf::from("D:/Games/TP"));
    // The importer failed: the old import stays in use.
    std::fs::create_dir_all(staging(&out).join("ships")).unwrap();
    setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
    setup.tx.send(Msg::Tool(ToolMsg::Exit(Err("exit 1".into())))).unwrap();
    setup.poll();
    assert!(matches!(setup.phase, Phase::Failed { .. }));
    assert!(out.join("ships").is_dir() && !out.join(STAMP).exists());
    // It succeeded: the new import, stamped with this version, replaces the old one.
    setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
    setup.tx.send(Msg::Tool(ToolMsg::Exit(Ok(())))).unwrap();
    setup.poll();
    assert_eq!(setup.phase, Phase::Starting);
    assert_eq!(imported(&out, env!("CARGO_PKG_VERSION")), Imported::Ready);
    assert!(!staging(&out).exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_crash_screen_shows_the_reports_message() {
    let dir = scratch("crash");
    let log = dir.join("crash.log");
    std::fs::write(&log, "OpenProcyon 0.1.4 crashed.\nmessage: missing field `point`\nat: src/data/ships.rs:157\n").unwrap();
    assert_eq!(crash_message(&log), "missing field `point`");
    assert_eq!(crash_message(&dir.join("missing.log")), "unknown error");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn importer_output_drives_the_phase() {
    let dir = std::env::temp_dir().join(format!("procyon-setup-poll-{}", std::process::id()));
    let mut setup = Setup::new(dir.clone(), Some(String::new()));
    setup.phase = Phase::Importing { step: 0, total: 0, name: String::new(), percent: None };
    for line in ["[1/3] ships (0%)", "skip Ship_Wreck", "[2/3] models (40%)", "import failed at step 'models': tools.mdb_to_glb"] {
        setup.tx.send(Msg::Tool(ToolMsg::Line(line.into()))).unwrap();
    }
    assert!(setup.poll());
    assert_eq!(setup.phase, Phase::Importing { step: 2, total: 3, name: "models".into(), percent: Some(40) });
    setup.tx.send(Msg::Tool(ToolMsg::Exit(Err("exit 1".into())))).unwrap();
    setup.poll();
    assert_eq!(setup.phase, Phase::Failed { step: Some("models".into()), reason: "exit 1".into() });
    assert_eq!(setup.log.len(), 4);
    // A clean exit without ships/ is still a failure; with it the game starts.
    setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
    setup.tx.send(Msg::Tool(ToolMsg::Exit(Ok(())))).unwrap();
    setup.poll();
    assert!(matches!(setup.phase, Phase::Failed { .. }));
    std::fs::create_dir_all(dir.join("ships")).unwrap();
    setup.phase = Phase::Importing { step: 3, total: 3, name: "bases".into(), percent: None };
    setup.tx.send(Msg::Tool(ToolMsg::Exit(Ok(())))).unwrap();
    setup.poll();
    assert_eq!(setup.phase, Phase::Starting);
    assert!(!setup.poll(), "nothing new");
    std::fs::remove_dir_all(dir).unwrap();
}
