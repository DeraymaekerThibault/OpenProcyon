//! `PROCYON_PROFILE`: frame-time and entity-count logging for performance runs.

use crate::*;

/// Whether `PROCYON_PROFILE` is set (frame-time logging, and vsync off so the display does not cap the measurement).
pub(crate) fn profiling() -> bool {
    std::env::var_os("PROCYON_PROFILE").is_some()
}

/// 99th-percentile (nearest rank) of per-frame times; 0 for no samples. Sorts the slice.
fn p99(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    match samples.len() {
        0 => 0.0,
        n => samples[(n * 99).div_ceil(100) - 1],
    }
}

pub(crate) fn profile_counts(
    time: Res<Time<Real>>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mission: Option<Res<campaign::Mission>>,
    entities: Query<Entity>,
    ships: Query<&Ship>,
    rocks: Query<&combat::Asteroid>,
    effects: Query<&fx::Fx>,
    meshes: Res<Assets<Mesh>>,
    audio: Query<&AudioPlayer>,
    point_lights: Query<&PointLight>,
    spot_lights: Query<&SpotLight>,
    draws: Query<&Mesh3d>,
    standard_materials: Res<Assets<StandardMaterial>>,
    effect_materials: Res<Assets<fx::EffectMaterial>>,
    mut last: Local<(f32, bool)>,
    mut frame_samples: Local<Vec<f64>>,
) {
    // One sample per rendered frame; the p99 covers the frames since the previous log line.
    if let Some(sample) = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.value()) {
        frame_samples.push(sample);
    }
    let (mission_time, objective, ended) = mission.as_ref().map_or((0.0, None, false), |m| (m.script.time, m.script.point, m.script.ended.is_some()));
    if time.elapsed_secs() - last.0 < 5.0 && ended == last.1 {
        return;
    }
    *last = (time.elapsed_secs(), ended);
    let frame_ms = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.average()).unwrap_or(0.0);
    let p99_ms = p99(&mut frame_samples);
    frame_samples.clear();
    let particles: usize = effects.iter().map(fx::Fx::particle_count).sum();
    info!(
        "profile real={:.1} mission={mission_time:.1} objective={objective:?} ended={ended} frame_ms={frame_ms:.2} entities={} ships={} rocks={} effects={} particles={particles} mesh_assets={} audio={} p99_ms={p99_ms:.2} lights={} draw_entities={} materials={}",
        time.elapsed_secs(),
        entities.iter().count(),
        ships.iter().count(),
        rocks.iter().count(),
        effects.iter().count(),
        meshes.len(),
        audio.iter().count(),
        point_lights.iter().count() + spot_lights.iter().count(),
        draws.iter().count(),
        standard_materials.len() + effect_materials.len()
    );
    // GPU milliseconds per render pass (rolling average), the passes above 0.2 ms, slowest first: where a slow frame goes.
    let mut gpu: Vec<(f64, String)> = diagnostics
        .iter()
        .filter_map(|d| Some((d.average()?, d.path().as_str().strip_prefix("render/")?.strip_suffix("/elapsed_gpu")?.to_string())))
        .filter(|(ms, _)| *ms > 0.2)
        .collect();
    gpu.sort_by(|a, b| b.0.total_cmp(&a.0));
    info!("profile gpu_ms {}", gpu.iter().map(|(ms, pass)| format!("{pass}={ms:.2}")).collect::<Vec<_>>().join(" "));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn p99_is_the_nearest_rank_sample() {
        assert_eq!(p99(&mut []), 0.0);
        assert_eq!(p99(&mut [7.0]), 7.0);
        let mut hundred: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        assert_eq!(p99(&mut hundred), 99.0, "one outlier in a hundred frames is excluded");
        let mut stutter = vec![16.0; 99];
        stutter.push(250.0);
        assert_eq!(p99(&mut stutter), 16.0);
        stutter.push(250.0);
        assert_eq!(p99(&mut stutter), 250.0, "two slow frames in 101 are the tail");
    }
}
