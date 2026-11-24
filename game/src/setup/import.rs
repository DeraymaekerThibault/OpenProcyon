//! Running the importer: the asset stamp, staging and swap, the child process and its progress lines.
use super::*;

/// Why the import screen opened instead of the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    Ready,
    /// No converted assets yet.
    Missing,
    /// Converted by another version (`None`: before imports were stamped); `game` is the install it came from.
    Outdated {
        version: Option<String>,
        game: Option<PathBuf>,
    },
}

/// The state of a player's asset folder for game version `version`: every release converts the game again.
pub fn imported(dir: &Path, version: &str) -> Imported {
    if !dir.join("ships").is_dir() {
        return Imported::Missing;
    }
    let stamp: serde_json::Value = std::fs::read_to_string(dir.join(STAMP)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let found = stamp["version"].as_str().map(String::from);
    if found.as_deref() == Some(version) {
        return Imported::Ready;
    }
    Imported::Outdated { version: found, game: stamp["game"].as_str().map(PathBuf::from) }
}

pub fn write_stamp(dir: &Path, version: &str, game: &Path) -> std::io::Result<()> {
    let stamp = serde_json::json!({ "version": version, "game": game.display().to_string() });
    std::fs::write(dir.join(STAMP), format!("{stamp:#}\n"))
}

/// The folder a player's import is written to before it replaces `out`, so a failed or cancelled import keeps the old one.
pub fn staging(out: &Path) -> PathBuf {
    out.with_file_name(format!("{}.importing", out.file_name().map_or("assets".into(), |n| n.to_string_lossy())))
}

/// Puts the finished import `staged` in place of `out` (the old folder is removed only once the new one is in place).
pub fn swap_in(staged: &Path, out: &Path) -> std::io::Result<()> {
    let old = out.with_file_name(format!("{}.replaced", out.file_name().map_or("assets".into(), |n| n.to_string_lossy())));
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    if out.exists() {
        retry(|| std::fs::rename(out, &old))?;
    }
    if let Err(err) = retry(|| std::fs::rename(staged, out)) {
        let _ = std::fs::rename(&old, out);
        return Err(err);
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Windows refuses to rename a folder while a virus scanner or the indexer briefly holds a file in it.
fn retry(mut f: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match f() {
            Err(_) if tries < 20 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            done => return done,
        }
    }
}

/// `[n/total] step` from the importer, optionally `[n/total] step: i/k (p%)` (a part of the step; p is the whole import).
pub fn parse_progress(line: &str) -> Option<(u32, u32, &str, Option<u32>)> {
    let (count, name) = line.trim().strip_prefix('[')?.split_once("] ")?;
    let (n, total) = count.split_once('/')?;
    let (n, total) = (n.parse().ok()?, total.parse().ok()?);
    let name = name.trim();
    let percent = name.strip_suffix("%)").and_then(|s| s.rsplit_once(" (")).and_then(|(rest, p)| Some((rest.trim_end(), p.parse::<u32>().ok()?.min(100))));
    let (name, percent) = percent.map_or((name, None), |(rest, p)| (rest, Some(p)));
    (n >= 1 && n <= total).then_some((n, total, name, percent))
}

/// The step name of the importer's `import failed at step 'name': ...` line.
pub fn failed_step(line: &str) -> Option<String> {
    let rest = line.split_once("import failed at step ")?.1;
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    Some(rest[1..].split(quote).next()?.to_owned())
}

pub(super) fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up for the child
    }
    cmd
}

/// The importers to try, in order, each with `args`: `openprocyon-import` next to the game (the release download), else Python on
/// the checkout's `tools.import_assets` (its `.venv` first).
fn importers(args: &[std::ffi::OsString]) -> Vec<(String, Command)> {
    let args = |cmd: &mut Command| {
        cmd.args(args);
    };
    let mut list = vec![];
    let name = if cfg!(windows) { "openprocyon-import.exe" } else { "openprocyon-import" };
    if let Some(frozen) = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join(name))).filter(|p| p.is_file()) {
        let mut cmd = Command::new(&frozen);
        args(&mut cmd);
        list.push((frozen.display().to_string(), cmd));
    }
    if let Some(checkout) = crate::checkout_dir().filter(|c| c.join("tools/import_assets.py").is_file()) {
        let venv = [".venv/Scripts/python.exe", ".venv/bin/python"].map(|p| checkout.join(p));
        let pythons: Vec<(String, Vec<&str>)> = venv
            .iter()
            .filter(|p| p.is_file())
            .map(|p| (p.display().to_string(), vec![]))
            .chain(if cfg!(windows) {
                vec![("py".into(), vec!["-3"]), ("python".into(), vec![]), ("python3".into(), vec![])]
            } else {
                vec![("python3".into(), vec![]), ("python".into(), vec![])]
            })
            .collect();
        for (python, pre) in pythons {
            let mut cmd = Command::new(&python);
            cmd.args(pre).args(["-m", "tools.import_assets"]).current_dir(&checkout);
            args(&mut cmd);
            list.push((format!("{python} -m tools.import_assets"), cmd));
        }
    }
    list
}

/// Starts the first importer that runs with `args` (an import, or `--bake-skies` for the Mods screen); its output lines and
/// exit arrive on `tx`. Returns which importer runs and the child, for stopping it. With `pipe_stdin` the child's standard
/// input is a pipe (take it from the child to hold it open; the baker's `--watch-stdin` stops when it closes), else empty.
pub fn spawn_tool<M: From<ToolMsg> + Send + 'static>(args: &[std::ffi::OsString], pipe_stdin: bool, tx: mpsc::Sender<M>) -> Result<(String, Arc<Mutex<Child>>), String> {
    let mut tried = vec![];
    for (label, mut cmd) in importers(args) {
        let stdin = if pipe_stdin { Stdio::piped() } else { Stdio::null() };
        no_window(&mut cmd).env("PYTHONUNBUFFERED", "1").stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                tried.push(format!("{label}: {err}"));
                continue;
            }
        };
        let readers = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
            .map(|stream| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                        let _ = tx.send(ToolMsg::Line(String::from_utf8_lossy(&line).trim_end().to_owned()).into());
                    }
                })
            })
            .collect::<Vec<_>>();
        let child = Arc::new(Mutex::new(child));
        let watched = child.clone();
        std::thread::spawn(move || {
            for reader in readers {
                let _ = reader.join();
            }
            let status = loop {
                match watched.lock().map(|mut c| c.try_wait()) {
                    Ok(Ok(Some(status))) => break Ok(status),
                    Ok(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(50)),
                    Ok(Err(err)) => break Err(err.to_string()),
                    Err(_) => break Err("lost the importer".into()),
                }
            };
            let _ = tx.send(
                ToolMsg::Exit(match status {
                    Ok(s) if s.success() => Ok(()),
                    Ok(s) => Err(format!("The importer stopped ({s}).")),
                    Err(err) => Err(err),
                })
                .into(),
            );
        });
        return Ok((label, child));
    }
    Err(format!("Could not start the importer. The download has openprocyon-import next to the game; from source, Python 3 with requirements.txt is needed.\n{}", tried.join("\n")))
}
