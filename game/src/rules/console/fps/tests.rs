use super::*;

fn steady(stats: &mut FrameStats, frames: usize, dt: f64, from: f64) -> f64 {
    let mut now = from;
    for _ in 0..frames {
        now += dt;
        stats.push(now, dt);
    }
    now
}

#[test]
fn a_steady_rate_reads_back_as_it_is() {
    let mut s = FrameStats::default();
    steady(&mut s, 120, 1.0 / 60.0, 0.0);
    assert!((s.fps() - 60.0).abs() < 0.01, "{}", s.fps());
    assert!((s.frame_ms() - 16.667).abs() < 0.01, "{}", s.frame_ms());
    assert!((s.worst_ms() - 16.667).abs() < 0.01);
}

#[test]
fn nothing_is_shown_before_a_frame() {
    let s = FrameStats::default();
    assert_eq!((s.fps(), s.frame_ms(), s.worst_ms()), (0.0, 0.0, 0.0));
}

#[test]
fn the_rate_follows_the_last_half_second_and_the_worst_frame_the_last_second() {
    let mut s = FrameStats::default();
    let now = steady(&mut s, 60, 1.0 / 60.0, 0.0);
    s.push(now + 0.1, 0.1); // one 100 ms hitch
    let now = now + 0.1;
    assert!((s.worst_ms() - 100.0).abs() < 0.01);
    assert!(s.fps() < 60.0 && s.fps() > 40.0, "the hitch lowers the smoothed rate: {}", s.fps());
    // Half a second of fast frames later the hitch has left the smoothed rate, but a second has not passed for the worst frame.
    let now = steady(&mut s, 90, 1.0 / 120.0, now);
    assert!((s.fps() - 120.0).abs() < 1.0, "{}", s.fps());
    assert!((s.worst_ms() - 100.0).abs() < 0.01, "still within the second");
    steady(&mut s, 120, 1.0 / 120.0, now);
    assert!(s.worst_ms() < 9.0, "a second later the hitch is forgotten: {}", s.worst_ms());
}

#[test]
fn old_samples_are_dropped() {
    let mut s = FrameStats::default();
    steady(&mut s, 5000, 1.0 / 100.0, 0.0);
    assert!(s.samples.len() <= 101, "{}", s.samples.len());
    s.clear();
    assert_eq!(s.fps(), 0.0);
}

#[test]
fn the_text_shows_rate_times_and_a_changed_game_speed() {
    let mut s = FrameStats::default();
    steady(&mut s, 120, 1.0 / 60.0, 0.0);
    assert_eq!(s.text(1.0), "60 fps  16.7 ms  worst 16.7 ms");
    assert!(s.text(2.0).ends_with("  x2.0"));
}
