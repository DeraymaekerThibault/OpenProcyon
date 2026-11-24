//! The command-line flags that print something and exit, before any window, asset or mod work: `--version` and
//! `--dump-schemas <dir>`. (The mission-authoring flags are in `rules/cli.rs`.)

use std::path::Path;

/// Runs `--version` or `--dump-schemas <dir>` when given and returns the exit status; `None` when neither is.
pub fn quick(args: &[String]) -> Option<i32> {
    if args.iter().skip(1).any(|a| a == "--version") {
        println!("OpenProcyon {}", crate::authoring::ENGINE_VERSION);
        return Some(0);
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
