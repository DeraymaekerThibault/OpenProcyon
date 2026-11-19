//! Finding the game install: Steam libraries, installed-program entries, disc installs and folder checks.
use super::*;

/// The game folder in a typed/pasted/dropped path: the folder with `WorldObjects/`. A dropped file (the game's program) means its
/// folder; a Steam library or its `steamapps/common` finds the game inside.
pub fn check_install(input: &str) -> Result<PathBuf, String> {
    let text = input.trim().trim_matches('"').trim();
    if text.is_empty() {
        return Err("Type or paste the folder you installed the game in, or drop it on this window.".into());
    }
    let mut path = match text.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(text), |h| PathBuf::from(h).join(rest)),
        None => PathBuf::from(text),
    };
    if path.is_file() {
        path.pop();
    }
    if !path.is_dir() {
        return Err(format!("There is no folder {}.", path.display()));
    }
    for candidate in [path.clone(), path.join(STEAM_DIR), path.join("common").join(STEAM_DIR), path.join("steamapps").join("common").join(STEAM_DIR)] {
        if is_install(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!("{} is not the game folder: it has no WorldObjects folder. Pick the folder that holds the game's WorldObjects folder and program.", path.display()))
}

pub fn is_install(dir: &Path) -> bool {
    dir.join("WorldObjects").is_dir()
}

/// Library folders listed in Steam's `steamapps/libraryfolders.vdf`: `"path" "D:\\Games"` entries (current format) or
/// `"1" "D:\\Games"` (older format).
pub fn library_folders(vdf: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for line in vdf.lines() {
        let words = quoted(line);
        if let [key, value] = words.as_slice() {
            let old_format = key.parse::<u32>().is_ok() && (value.contains('/') || value.contains('\\'));
            if key.eq_ignore_ascii_case("path") || old_format {
                out.push(PathBuf::from(value));
            }
        }
    }
    out
}

/// The quoted strings of one VDF line (`\\` and `\"` unescaped).
pub(super) fn quoted(line: &str) -> Vec<String> {
    let (mut words, mut chars) = (vec![], line.chars());
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut word = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => word.extend(chars.next()),
                c => word.push(c),
            }
        }
        words.push(word);
    }
    words
}

/// `SteamPath` from `reg query HKCU\Software\Valve\Steam /v SteamPath` output.
pub fn reg_value(output: &str) -> Option<PathBuf> {
    let line = output.lines().find(|l| l.trim_start().starts_with("SteamPath"))?;
    Some(PathBuf::from(line.split_once("REG_SZ")?.1.trim()))
}

/// Steam installs on this machine (their libraries are listed inside).
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = vec![];
    #[cfg(windows)]
    {
        // `reg` ships with Windows; reading the key needs no extra crate.
        let mut reg = Command::new("reg");
        reg.args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]);
        if let Ok(out) = no_window(&mut reg).output() {
            roots.extend(reg_value(&String::from_utf8_lossy(&out.stdout)));
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            roots.extend(std::env::var_os(var).map(|p| PathBuf::from(p).join("Steam")));
        }
    }
    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.extend([".steam/steam", ".local/share/Steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"].map(|p| home.join(p)));
    }
    roots
}

/// `(name, data)` of one value line in `reg query` output: `    Name    REG_SZ    data`.
pub(super) fn reg_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.trim_start().splitn(3, "    ");
    let (name, kind) = (parts.next()?, parts.next()?);
    kind.starts_with("REG_").then(|| (name, parts.next().unwrap_or("").trim_end()))
}

/// Folders named by installed-program entries whose `DisplayName` mentions Treasure Planet, from `reg query <Uninstall key> /s`
/// output: `InstallLocation`, and every quoted path (or the unquoted path) in `DisplayIcon` / `UninstallString` with its folder.
/// Disc installers chose their own folders, so this finds them wherever they went; callers keep only real installs.
pub fn uninstall_dirs(output: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in output.split("HKEY_") {
        let values: Vec<_> = entry.lines().filter_map(reg_line).collect();
        let get = |name: &str| values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| *v);
        if !get("DisplayName").is_some_and(|n| n.to_ascii_lowercase().contains("treasure planet")) {
            continue;
        }
        out.extend(get("InstallLocation").filter(|v| !v.is_empty()).map(|v| PathBuf::from(v.trim_matches('"'))));
        for value in ["DisplayIcon", "UninstallString"].into_iter().filter_map(get) {
            let quoted: Vec<&str> = value.split('"').skip(1).step_by(2).collect();
            let paths = if quoted.is_empty() { vec![value.split(',').next().unwrap_or("").trim()] } else { quoted };
            for path in paths.into_iter().filter(|p| !p.is_empty()) {
                // Registry paths use '\'; split by hand so the parent is also found when this runs (or is tested) off Windows.
                out.extend(path.rsplit_once(['\\', '/']).map(|(dir, _)| PathBuf::from(dir)));
                out.push(PathBuf::from(path));
            }
        }
    }
    out
}

/// Folders up to two levels below `dirs` whose name mentions Treasure Planet, with their subfolders (a publisher folder may
/// hold the game folder): disc installs in Program Files or a Wine prefix that left no installed-program entry.
pub fn scan(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let subdirs = |dir: &Path| -> Vec<PathBuf> { std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect() };
    let (mut out, mut level) = (vec![], dirs.to_vec());
    for _ in 0..2 {
        let mut next = vec![];
        for path in level.iter().flat_map(|d| subdirs(d)) {
            if path.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("treasure planet")) {
                out.extend(subdirs(&path));
                out.push(path);
            } else {
                next.push(path);
            }
        }
        level = next;
    }
    out
}

/// Places a disc (or other non-Steam) install may be: installed-program entries and Program Files on Windows, the default
/// Wine prefix elsewhere.
fn disc_candidates() -> Vec<PathBuf> {
    let mut found = vec![];
    #[cfg(windows)]
    {
        // 64-bit, 32-bit (a 2002 installer is 32-bit) and per-user entries.
        for key in [
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ] {
            let mut reg = Command::new("reg");
            reg.args(["query", key, "/s"]);
            if let Ok(out) = no_window(&mut reg).output() {
                found.extend(uninstall_dirs(&String::from_utf8_lossy(&out.stdout)));
            }
        }
        let program_files: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(|v| std::env::var_os(v).map(PathBuf::from)).collect();
        found.extend(scan(&program_files));
    }
    #[cfg(not(windows))]
    {
        let prefix = std::env::var_os("WINEPREFIX").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".wine")));
        if let Some(c) = prefix.map(|p| p.join("drive_c")) {
            found.extend(scan(&[c.join("Program Files (x86)"), c.join("Program Files")]));
        }
    }
    found
}

/// Game installs in every Steam library, then disc installs, without duplicates.
pub fn find_installs() -> Vec<PathBuf> {
    let mut candidates = vec![];
    for root in steam_roots() {
        let vdf = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")).unwrap_or_default();
        for library in std::iter::once(root).chain(library_folders(&vdf)) {
            candidates.push(library.join("steamapps").join("common").join(STEAM_DIR));
        }
    }
    candidates.extend(disc_candidates());
    let key = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut found: Vec<PathBuf> = vec![];
    for dir in candidates {
        if is_install(&dir) && !found.iter().any(|f| key(f) == key(&dir)) {
            found.push(dir);
        }
    }
    found
}
