//! Render-side interpolation of ship, lifeboat, asteroid and shell transforms between fixed battle ticks (docs/development.md
//! "Battle clock and scheduling"). The simulation is untouched: it keeps reading and writing `Transform` on the 33 ms tick.
//!
//! Each frame, right after the fixed loop and before `Update` and transform propagation, a tracked entity's `Transform` is
//! swapped for `previous.lerp(current, overstep_fraction)`. Right before the next fixed loop it is swapped back, so systems in
//! `FixedUpdate` always see the pose their own tick produced. Everything that runs in `Update` (orbit camera, wakes, picking,
//! HUD markers) and everything after propagation (flags, particles, sails) sees the interpolated pose.
use bevy::prelude::*;

/// Entities whose motion is simulated on the fixed tick and drawn between ticks. Lifeboats are ships.
type Tracked = Or<(With<crate::Ship>, With<crate::combat::Asteroid>, With<crate::combat::Shell>)>;

/// Interpolation state of one tracked entity.
#[derive(Component, Clone, Debug)]
pub struct Interp {
    /// The simulation pose at the start of the latest tick.
    prev: Transform,
    /// The simulation pose while an interpolated pose is drawn; `None` while `Transform` holds the simulation's.
    sim: Option<Transform>,
    /// What was written to `Transform` for drawing, to tell a later write by another system from our own.
    drawn: Transform,
}

impl Interp {
    pub fn new(at: Transform) -> Self {
        Interp { prev: at, sim: None, drawn: at }
    }
    /// The simulation's pose: the stored one while an interpolated pose is drawn, else `current` (the entity's `Transform`).
    pub fn sim_pose(&self, current: &Transform) -> Transform {
        self.sim.unwrap_or(*current)
    }
}

/// `previous.lerp(current, t)`: translation and scale linearly, rotation by slerp.
pub fn blend(previous: &Transform, current: &Transform, t: f32) -> Transform {
    Transform {
        translation: previous.translation.lerp(current.translation, t),
        rotation: previous.rotation.slerp(current.rotation, t),
        scale: previous.scale.lerp(current.scale, t),
    }
}

/// New tracked entities start still: drawn where they spawned, with no motion to blend from.
fn attach(mut commands: Commands, new: Query<(Entity, &Transform), (Tracked, Without<Interp>, Without<crate::combat::Sinking>)>) {
    for (e, tf) in &new {
        commands.entity(e).insert(Interp::new(*tf));
    }
}

/// Before the fixed loop: put the simulation's pose back. A `Transform` that something else wrote since we drew it
/// (a teleport from `Update`) is that system's new simulation pose; the entity snaps there instead of gliding.
fn restore(mut q: Query<(&mut Transform, &mut Interp)>) {
    for (mut tf, mut it) in &mut q {
        match it.sim.take() {
            Some(sim) if *tf == it.drawn => {
                tf.set_if_neq(sim);
            }
            Some(_) => it.prev = *tf,
            None => {}
        }
    }
}

/// First thing each tick: the pose the tick starts from, which the tick's motion then blends towards.
fn record(mut q: Query<(&Transform, &mut Interp)>) {
    for (tf, mut it) in &mut q {
        it.prev = *tf;
    }
}

/// After the fixed loop: draw between the last two simulation poses. While virtual time is paused no tick runs and the
/// overstep stays where it was, which holds the pose.
fn draw(fixed: Res<Time<Fixed>>, mut q: Query<(&mut Transform, &mut Interp), Without<crate::combat::Sinking>>) {
    let t = fixed.overstep_fraction();
    for (mut tf, mut it) in &mut q {
        let current = *tf;
        let pose = blend(&it.prev, &current, t);
        it.sim = Some(current);
        it.drawn = pose;
        tf.set_if_neq(pose);
    }
}

/// Start of every tick: `GlobalTransform`s under a tracked entity follow the simulation's pose. They are normally propagated once per
/// frame in `PostUpdate`, from the drawn pose, and gunnery reads a muzzle's `GlobalTransform` on the tick; without this a shell's
/// spawn point would depend on the frame rate. Only subtrees whose root moved since the last tick are walked; an unmoved root is
/// drawn at its simulation pose, so the frame's propagation already left its subtree right.
fn propagate_sim(
    mut roots: Query<(Ref<Transform>, &mut GlobalTransform, Option<&Children>), (With<Interp>, Without<ChildOf>)>,
    mut nodes: Query<(&Transform, &mut GlobalTransform, Option<&Children>), With<ChildOf>>,
    mut stack: Local<Vec<(Entity, GlobalTransform)>>,
) {
    for (tf, mut global, children) in &mut roots {
        if !tf.is_changed() {
            continue;
        }
        global.set_if_neq(GlobalTransform::from(*tf));
        stack.extend(children.into_iter().flatten().map(|c| (*c, *global)));
        while let Some((e, parent)) = stack.pop() {
            let Ok((local, mut global, children)) = nodes.get_mut(e) else { continue };
            global.set_if_neq(parent * GlobalTransform::from(*local));
            stack.extend(children.into_iter().flatten().map(|c| (*c, *global)));
        }
    }
}

pub fn plugin(app: &mut App) {
    use bevy::app::RunFixedMainLoopSystems::{AfterFixedMainLoop, BeforeFixedMainLoop};
    app.add_systems(RunFixedMainLoop, (attach, restore).chain().in_set(BeforeFixedMainLoop))
        .add_systems(RunFixedMainLoop, (attach, draw).chain().in_set(AfterFixedMainLoop))
        // Spawned during a tick: its first drawn pose is where it spawned, and the next tick records from there.
        .add_systems(FixedLast, attach)
        .add_systems(FixedPreUpdate, (record, propagate_sim));
}
