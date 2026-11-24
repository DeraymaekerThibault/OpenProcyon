//! The command-line flags that print something and exit, before any window, asset or mod work: `--version`,
//! `--dump-schemas <dir>` and `--check-mod <dir> [--json]`. (The mission-authoring flags are in `rules/cli.rs`.)

use std::io::Write;
use std::path::Path;

/// Runs `--version` or `--dump-schemas <dir>` when given and returns the exit status; `None` when neither is.
pub fn quick(args: &[String]) -> Option<i32> {
    if args.iter().skip(1).any(|a| a == "--version") {
        println!("OpenProcyon {}", crate::authoring::ENGINE_VERSION);
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
    let warnings = findings.len() - errors;
    // A closed pipe (`| head`) is not an error.
    let out = &mut std::io::stdout();
    if json {
        let doc = serde_json::json!({"mod": dir, "engine": crate::authoring::ENGINE_VERSION, "errors": errors, "warnings": warnings, "findings": findings});
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&doc).expect("findings are JSON"));
    } else {
        for f in &findings {
            let _ = if f.file.is_empty() { writeln!(out, "{f}") } else { writeln!(out, "{}: {f}", f.file) };
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
        assert_eq!(quick(&args(&["game", "--map", "x"])), None, "other runs go on");
        assert_eq!(quick(&args(&["game", "--dump-schemas"])), Some(2), "a folder is needed");
        assert_eq!(quick(&args(&["game", "--dump-schemas", "--mod"])), Some(2));
    }

    #[test]
    fn dump_schemas_writes_one_file_per_type() {
        let dir = std::env::temp_dir().join(format!("procyon-schemas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("nested");
        assert_eq!(quick(&args(&["game", "--dump-schemas", &out.to_string_lossy()])), Some(0));
        let mut files: Vec<String> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        files.sort();
        assert_eq!(files, ["bullets.schema.json", "crew_nodes.schema.json", "guns.schema.json", "mod.schema.json", "ship.schema.json"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
