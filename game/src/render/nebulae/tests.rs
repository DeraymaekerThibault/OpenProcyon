use super::*;
use serde_json::json;

#[test]
fn flash_light_fades_smoothly_with_distance_to_the_visible_bolt() {
    let mut bolt = Bolt { from: Vec3::X * -100.0, to: Vec3::X * 100.0, eye: Vec3::X * 150.0, left: 0.5, ..default() };
    assert_eq!(bolt.light(50.0), 0.25, "near the end of the bolt, not its midpoint");
    bolt.eye.x = 200.0;
    assert_eq!(bolt.light(50.0), 0.125, "halfway through the distance fade");
    bolt.eye.x = 250.0;
    assert_eq!(bolt.light(50.0), 0.0, "a distant flash contributes no light");
    let sheet = Bolt { eye: Vec3::X * 150.0, left: 1.0, ..default() };
    assert_eq!(sheet.light(50.0), 0.0, "a sheet flash is a point, with the same falloff");
}

#[test]
fn a_bolts_light_fades_over_its_seconds_from_the_frame_data() {
    let mut frame = Frame::default();
    assert_eq!(Bolt::of(&frame).left, 0.0, "none yet");
    frame.rows[super::super::frame::BOLT_AT] = Vec3::X.extend(0.0);
    assert_eq!(Bolt::of(&frame).left, 1.0, "struck just now");
    frame.rows[super::super::frame::BOLT_AT].w = FLASH_SECONDS * 0.5;
    assert_eq!((Bolt::of(&frame).at, Bolt::of(&frame).left), (Vec3::X, 0.5));
    frame.rows[super::super::frame::BOLT_AT].w = FLASH_SECONDS * 2.0;
    assert_eq!(Bolt::of(&frame).left, 0.0, "out");
}

#[test]
fn absent_without_the_key_and_a_rule_needs_its_match() {
    assert!(parse(&Value::Null).is_none());
    assert!(parse(&json!({})).unwrap().rules.is_empty());
    assert!(parse(&json!({"rules": [{"density": 2}]})).unwrap().rules.is_empty(), "a rule without `match` is dropped");
}

#[test]
fn defaults_and_clamping() {
    let n = parse(&json!({"rules": [{"match": "Nebula"}, {"match": "SolarStorm", "steps": 999, "density": -3, "inside": 5}]})).unwrap();
    assert_eq!((n.rules[0].steps, n.rules[0].density, n.rules[0].matching.as_str()), (16.0, 1.0, "nebula"));
    assert_eq!((n.rules[1].steps, n.rules[1].density, n.rules[1].inside), (MAX_STEPS, 0.0, 1.0));
}

#[test]
fn rules_match_by_effect_name_first_wins() {
    let n = parse(&json!({"rules": [{"match": "SolarStorm", "density": 4}, {"match": "Nebula", "density": 1}, {"match": "s", "density": 9}]})).unwrap();
    assert_eq!(n.find("SolarStorm").map(|r| r.density), Some(4.0));
    assert_eq!(n.find("Nebula").map(|r| r.density), Some(1.0));
    assert!(n.find("Meteor").is_none(), "no rule, the retail material stays");
}

#[test]
fn the_uniform_carries_the_boxes_axes_and_a_fading_bolt() {
    let rule = &parse(&json!({"rules": [{"match": "n", "steps": 20}]})).unwrap().rules[0];
    let bolt = Bolt { at: Vec3::new(1.0, 2.0, 3.0), left: 0.5, ..default() };
    let u = rule.uniform(Vec3::new(10.0, 0.0, 0.0), 300.0, 4.0, &bolt);
    assert_eq!((u.centre, u.axes), (Vec4::new(10.0, 0.0, 0.0, 300.0), Vec4::new(1.0, 0.7, 0.99, 20.0)));
    assert_eq!(u.flash, Vec4::new(1.0, 2.0, 3.0, 0.25));
    assert_eq!(rule.uniform(Vec3::ZERO, 1.0, 0.0, &Bolt::default()).flash.w, 0.0);
}

#[test]
fn a_cloud_swaps_its_material_once_keeping_its_mesh_and_unmatched_clouds_keep_theirs() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Mesh>()
        .init_asset::<EffectMaterial>()
        .init_asset::<NebulaMaterial>()
        .init_resource::<Frame>()
        .insert_resource(Look::parse(&json!({"nebulae": {"rules": [{"match": "Nebula"}]}})))
        .add_systems(Update, restyle);
    let (mesh, retail) = {
        let w = app.world_mut();
        (w.resource_mut::<Assets<Mesh>>().add(Mesh::from(Cuboid::default())), w.resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default())))
    };
    let (cloud, storm) = (Cloud::for_tests("Nebula", 100.0), Cloud::for_tests("MeteorStorm", 50.0));
    let a = app.world_mut().spawn((cloud, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
    let b = app.world_mut().spawn((storm, GlobalTransform::default(), Mesh3d(mesh.clone()), MeshMaterial3d(retail.clone()))).id();
    app.update();
    app.update();
    let w = app.world();
    assert!(w.entity(a).contains::<MeshMaterial3d<NebulaMaterial>>() && w.entity(a).contains::<Volumetric>() && !w.entity(a).contains::<MeshMaterial3d<EffectMaterial>>());
    assert_eq!(w.entity(a).get::<Mesh3d>().map(|m| m.0.id()), Some(mesh.id()), "the mesh, with its vertex colours, is kept");
    assert!(w.entity(b).contains::<MeshMaterial3d<EffectMaterial>>() && !w.entity(b).contains::<Volumetric>());
    assert_eq!(w.resource::<Assets<NebulaMaterial>>().len(), 1, "once");
}

#[test]
fn replace_is_per_rule_or_for_the_block_and_off_by_default() {
    assert!(!parse(&json!({"rules": [{"match": "n"}]})).unwrap().rules[0].replace);
    let n = parse(&json!({"replace": true, "rules": [{"match": "Nebula"}, {"match": "SolarStorm", "replace": false}]})).unwrap();
    assert_eq!((n.rules[0].replace, n.rules[1].replace), (true, false));
    assert!(!n.replaces_storms(), "the storm rule opted out");
    assert!(parse(&json!({"rules": [{"match": "SolarStorm", "replace": true}]})).unwrap().replaces_storms());
}

#[test]
fn a_replacing_rule_hides_the_shell_and_keeps_the_cloud_and_its_material() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Mesh>()
        .init_asset::<EffectMaterial>()
        .init_asset::<NebulaMaterial>()
        .init_resource::<Frame>()
        .insert_resource(Look::parse(&json!({"nebulae": {"rules": [{"match": "Nebula", "replace": true}, {"match": "Meteor"}]}})))
        .add_systems(Update, (hide_shells, restyle).chain());
    let retail = app.world_mut().resource_mut::<Assets<EffectMaterial>>().add(crate::fx::additive_material(Handle::default()));
    let mut spawn = |effect: &str| app.world_mut().spawn((Cloud::for_tests(effect, 100.0), GlobalTransform::default(), Visibility::default(), MeshMaterial3d(retail.clone()))).id();
    let (replaced, plain) = (spawn("Nebula"), spawn("MeteorStorm"));
    app.update();
    let w = app.world();
    assert_eq!(w.entity(replaced).get::<Visibility>(), Some(&Visibility::Hidden));
    assert!(w.entity(replaced).contains::<Cloud>() && w.entity(replaced).contains::<MeshMaterial3d<EffectMaterial>>(), "nothing is removed from it");
    assert_eq!(w.entity(plain).get::<Visibility>(), Some(&Visibility::Inherited));
    assert!(w.entity(plain).contains::<Volumetric>(), "a rule without replace still restyles the shell");
}

fn volume_app(look: serde_json::Value) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Shader>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<VolumeMaterial>()
        .init_resource::<Frame>()
        .insert_resource(Look::parse(&look))
        .add_systems(Startup, crate::render::volnoise::bake)
        .add_systems(Update, (spawn_volumes, update_volumes).chain());
    app
}

#[test]
fn volume_keys_have_defaults_and_clamp() {
    let r =
        &parse(&json!({"rules": [{"match": "n"}, {"match": "s", "absorption": 99, "dust": -1, "max_opacity": 5, "light_steps": 9, "sun": 50, "core_glow": 300}]})).unwrap().rules;
    assert_eq!((r[0].absorption, r[0].dust, r[0].max_opacity, r[0].light_steps, r[0].sun), (3.0, 0.6, 0.6, 2.0, 1.0));
    assert_eq!((r[1].absorption, r[1].dust, r[1].max_opacity, r[1].light_steps, r[1].sun), (20.0, 0.0, 1.0, 3.0, 4.0));
    assert_eq!((r[0].core_glow, r[1].core_glow), (0.0, 12.0), "older profiles get no extra core emission");
}

#[test]
fn a_replaced_cloud_gets_one_back_face_proxy_that_follows_it_and_others_get_none() {
    let mut app = volume_app(json!({"nebulae": {"rules": [{"match": "Nebula", "replace": true, "steps": 20}, {"match": "MeteorStorm"}]}}));
    let spawn = |app: &mut App, effect: &str, x: f32| app.world_mut().spawn((Cloud::for_tests(effect, 100.0), GlobalTransform::from_xyz(x, 0.0, 0.0), Visibility::Hidden)).id();
    let (cloud, plain) = (spawn(&mut app, "Nebula", 10.0), spawn(&mut app, "MeteorStorm", 0.0));
    app.update();
    app.update();
    app.update();
    let proxies = app
        .world_mut()
        .query::<(&ChildOf, &VolumeProxy, &MeshMaterial3d<VolumeMaterial>, &Visibility, &Transform)>()
        .iter(app.world())
        .map(|(p, _, m, v, t)| (p.parent(), m.0.clone(), *v, *t))
        .collect::<Vec<_>>();
    assert_eq!(proxies.len(), 1, "one proxy, once");
    let (parent, mat, vis, tf) = &proxies[0];
    assert_eq!(*parent, cloud);
    assert_eq!(*vis, Visibility::Visible, "drawn although the shell is hidden");
    assert_eq!(tf.scale, AXES * 2.0 * 100.0 * BOX_MARGIN);
    assert!(!app.world().entity(plain).contains::<VolumeProxyOf>());
    let m = app.world().resource::<Assets<VolumeMaterial>>().get(mat).unwrap();
    assert_eq!(m.base.cull_mode, Some(bevy::render::render_resource::Face::Front), "back faces only, so it draws from inside");
    assert_eq!(m.base.alpha_mode, AlphaMode::Premultiplied);
    assert_eq!((m.extension.u.centre, m.extension.u.axes.w), (Vec4::new(10.0, 0.0, 0.0, 100.0), 20.0));
    assert!(m.extension.noise != Handle::default(), "the baked noise is bound");
}

#[test]
fn the_volume_follows_the_retail_breath_the_sun_and_the_bolt() {
    let rule = &parse(&json!({"rules": [{"match": "n", "steps": 12, "max_opacity": 0.5, "sun": 2, "core_glow": 6}]})).unwrap().rules[0];
    let cloud = Cloud::for_tests("Nebula", 200.0);
    let sun = SunLight { toward: Vec3::new(0.0, 3.0, 0.0), color: Vec3::new(2.0, 1.0, 1.0) };
    let u = rule.volume(&cloud, Vec3::new(1.0, 2.0, 3.0), 5.0, &Bolt { at: Vec3::X, from: Vec3::X, to: Vec3::X * 3.0, left: 0.5, ..default() }, sun);
    assert_eq!(u.axes, (AXES * cloud.breath()).extend(12.0));
    assert_eq!((u.sun, u.sun_color.truncate()), (Vec4::new(0.0, 1.0, 0.0, 2.0), Vec3::new(1.0, 0.5, 0.5)), "a unit direction and a colour normalised to its brightest channel");
    assert_eq!((u.low.w, u.flash.w, u.params.x, u.params.w), (0.5, 0.25, 5.0, 1.0));
    assert_eq!(u.alt, Vec3::ONE.extend(6.0), "core emission reaches the shader independently of opacity, flash and hue");
    assert_eq!((u.flash.truncate(), u.flash_to.truncate()), (Vec3::X, Vec3::X * 3.0), "the volume is lit along the whole bolt");
}
