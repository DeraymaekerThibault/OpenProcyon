//! The light a map authors and its application to the battle's sun and ambient.
use super::*;

/// The light a map authors (map.json `lighting`): the roof light's colour and travel direction (Bevy frame) and the floor light's colour.
/// PROVISIONAL: the original blends an ambient colour with floor/roof hemisphere lights; the remake uses the roof light as the sun and
/// the floor colour as the uniform ambient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub direction: Vec3,
    pub sun: Color,
    pub ambient: Color,
}
#[derive(Resource, Default)]
pub struct MapLighting(pub Option<Lighting>);

pub fn lighting_of(map: &serde_json::Value) -> Option<Lighting> {
    let l = map.get("lighting")?;
    let colour = |v: &serde_json::Value| {
        let c: Vec<f32> = v.as_array()?.iter().filter_map(|x| x.as_f64().map(|x| x as f32)).collect();
        (c.len() >= 3).then(|| Color::srgb(c[0], c[1], c[2]))
    };
    let direction = game_to_bevy(&l["roof_direction"]);
    Some(Lighting { direction: direction.try_normalize()?, sun: colour(&l["roof"])?, ambient: colour(&l["floor"])? })
}

/// Apply the map's light, or the default white light from above when it has none.
pub fn apply_lighting(light: MapLighting_, mut ambient: ResMut<GlobalAmbientLight>, map: Res<MapLighting>) {
    let (mut sun, mut tf) = light.into_inner();
    match map.0 {
        Some(l) => {
            sun.color = l.sun;
            *tf = Transform::IDENTITY.looking_to(l.direction, Vec3::Y);
            ambient.color = l.ambient;
        }
        None => {
            sun.color = Color::WHITE;
            *tf = Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y);
            ambient.color = Color::WHITE;
        }
    }
}
type MapLighting_<'w, 's> = Single<'w, 's, (&'static mut DirectionalLight, &'static mut Transform)>;
