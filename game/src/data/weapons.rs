//! Typed gun and bullet definitions (`data/weapons/guns.json`, `bullets.json`; layout: docs/data-format.md).
//! Field names and defaults are exactly those the untyped reader had: a field it required is required here (the error names the
//! entry and the field), a field it tolerated (`as_*().unwrap_or`) is tolerated the same way, and unknown fields are allowed.
use super::*;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

/// A field the original reader took with `as_str()`, `as_f64()`, ... and a default: a missing value or one of the wrong type is `None`.
pub(crate) fn lenient<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(Value::deserialize(d).ok().and_then(|v| serde_json::from_value(v).ok()))
}

/// A game-frame point: at least three numbers (more are ignored, as before).
#[derive(Clone, Copy, JsonSchema)]
#[schemars(with = "Vec<f64>")]
struct Point(Vec3);
impl<'de> Deserialize<'de> for Point {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<f64>::deserialize(d)?;
        match v[..] {
            [x, y, z, ..] => Ok(Point(conv(x as f32, y as f32, z as f32))),
            _ => Err(serde::de::Error::invalid_length(v.len(), &"3 numbers for a point (visual.pivot, visual.muzzles)")),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct Visual {
    /// Turret base mesh name.
    #[serde(default, deserialize_with = "lenient")]
    base: Option<String>,
    /// Barrel mesh name.
    #[serde(default, deserialize_with = "lenient")]
    barrel: Option<String>,
    /// Barrel pivot, in the base frame.
    pivot: Point,
    /// Muzzle points, in the base frame.
    muzzles: Vec<Point>,
}

/// One entry of `guns.json`.
#[derive(Deserialize, JsonSchema)]
pub(super) struct GunDoc {
    /// Mount size class, e.g. `Medium Mount`, `Heavy  Mount`, `Non-Selectable Special Mount` (whitespace normalised on load).
    #[serde(rename = "Mount Type", default, deserialize_with = "lenient")]
    mount_type: Option<String>,
    /// Fleet cost in victory points (non-negative integer). Missing: the gun cannot be bought.
    #[serde(rename = "Victory Point Cost", default, deserialize_with = "lenient")]
    points: Option<u32>,
    /// `None` or a race that cannot fit this gun.
    #[serde(rename = "Exclusion Race", default, deserialize_with = "lenient")]
    exclusion: Option<String>,
    /// `true` for a point-defence gun (shoots down torpedoes).
    #[serde(rename = "Is PointDefense Gun", default, deserialize_with = "lenient")]
    point_defense: Option<bool>,
    /// `true` for a torpedo launcher.
    #[serde(rename = "Is TorpedoLauncher Gun", default, deserialize_with = "lenient")]
    torpedo_launcher: Option<bool>,
    /// Shell speed leaving the muzzle.
    #[serde(rename = "MuzzleSpeed")]
    muzzle: f64,
    /// Seconds between bursts (the reload).
    #[serde(rename = "TimeBetweenBursts")]
    reload: f64,
    /// Shots per burst.
    #[serde(rename = "BulletsPerBurst")]
    burst: u32,
    /// Seconds between shots inside a burst.
    #[serde(rename = "DuringBurstReloadTime")]
    burst_reload: f64,
    /// Whether each shot of a burst gets its own deviation.
    #[serde(rename = "CalculateAccuracyDeviationDuringBurst", default, deserialize_with = "lenient")]
    dev_per_shot: Option<bool>,
    /// Effective range.
    #[serde(rename = "EffectiveRange_Range")]
    effective_range: f64,
    /// Long range.
    #[serde(rename = "LongRange_Range")]
    long_range: f64,
    /// Maximum range.
    #[serde(rename = "MaximumRange_Range")]
    maximum_range: f64,
    /// Ricochet factor at effective range (0 when absent).
    #[serde(rename = "EffectiveRange_RicochetFactor", default, deserialize_with = "lenient")]
    effective_ricochet: Option<f64>,
    /// Ricochet factor at long range (0 when absent).
    #[serde(rename = "LongRange_RicochetFactor", default, deserialize_with = "lenient")]
    long_ricochet: Option<f64>,
    /// Ricochet factor at maximum range (0 when absent).
    #[serde(rename = "MaximumRange_RicochetFactor", default, deserialize_with = "lenient")]
    maximum_ricochet: Option<f64>,
    /// Accuracy deviation at effective range.
    #[serde(rename = "EffectiveRange_AccuracyDeviation")]
    effective_deviation: f64,
    /// Accuracy deviation at long range.
    #[serde(rename = "LongRange_AccuracyDeviation")]
    long_deviation: f64,
    /// Accuracy deviation at maximum range.
    #[serde(rename = "MaximumRange_AccuracyDeviation")]
    maximum_deviation: f64,
    /// Whether the shell drops under gravity.
    #[serde(rename = "BulletAffectedByGravity")]
    gravity: bool,
    /// The shell fired: a key of `data/weapons/bullets.json`.
    #[serde(rename = "BulletTypeName_Primary")]
    bullet: String,
    /// Turret meshes: `base`, `barrel`, `pivot` and `muzzles` (points in the gun's frame, game axes).
    visual: Visual,
    /// Turret turn rate, radians per second.
    #[serde(rename = "HorizontalRotationSpeed")]
    yaw_speed: f64,
    /// Barrel elevation rate, radians per second.
    #[serde(rename = "VerticalRotationSpeed")]
    pitch_speed: f64,
    /// Lowest barrel elevation, radians.
    #[serde(rename = "VerticalMinAngle")]
    pitch_min: f64,
    /// Highest barrel elevation, radians.
    #[serde(rename = "VerticalMaxAngle")]
    pitch_max: f64,
    /// Effect played at the muzzle.
    #[serde(rename = "MuzzleFlashEffect", default, deserialize_with = "lenient")]
    muzzle_fx: Option<String>,
    /// Fire sound (a beam gun plays it once as the charge starts).
    #[serde(rename = "SoundName", default, deserialize_with = "lenient")]
    sound: Option<String>,
    /// Fire sound volume.
    #[serde(rename = "SoundVolume", default, deserialize_with = "lenient")]
    sound_volume: Option<f64>,
    /// Distance beyond which the fire sound is not heard.
    #[serde(rename = "SoundMaxDistance", default, deserialize_with = "lenient")]
    sound_distance: Option<f64>,
    /// `true` for mortars and gravity charges, which fire on the fixed high arc `LobAngle`.
    #[serde(rename = "Is Lobbing Gun", default, deserialize_with = "lenient")]
    lobbing: Option<bool>,
    /// Lob arc angle, radians.
    #[serde(rename = "LobAngle", default, deserialize_with = "lenient")]
    lob_angle: Option<f64>,
    /// `true` for torpedoes and gatlings.
    #[serde(rename = "Can Be Fired While Cloaked", default, deserialize_with = "lenient")]
    cloak_fire: Option<bool>,
    /// Not retail: a beam gun's `beam` block (`charge`, `duration`, `tick`, `ramp`, `max_power`; see docs/modding.md "A new weapon").
    /// Not retail: the mod beam block (`BeamDef::from_json` keeps its own defaults).
    #[serde(default)]
    beam: Value,
}

impl GunDoc {
    pub(super) fn into_def(self) -> GunDef {
        let f = |v: f64| v as f32;
        let bullet = self.bullet;
        GunDef {
            mount_type: self.mount_type.map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")),
            points: self.points,
            exclusion: self.exclusion.filter(|s| s != "None"),
            point_defense: self.point_defense.unwrap_or(false),
            torpedo_launcher: self.torpedo_launcher.unwrap_or(false),
            muzzle: f(self.muzzle),
            reload: f(self.reload),
            burst: self.burst,
            burst_reload: f(self.burst_reload),
            dev_per_shot: self.dev_per_shot.unwrap_or(false),
            range: [f(self.effective_range), f(self.long_range), f(self.maximum_range)],
            ricochet: [self.effective_ricochet, self.long_ricochet, self.maximum_ricochet].map(|r| r.unwrap_or(0.0) as f32),
            deviation: [f(self.effective_deviation), f(self.long_deviation), f(self.maximum_deviation)],
            gravity: self.gravity,
            family: GunFamily::Cannon,
            bullet_family: GunFamily::of(&bullet),
            bullet_torpedo: is_torpedo_bullet(&bullet),
            cloak_fire: self.cloak_fire.unwrap_or_else(|| is_torpedo_bullet(&bullet) || bullet == "Bullet_Gattling"),
            bullet,
            base: self.visual.base,
            barrel: self.visual.barrel,
            pivot: self.visual.pivot.0,
            muzzles: self.visual.muzzles.iter().map(|p| p.0).collect(),
            yaw_speed: f(self.yaw_speed),
            pitch_speed: f(self.pitch_speed),
            pitch_limits: [f(self.pitch_min), f(self.pitch_max)],
            muzzle_fx: self.muzzle_fx,
            sound: self.sound.filter(|s| !s.is_empty()).map(|s| (s, self.sound_volume.unwrap_or(1.0) as f32, self.sound_distance.unwrap_or(600.0) as f32)),
            lob: (self.lobbing == Some(true)).then(|| self.lob_angle.unwrap_or(0.785) as f32),
            beam: BeamDef::from_json(&self.beam),
        }
    }
}

/// One entry of `bullets.json`.
#[derive(Deserialize, JsonSchema)]
pub(super) struct BulletDoc {
    /// Torpedoes only: `Mass`, `MaxThrust`, `MaxSpeed`, `RotationalFriction`, `MaxAngularAcceleration` for guided flight.
    #[serde(default, deserialize_with = "lenient")]
    physics: Option<serde_json::Map<String, Value>>,
    /// Model of the shell.
    #[serde(default, deserialize_with = "lenient")]
    glb: Option<String>,
    /// Seconds the shell flies before it expires.
    lifetime: f64,
    /// Hit points of a shell that can be shot down.
    #[serde(default, deserialize_with = "lenient")]
    hitpoints: Option<f64>,
    /// Impact sound volume.
    impact_vol: f64,
    /// Distance beyond which the impact sound is not heard.
    impact_max: f64,
    /// Impact sounds by material name: a list of sound names each.
    impact_snd: HashMap<String, Vec<String>>,
    #[serde(default, deserialize_with = "lenient")]
    net: Option<serde_json::Map<String, Value>>,
    /// Effect played where the shell hits.
    #[serde(default, deserialize_with = "lenient")]
    hit_fx: Option<String>,
    /// Effect carried by the flying shell.
    #[serde(default, deserialize_with = "lenient")]
    bullet_fx: Option<String>,
    /// Kind of projectile.
    #[serde(default, deserialize_with = "lenient")]
    kind: Option<String>,
    #[serde(default)]
    blast: Value,
    #[serde(default)]
    mine: Value,
    #[serde(default)]
    travel_snd: Value,
    /// Damage decal size: `Small`, `Medium` or `Large`.
    #[serde(default, deserialize_with = "lenient")]
    decal: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    ricochet_snd: Option<serde_json::Map<String, Value>>,
    /// Damage by material: `[lower, upper, crit chance, fire chance, initial fire strength]`; more numbers are ignored.
    damage: BTreeMap<String, Vec<f64>>,
}

impl BulletDoc {
    /// `name` only appears in the errors.
    pub(super) fn into_def(self, name: &str) -> Result<BulletDef, String> {
        let physics = self
            .physics
            .map(|p| {
                let get = |key: &str| p.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN) as f32;
                let physics = Physics {
                    mass: get("Mass"),
                    thrust: get("MaxThrust"),
                    max_speed: get("MaxSpeed"),
                    rot_friction: get("RotationalFriction"),
                    max_ang_acc: get("MaxAngularAcceleration"),
                };
                let ok = [physics.mass, physics.thrust, physics.max_speed, physics.max_ang_acc].iter().all(|v| v.is_finite() && *v > 0.0)
                    && physics.rot_friction.is_finite()
                    && physics.rot_friction >= 0.0;
                ok.then_some(physics).ok_or_else(|| {
                    format!("bullet {name:?}: invalid physics (Mass, MaxThrust, MaxSpeed and MaxAngularAcceleration must be positive numbers, RotationalFriction not negative)")
                })
            })
            .transpose()?;
        // Mod bullets may omit materials: missing ones deal no damage instead of panicking in `shells`.
        let mut damage: HashMap<String, [f32; 5]> = crate::combat::MATERIALS.iter().map(|m| (m.to_string(), [0.0; 5])).collect();
        for (material, d) in &self.damage {
            let Some(&[a, b, c, e, f]) = d.get(..5).map(|s| <&[f64; 5]>::try_from(s).unwrap()) else {
                return Err(format!("bullet {name:?}: damage.{material} needs 5 numbers, got {}", d.len()));
            };
            damage.insert(material.clone(), [a, b, c, e, f].map(|v| v as f32));
        }
        let f = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
        let payload = match (self.kind.as_deref().unwrap_or(""), &self.blast, &self.mine) {
            (_, _, m) if m.is_object() => Payload::Mine { sight: f(m, "sight"), speed: f(m, "speed") },
            ("NovaMortar", ..) => Payload::Nova,
            // 40000: the retail value, for imports made before `magnitude` was exported.
            ("GravCharge", bl, _) => Payload::Grav { radius: f(bl, "radius"), duration: f(bl, "duration"), magnitude: bl["magnitude"].as_f64().map_or(40000.0, |m| m as f32) },
            (_, bl, _) if bl.is_object() => Payload::Blast { radius: f(bl, "radius"), duration: f(bl, "duration") },
            _ => Payload::Shell,
        };
        Ok(BulletDef {
            physics,
            glb: self.glb,
            lifetime: self.lifetime as f32,
            hitpoints: self.hitpoints.unwrap_or(1.0) as f32,
            impact_vol: self.impact_vol as f32,
            impact_max: self.impact_max as f32,
            impact_snd: self.impact_snd,
            family: GunFamily::Cannon,
            torpedo: false,
            net: self.net.map(|n| (n.get("duration").and_then(Value::as_f64).unwrap_or(0.0) as f32, n.get("dissipated").and_then(Value::as_f64).unwrap_or(0.0) as f32)),
            hit_fx: self.hit_fx,
            bullet_fx: self.bullet_fx,
            payload,
            travel_snd: self.travel_snd.as_array().and_then(|a| Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))),
            decal: self.decal.and_then(|d| ["Small", "Medium", "Large"].iter().position(|x| *x == d)).map(|i| i as u8),
            ricochet_snd: self.ricochet_snd.map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default(),
            damage,
        })
    }
}

/// A gun entry as a def, the gun's name fixing its family. Errors name the entry and the field.
pub fn gun_from_value(name: &str, v: Value) -> Result<GunDef, String> {
    let doc: GunDoc = serde_json::from_value(v).map_err(|e| format!("gun {name:?}: {e}"))?;
    Ok(GunDef { family: GunFamily::of(name), ..doc.into_def() })
}

/// A bullet entry as a def, the bullet's name fixing its family and torpedo flag.
pub fn bullet_from_value(name: &str, v: Value) -> Result<BulletDef, String> {
    let doc: BulletDoc = serde_json::from_value(v).map_err(|e| format!("bullet {name:?}: {e}"))?;
    Ok(BulletDef { family: GunFamily::of(name), torpedo: is_torpedo_bullet(name), ..doc.into_def(name)? })
}

/// The schema of `data/weapons/guns.json`: an object of gun entries by name. Mods merge this file key by key, so a mod entry that only
/// changes some keys of a retail gun is valid. (`authoring::schemas` publishes these schemas without `required`.)
pub(super) fn guns_schema() -> Value {
    table_schema::<GunDoc>("guns.json")
}
/// The schema of `data/weapons/bullets.json` (see `guns_schema`).
pub(super) fn bullets_schema() -> Value {
    table_schema::<BulletDoc>("bullets.json")
}
/// A file of entries by name, each an entry `T` (and `$schema`).
pub(super) fn table_schema<T: JsonSchema>(title: &str) -> Value {
    let mut v = serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<T>()).expect("schema is JSON");
    let entry = serde_json::json!({"type": "object", "additionalProperties": {"$ref": "#/$defs/entry"}, "properties": {"$schema": {"type": "string"}}});
    let mut file = entry;
    file["title"] = title.into();
    file["$schema"] = v["$schema"].take();
    let mut defs = v.get("$defs").cloned().unwrap_or_else(|| serde_json::json!({}));
    v.as_object_mut().expect("object").remove("$schema");
    v.as_object_mut().expect("object").remove("$defs");
    defs["entry"] = v;
    file["$defs"] = defs;
    file
}

/// A weapons table (a JSON object of named entries) read through the typed path; errors name the file.
fn table<T>(path: &std::path::Path, entry: fn(&str, Value) -> Result<T, String>) -> Result<HashMap<String, T>, String> {
    let mut entries: BTreeMap<String, Value> = load(path).map_err(|e| e.to_string())?;
    entries.remove("$schema"); // an editor hint (`docs/modding.md` "JSON schemas"), not an entry
    let source = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    entries.into_iter().map(|(name, v)| entry(&name, v).map(|t| (name, t)).map_err(|e| format!("{} (source {}): {e}", path.display(), source.display()))).collect()
}

/// Every gun and bullet definition (`data/weapons/*.json`), shared by all ships.
pub fn try_load_weapons(root: &std::path::Path) -> Result<(HashMap<String, GunDef>, HashMap<String, BulletDef>), String> {
    let dir = root.join("data/weapons");
    Ok((table(&dir.join("guns.json"), gun_from_value)?, table(&dir.join("bullets.json"), bullet_from_value)?))
}

/// Ships fit only guns that exist and fire only bullets that exist: a mod that renames or omits an entry gets this error,
/// naming the ship, the gun and the missing name, instead of an index panic in combat. `mounts`: (ship, gun) pairs.
pub fn check_weapon_refs<'a>(mounts: impl IntoIterator<Item = (&'a str, &'a str)>, guns: &HashMap<String, GunDef>, bullets: &HashMap<String, BulletDef>) -> Result<(), String> {
    for (ship, gun_name) in mounts {
        let gun = guns.get(gun_name).ok_or_else(|| format!("ship {ship}: a mount fits gun {gun_name:?}, which is not in data/weapons/guns.json"))?;
        if !bullets.contains_key(&gun.bullet) {
            return Err(format!("ship {ship}: gun {gun_name:?} fires bullet {:?}, which is not in data/weapons/bullets.json", gun.bullet));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn gun() -> Value {
        json!({
            "MuzzleSpeed": 900, "TimeBetweenBursts": 2.5, "BulletsPerBurst": 3, "DuringBurstReloadTime": 0.2,
            "EffectiveRange_Range": 100, "LongRange_Range": 200, "MaximumRange_Range": 300,
            "EffectiveRange_AccuracyDeviation": 0.01, "LongRange_AccuracyDeviation": 0.02, "MaximumRange_AccuracyDeviation": 0.03,
            "BulletAffectedByGravity": true, "BulletTypeName_Primary": "Bullet_Torpedo",
            "visual": {"pivot": [1, 2, 3], "muzzles": [[0, 1, 0], [0, 2, 0]]},
            "HorizontalRotationSpeed": 1, "VerticalRotationSpeed": 2, "VerticalMinAngle": -0.1, "VerticalMaxAngle": 0.5
        })
    }
    fn bullet() -> Value {
        json!({"lifetime": 4, "impact_vol": 1, "impact_max": 2, "impact_snd": {"Wood": ["a", "b"]}, "damage": {"Wood": [1, 2, 0, 0, 0, 9]}})
    }
    fn both(name: &str, g: &Value) {
        let old = GunDef { family: GunFamily::of(name), ..super::super::legacy::gun_def(g) };
        assert_eq!(gun_from_value(name, g.clone()).unwrap(), old, "{g}");
    }

    #[test]
    fn a_minimal_gun_gets_every_default_and_matches_the_untyped_reader() {
        let g = gun_from_value("Gun_Torpedo", gun()).unwrap();
        assert_eq!((g.burst, g.muzzle, g.points, g.exclusion, g.sound, g.lob), (3, 900.0, None, None, None, None));
        assert!(g.cloak_fire && g.bullet_torpedo, "torpedoes fire cloaked unless the gun says otherwise");
        assert_eq!(g.ricochet, [0.0; 3]);
        assert_eq!(g.pivot, conv(1.0, 2.0, 3.0));
        both("Gun_Torpedo", &gun());
        let mut odd = gun();
        odd["Mount Type"] = json!("  Heavy   Port ");
        odd["Victory Point Cost"] = json!("not a number");
        odd["Exclusion Race"] = json!("None");
        odd["SoundName"] = json!("");
        odd["SoundVolume"] = json!("loud");
        odd["Is Lobbing Gun"] = json!(true);
        odd["Can Be Fired While Cloaked"] = json!(false);
        odd["LongRange_RicochetFactor"] = json!(0.5);
        odd["beam"] = json!({"charge": 2});
        both("Gun_Mortar", &odd);
        odd["SoundName"] = json!("Fire");
        both("Gun_Cannon", &odd);
        assert_eq!(gun_from_value("Gun_Cannon", odd).unwrap().sound, Some(("Fire".into(), 1.0, 600.0)));
    }

    #[test]
    fn a_gun_missing_a_required_field_names_the_gun_and_the_field() {
        let mut g = gun();
        g.as_object_mut().unwrap().remove("MuzzleSpeed");
        let e = gun_from_value("Gun_Mod", g).unwrap_err();
        assert!(e.contains("Gun_Mod") && e.contains("MuzzleSpeed"), "{e}");
        let mut g = gun();
        g["visual"]["pivot"] = json!([1, 2]);
        let e = gun_from_value("Gun_Mod", g).unwrap_err();
        assert!(e.contains("Gun_Mod") && e.contains("pivot"), "{e}");
    }

    #[test]
    fn bullets_default_like_the_untyped_reader_and_report_bad_entries_by_name() {
        let old = BulletDef { family: GunFamily::of("Bullet_Mod"), torpedo: false, ..super::super::legacy::bullet_def(&bullet()) };
        let b = bullet_from_value("Bullet_Mod", bullet()).unwrap();
        assert_eq!(b, old);
        assert_eq!((b.hitpoints, b.damage["Wood"], b.damage["Stone"], b.payload), (1.0, [1.0, 2.0, 0.0, 0.0, 0.0], [0.0; 5], Payload::Shell));
        let mut bad = bullet();
        bad["damage"]["Wood"] = json!([1, 2]);
        let e = bullet_from_value("Bullet_Mod", bad).unwrap_err();
        assert!(e.contains("Bullet_Mod") && e.contains("damage.Wood"), "{e}");
        let mut bad = bullet();
        bad["physics"] = json!({"Mass": 1});
        let e = bullet_from_value("Bullet_Mod", bad).unwrap_err();
        assert!(e.contains("Bullet_Mod") && e.contains("physics"), "{e}");
        let mut bad = bullet();
        bad.as_object_mut().unwrap().remove("lifetime");
        assert!(bullet_from_value("Bullet_Mod", bad).unwrap_err().contains("lifetime"));
    }

    #[test]
    fn a_ship_fitting_a_missing_gun_or_bullet_gets_an_error_naming_ship_and_entry() {
        let guns: HashMap<_, _> = [("Gun_A".to_string(), gun_from_value("Gun_A", gun()).unwrap())].into();
        let bullets: HashMap<_, _> = HashMap::new();
        let e = check_weapon_refs([("Ship_Mod", "Gun_Missing")], &guns, &bullets).unwrap_err();
        assert!(e.contains("Ship_Mod") && e.contains("Gun_Missing"), "{e}");
        let e = check_weapon_refs([("Ship_Mod", "Gun_A")], &guns, &bullets).unwrap_err();
        assert!(e.contains("Ship_Mod") && e.contains("Gun_A") && e.contains("Bullet_Torpedo"), "{e}");
        let bullets: HashMap<_, _> = [("Bullet_Torpedo".to_string(), bullet_from_value("Bullet_Torpedo", bullet()).unwrap())].into();
        assert!(check_weapon_refs([("Ship_Mod", "Gun_A")], &guns, &bullets).is_ok());
    }

    #[test]
    fn a_broken_weapons_file_is_reported_with_its_path_and_the_entry() {
        let dir = std::env::temp_dir().join(format!("procyon-weapons-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/weapons")).unwrap();
        std::fs::write(dir.join("data/weapons/bullets.json"), json!({"Bullet_Ok": bullet()}).to_string()).unwrap();
        std::fs::write(dir.join("data/weapons/guns.json"), "Gun_X: not json").unwrap();
        let e = try_load_weapons(&dir).err().unwrap();
        assert!(e.contains("guns.json") && e.contains("line 1"), "{e}");
        std::fs::write(dir.join("data/weapons/guns.json"), json!({"Gun_X": {"MuzzleSpeed": "fast"}}).to_string()).unwrap();
        let e = try_load_weapons(&dir).err().unwrap();
        assert!(e.contains("guns.json") && e.contains("Gun_X"), "{e}");
        std::fs::write(dir.join("data/weapons/guns.json"), json!({"Gun_X": gun()}).to_string()).unwrap();
        let (guns, bullets) = try_load_weapons(&dir).unwrap();
        assert!(guns.contains_key("Gun_X") && bullets.contains_key("Bullet_Ok"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
