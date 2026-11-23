//! `--frame-stats`: logs frame count and timings once a second. It requests a frame on every frame,
//! so it measures the worst case, not idle.
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::Window;

struct Stats {
    last: Instant,
    window_start: Instant,
    frames: u32,
    worst: Duration,
}

/// Start logging frame stats for `window`.
pub fn start(window: &mut Window) {
    let now = Instant::now();
    let stats = Rc::new(RefCell::new(Stats {
        last: now,
        window_start: now,
        frames: 0,
        worst: Duration::ZERO,
    }));
    tick(stats, window);
}

fn tick(stats: Rc<RefCell<Stats>>, window: &mut Window) {
    window.on_next_frame(move |window, _| {
        {
            let mut s = stats.borrow_mut();
            let now = Instant::now();
            s.worst = s.worst.max(now - s.last);
            s.last = now;
            s.frames += 1;
            if now - s.window_start >= Duration::from_secs(1) {
                let secs = (now - s.window_start).as_secs_f32();
                log::info!(
                    "ytdlpd: frame-stats {:.0} fps, worst frame {:.1} ms",
                    s.frames as f32 / secs,
                    s.worst.as_secs_f32() * 1000.
                );
                (s.window_start, s.frames, s.worst) = (now, 0, Duration::ZERO);
            }
        }
        tick(stats, window);
    });
    window.refresh();
}
