//! The HUD art the engine generates for a ship that brings none: [`ship_art`] returns it as data, without writing a file.
use std::path::Path;

use serde::Serialize;

use super::resolved_ship;

/// The weapon-panel slot a gun mount takes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GunBank {
    /// The mount's label (its key in `guns`).
    pub name: String,
    /// Bank number: `BankNN` is NN.
    pub bank: usize,
}

/// The art `shipart::generate` would make for a ship, as the files it would write hold it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShipArt {
    /// The command-bar side icon (`NoCursor.png` of `ShipBar_ShipIcon_<art>`); `None` when the ship has no readable model with triangles.
    pub icon_png: Option<Vec<u8>>,
    /// The weapon-panel outline (`NoCursor.png` of `WeaponBar_ShipIcon_<art>`), cropped to what is drawn.
    pub outline_png: Option<Vec<u8>>,
    /// Where the outline's cropped picture sits in its 126x230 box: left, top, right and bottom margins (the sidecar's `trim`).
    pub outline_trim: Option<[f64; 4]>,
    /// The bank of each gun, as the loader gives it (a bank the file names, else the panel slot nearest the gun).
    pub banks: Vec<GunBank>,
}

/// The side icon, the weapon-panel outline and each gun's bank of ship `id` in `root` (the base assets or an overlay): the same
/// pictures `shipart::generate` writes (it shares the code), whether or not the ship has art of its own. Writes nothing. Errors are
/// the ship loader's (see [`resolved_ship`]).
pub fn ship_art(root: &Path, id: &str) -> Result<ShipArt, String> {
    let ship = resolved_ship(root, id)?;
    let doc = super::read_merged(root, &format!("ships/{id}/ship.json")).map_err(|e| e.to_string())?;
    let facts = crate::shipart::model_facts(root, &doc, id);
    let icon = facts.as_ref().and_then(|f| crate::shipart::icon_sprite(&f.layers));
    let panel = facts.as_ref().and_then(|f| crate::shipart::panel_sprite(&f.layers));
    Ok(ShipArt {
        icon_png: icon.map(|s| s.png()),
        outline_trim: panel.as_ref().map(|s| s.trim),
        outline_png: panel.map(|s| s.png()),
        banks: ship.guns.iter().map(|g| GunBank { name: g.name.clone(), bank: g.bank }).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::derive::tests::model;
    use serde_json::json;

    fn is_png(b: &[u8]) -> bool {
        b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) && b.ends_with(&0xae42_6082u32.to_be_bytes())
    }

    #[test]
    fn a_ship_from_a_model_gets_two_pngs_and_banks_and_nothing_is_written() {
        let root = std::env::temp_dir().join(format!("procyon-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ships/Mod_Ship")).unwrap();
        std::fs::create_dir_all(root.join("data/weapons")).unwrap();
        let gun = json!({"Gun": {"MuzzleSpeed": 1, "TimeBetweenBursts": 1, "BulletsPerBurst": 1, "DuringBurstReloadTime": 1, "EffectiveRange_Range": 100, "LongRange_Range": 1,
            "MaximumRange_Range": 1, "EffectiveRange_AccuracyDeviation": 1, "LongRange_AccuracyDeviation": 1, "MaximumRange_AccuracyDeviation": 1, "BulletAffectedByGravity": false,
            "BulletTypeName_Primary": "B", "visual": {"pivot": [0, 0, 0], "muzzles": [[0, 0, 0]]}, "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 1, "VerticalMinAngle": 0, "VerticalMaxAngle": 1}});
        std::fs::write(root.join("data/weapons/guns.json"), gun.to_string()).unwrap();
        std::fs::write(root.join("data/weapons/bullets.json"), "{}").unwrap();
        std::fs::write(root.join("ships/Mod_Ship/model.glb"), model()).unwrap();
        std::fs::write(
            root.join("ships/Mod_Ship/ship.json"),
            json!({"guns": {"Gun_A": {"gun": "Gun", "arc_deg": 60}, "Named": {"gun": "Gun", "arc_deg": 60, "pos": [0, 0, 1], "rot": [0, 0, 0], "bank": "Bank05"}}}).to_string(),
        )
        .unwrap();
        let before: Vec<_> = walk(&root);
        let art = ship_art(&root, "Mod_Ship").unwrap();
        assert!(is_png(art.icon_png.as_ref().unwrap()) && is_png(art.outline_png.as_ref().unwrap()));
        assert_eq!(art.banks, [GunBank { name: "Gun_A".into(), bank: 0 }, GunBank { name: "Named".into(), bank: 5 }]);
        assert_eq!(walk(&root), before, "no file written");
        // `generate` writes the same bytes
        crate::shipart::generate(&root, &["Mod_Ship".into()]);
        let side = std::fs::read(root.join("hud/sprites/ShipBar_ShipIcon_Mod_Ship/Navy/NoCursor.png")).unwrap();
        let top = std::fs::read(root.join("hud/sprites/WeaponBar_ShipIcon_Mod_Ship/Navy/NoCursor.png")).unwrap();
        assert_eq!((Some(side), Some(top)), (art.icon_png, art.outline_png));
        let sidecar = crate::data::try_json(&root.join("hud/sprites/WeaponBar_ShipIcon_Mod_Ship/Navy.json")).unwrap();
        assert_eq!(sidecar["states"]["NoCursor"]["trim"], json!(art.outline_trim.unwrap()));
        // a ship without a model has no art but still has banks
        std::fs::remove_file(root.join("ships/Mod_Ship/model.glb")).unwrap();
        std::fs::write(root.join("ships/Mod_Ship/geometry.json"), json!({}).to_string()).unwrap();
        std::fs::write(
            root.join("ships/Mod_Ship/ship.json"),
            json!({"guns": {"G": {"gun": "Gun", "arc_deg": 1, "pos": [0, 0, 0], "rot": [0, 0, 0], "bank": "Bank02"}}}).to_string(),
        )
        .unwrap();
        let bare = ship_art(&root, "Mod_Ship").unwrap();
        assert_eq!((bare.icon_png, bare.outline_png, bare.banks.len()), (None, None, 1));
        assert!(ship_art(&root, "Nowhere").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn walk(dir: &Path) -> Vec<String> {
        let mut out = vec![];
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            out.push(p.display().to_string());
            if p.is_dir() {
                out.extend(walk(&p));
            }
        }
        out.sort();
        out
    }
}
