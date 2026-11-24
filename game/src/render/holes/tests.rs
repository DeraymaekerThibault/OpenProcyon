use super::*;
use serde_json::json;

#[test]
fn absent_without_the_key_and_the_horizon_is_the_kill_core_by_default() {
    assert!(parse(&Value::Null).is_none());
    let b = parse(&json!({})).unwrap();
    let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
    assert_eq!(b.horizon, pull.core() / pull.radius, "the black area is the kill zone");
    assert!(!b.replace && parse(&json!({"replace": true})).unwrap().replace);
    assert!(b.disc.inner > b.horizon && b.disc.outer > b.disc.inner && b.disc.outer <= 1.0);
}

#[test]
fn values_are_clamped_and_disc_edges_keep_their_order() {
    let b = parse(&json!({"horizon": 3, "lensing": 99, "ring": -1, "disc": {"inner": 0.8, "outer": 0.1, "brightness": -2}})).unwrap();
    assert_eq!((b.horizon, b.lensing, b.ring, b.disc.brightness), (0.95, 4.0, 0.0, 0.0));
    assert!(b.disc.outer > b.disc.inner);
}

#[test]
fn sizes_scale_with_the_pull_radius() {
    let b = parse(&json!({})).unwrap();
    let small = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Small").unwrap(), 0.0);
    let big = b.uniform(Vec3::ZERO, &Pull::for_terrain("Terrain_BlackHole_Big").unwrap(), 0.0);
    assert_eq!(small.shape.x, small.shape.y, "the horizon is the core of a small hole");
    assert_eq!(big.shape.x, big.shape.y, "and of a big one");
    assert!(big.disc.y / small.disc.y > 4.0 && big.centre.w == 625.0);
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Mesh>()
        .init_asset::<HoleMaterial>()
        .init_asset::<Image>()
        .add_systems(Startup, crate::render::volnoise::bake)
        .insert_resource(Look::parse(&json!({"black_holes": {}})))
        .add_systems(Update, spawn_discs);
    app
}

#[test]
fn each_hole_gets_one_disc_that_follows_it_and_none_without_the_block() {
    let mut app = app();
    let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
    let hole = app.world_mut().spawn((Transform::from_xyz(50.0, 0.0, 0.0), GlobalTransform::default(), pull)).id();
    app.update();
    assert!(!app.world().entity(hole).contains::<HoleLook>(), "a hole is only a BlackHole with the retail component");
    app.world_mut().entity_mut(hole).insert(BlackHole::default());
    app.update();
    app.update();
    let discs = app.world_mut().query::<(&ChildOf, &HoleDisc)>().iter(app.world()).filter(|(p, _)| p.parent() == hole).count();
    assert_eq!(discs, 1, "once, not every frame");
    assert_eq!(app.world().resource::<Assets<HoleMaterial>>().len(), 1);
    let mut none = App::new();
    none.add_plugins(MinimalPlugins).insert_resource(Look::parse(&json!({})));
    super::plugin(&mut none, &Look::parse(&json!({})));
    assert!(none.world().get_resource::<Assets<HoleMaterial>>().is_none(), "classic never sees it");
}

#[test]
fn the_holes_own_ribbons_get_the_fx_gain() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<StandardMaterial>()
        .insert_resource(Look::parse(&json!({"black_holes": {}, "fx_gain": 1.6})))
        .add_systems(Update, ribbon_gain);
    let mat = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Add, unlit: true, ..default() });
    let other = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
    app.world_mut().spawn((BlackHole::default(), MeshMaterial3d(mat.clone())));
    app.world_mut().spawn(MeshMaterial3d(other.clone()));
    app.update();
    let mats = app.world().resource::<Assets<StandardMaterial>>();
    assert_eq!(mats.get(&mat).unwrap().base_color.to_linear().red, 1.6);
    assert_eq!(mats.get(&other).unwrap().base_color, Color::WHITE, "only the hole's ribbons");
}

#[test]
fn a_swallowed_ship_inside_the_radius_flashes_the_disc() {
    let mut app = app();
    app.add_systems(Update, (update_discs, flash_on_swallow).chain().after(spawn_discs));
    let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
    let hole = app.world_mut().spawn((Transform::default(), GlobalTransform::default(), pull, BlackHole::default())).id();
    app.update();
    app.world_mut().spawn((GlobalTransform::from_xyz(pull.radius * 0.5, 0.0, 0.0), Swallowed));
    app.world_mut().spawn((GlobalTransform::from_xyz(pull.radius * 3.0, 0.0, 0.0), Swallowed));
    app.update();
    let flash = app.world_mut().query::<(&ChildOf, &HoleDisc)>().iter(app.world()).find(|(p, _)| p.parent() == hole).map(|(_, d)| d.flash).unwrap();
    assert!(flash > 0.9, "{flash}");
}

#[test]
fn the_whirlpool_is_flat_at_the_rim_and_all_the_way_down_in_the_throat() {
    assert_eq!(funnel_drop(1.0, 0.06), 0.0);
    assert_eq!(funnel_drop(0.06, 0.06), 1.0);
    assert_eq!(funnel_drop(0.01, 0.06), 1.0);
    let (outer, core) = (funnel_drop(0.6, 0.06), funnel_drop(0.25, 0.06));
    assert!(outer < 0.05 && core > 3.0 * outer && core < 0.5, "nearly flat out where ships sail, plunging only near the core: {outer} {core}");
    let m = funnel_mesh(100.0, 1.2, 0.06);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
    assert!(p.iter().all(|q| q[1] <= 0.0 && q[1] >= -120.0 - 1e-3), "never above the plane, never below the throat");
    assert!(p.iter().map(|q| q[0].hypot(q[2])).fold(0.0, f32::max) <= 100.0 + 1e-3);
}

#[test]
fn the_sky_bends_around_the_bottom_of_the_throat_and_flat_holes_stay_in_the_plane() {
    let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
    let at = Vec3::new(10.0, 0.0, 20.0);
    assert_eq!(parse(&json!({"drop": 0.0})).unwrap().singularity(at, &pull), at);
    let deep = parse(&json!({"drop": 0.0, "funnel": {"depth": 1.0}})).unwrap().singularity(at, &pull);
    assert_eq!(deep, at - Vec3::Y * pull.radius);
    assert_eq!(parse(&json!({})).unwrap().shadow_core(&pull), pull.core());
    assert!(parse(&json!({"funnel": {"depth": 1.0}})).unwrap().shadow_core(&pull) < pull.core() * 0.5);
}

#[test]
fn the_ribbons_colour_comes_from_the_profile() {
    let pull = Pull::for_terrain("Terrain_BlackHole_Med").unwrap();
    let default = parse(&json!({})).unwrap().ribbon(Vec3::ZERO, &pull, 1.0);
    assert_eq!(default.color, Vec4::new(1.0, 0.38, 0.12, 0.0));
    let own = parse(&json!({"ribbon_color": [0.2, 0.5, 1.0]})).unwrap().ribbon(Vec3::ZERO, &pull, 1.0);
    assert_eq!(own.color, Vec4::new(0.2, 0.5, 1.0, 0.0));
}

#[test]
fn ribbons_default_to_full_brightness_and_are_clamped() {
    assert_eq!(parse(&json!({})).unwrap().ribbons, 1.0);
    assert_eq!(parse(&json!({"ribbons": 99})).unwrap().ribbons, 8.0);
}

fn ribbon_app(look: serde_json::Value) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<RibbonMaterial>()
        .insert_resource(Look::parse(&look))
        .add_systems(Update, (ribbon_gain, restyle_ribbons, update_ribbons).chain());
    let mat = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Add, unlit: true, ..default() });
    let pull = Pull::for_terrain("Terrain_BlackHole_Small").unwrap();
    let hole = app.world_mut().spawn((BlackHole::default(), pull, GlobalTransform::from_xyz(7.0, 0.0, 0.0), MeshMaterial3d(mat))).id();
    (app, hole)
}

#[test]
fn with_replace_the_vortex_strips_swap_to_the_thin_material_once_and_keep_their_mesh() {
    let (mut app, hole) = ribbon_app(json!({"black_holes": {"replace": true, "ribbons": 2}, "fx_gain": 1.5}));
    for _ in 0..3 {
        app.update();
    }
    let e = app.world().entity(hole);
    assert!(e.contains::<RibbonStyled>() && e.contains::<MeshMaterial3d<RibbonMaterial>>() && !e.contains::<MeshMaterial3d<StandardMaterial>>());
    assert!(e.contains::<BlackHole>(), "the hole and its trail sim are untouched");
    let mats = app.world().resource::<Assets<RibbonMaterial>>();
    assert_eq!(mats.len(), 1, "once");
    let u = mats.iter().next().unwrap().1.extension.u;
    assert_eq!((u.centre, u.shape.x, u.shape.y), (Vec4::new(7.0, 0.0, 0.0, 145.0), 145.0 / 4.0, 3.0), "the kill core and brightness times fx_gain");
}

#[test]
fn without_replace_the_strips_keep_the_retail_material_with_only_the_gain() {
    let (mut app, hole) = ribbon_app(json!({"black_holes": {}, "fx_gain": 1.5}));
    for _ in 0..3 {
        app.update();
    }
    let e = app.world().entity(hole);
    assert!(!e.contains::<RibbonStyled>() && e.contains::<MeshMaterial3d<StandardMaterial>>());
    let mat = e.get::<MeshMaterial3d<StandardMaterial>>().unwrap();
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().get(&mat.0).unwrap().base_color.to_linear().red, 1.5);
}
