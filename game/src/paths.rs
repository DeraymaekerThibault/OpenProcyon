//! Where the game finds its files: the imported assets, `content/`, the bundled mods and the overlay the launch builds.

use crate::{campaign, modding};
use std::path::PathBuf;

static ASSETS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
/// Converted assets: `PROCYON_ASSETS`, else `assets/` in the source checkout, else the per-user folder the importer
/// (`python -m tools.import_assets`, or the release download's `openprocyon-import`) fills. Nothing from the original game ships with the code.
pub(crate) fn base_assets() -> PathBuf {
    find_base_assets().unwrap_or_else(|| PathBuf::from("assets"))
}
/// The lookup behind `base_assets`, with no fallback: `None` when there is no env var, no checkout and no per-user folder to name.
/// Touches nothing on disk.
pub(crate) fn find_base_assets() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PROCYON_ASSETS") {
        return Some(PathBuf::from(dir));
    }
    let checkout = checkout_dir().map(|dir| dir.join("assets"));
    if let Some(checkout) = checkout.as_ref().filter(|c| c.join("ships").is_dir()) {
        return Some(checkout.clone());
    }
    user_assets().or(checkout)
}

/// The source checkout, found at run time (never baked into the binary, so a release build carries no build-machine path): the
/// nearest folder holding `game/Cargo.toml` and `content/`, looking up from the working directory, then from the executable
/// (`make run` and the check scripts start inside the checkout or build into it).
pub(crate) fn checkout_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok();
    let binary = std::env::current_exe().ok();
    [cwd, binary]
        .into_iter()
        .flatten()
        .find_map(|start| start.ancestors().find(|dir| dir.join("game/Cargo.toml").is_file() && dir.join("content").is_dir()).map(|dir| dir.to_path_buf()))
}

/// Where the importer puts converted assets by default (see `campaign::user_dir`).
pub(crate) fn user_assets() -> Option<PathBuf> {
    Some(campaign::user_dir()?.join("assets"))
}
/// Whether `base` is the per-user folder a player's download imports into (not PROCYON_ASSETS or the checkout's assets/).
pub(crate) fn player_assets(base: &std::path::Path) -> bool {
    std::env::var_os("PROCYON_ASSETS").is_none() && user_assets().is_some_and(|u| u == base)
}
/// Files that ship with the code (fonts, hand-written data), layered over the imported assets: `content/` next to the
/// executable (the release download), else `content/` in the checkout.
pub(crate) fn content_dir() -> PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("content")));
    content_dir_from(exe_dir, checkout_dir().map_or_else(|| PathBuf::from("content"), |dir| dir.join("content")))
}
/// The `mods/` that ships with the game: next to the executable in a download, the checkout's under `make run`.
pub fn bundled_mods_dir() -> Option<PathBuf> {
    let beside_binary = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.join("mods")));
    beside_binary.filter(|dir| dir.is_dir()).or_else(|| checkout_dir().map(|dir| dir.join("mods")).filter(|d| d.is_dir()))
}
fn content_dir_from(next_to_exe: Option<PathBuf>, checkout: PathBuf) -> PathBuf {
    next_to_exe.filter(|dir| dir.join("fonts").is_dir()).unwrap_or(checkout)
}
/// PROCYON_OVERLAY: where this launch builds its overlay (modding::overlay).
pub(crate) fn own_overlay() -> Option<PathBuf> {
    std::env::var_os("PROCYON_OVERLAY").map(PathBuf::from)
}
/// The asset root: the imported assets with `content/` and any `--mod` folders overlaid on top (see modding.rs).
pub fn assets_dir() -> &'static std::path::Path {
    ASSETS.get_or_init(|| modding::overlay(&base_assets(), &[content_dir()], own_overlay()))
}

/// Fixes the asset root once the mods are chosen (`run`).
pub(crate) fn set_assets(root: PathBuf) {
    let _ = ASSETS.set(root);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_next_to_the_executable_wins_over_the_checkout() {
        let dir = std::env::temp_dir().join(format!("procyon-content-test-{}", std::process::id()));
        let (unpacked, checkout) = (dir.join("download/content"), dir.join("checkout/content"));
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout.clone()), checkout, "no content/ next to a cargo-built binary");
        std::fs::create_dir_all(unpacked.join("fonts")).unwrap();
        assert_eq!(content_dir_from(Some(unpacked.clone()), checkout), unpacked);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
