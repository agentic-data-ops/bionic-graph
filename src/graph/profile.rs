//! Simple batch profiling — accumulates elapsed time per category
//! and logs a summary at the end of a batch operation.
//!
//! Uses a thread-local accumulator so individual functions can record
//! timing without changing any function signatures.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

thread_local! {
    static PROFILE: RefCell<Option<BatchProfile>> = const { RefCell::new(None) };
}

/// Begin a profiling session on the current thread.
pub fn begin_batch(label: String) {
    PROFILE.with(|p| {
        *p.borrow_mut() = Some(BatchProfile::new(label));
    });
}

/// Record elapsed time under a named category.
pub fn record(label: &'static str, elapsed: Duration) {
    PROFILE.with(|p| {
        if let Some(ref mut prof) = *p.borrow_mut() {
            prof.add(label, elapsed);
        }
    });
}

/// Time a closure and record it.
pub fn time<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let t0 = Instant::now();
    let result = f();
    let elapsed = t0.elapsed();
    record(label, elapsed);
    result
}

/// Log the summary and drop the session.
pub fn end_batch() {
    PROFILE.with(|p| {
        if let Some(prof) = p.borrow_mut().take() {
            prof.log_summary();
        }
    });
}

/// Accumulates wall-clock time per named category.
struct BatchProfile {
    label: String,
    labels: BTreeMap<&'static str, (Duration, usize)>,
    t_start: Instant,
}

impl BatchProfile {
    fn new(label: String) -> Self {
        Self {
            label,
            labels: BTreeMap::new(),
            t_start: Instant::now(),
        }
    }

    fn add(&mut self, label: &'static str, elapsed: Duration) {
        let entry = self.labels.entry(label).or_insert((Duration::ZERO, 0));
        entry.0 += elapsed;
        entry.1 += 1;
    }

    fn log_summary(&self) {
        let total = self.t_start.elapsed();
        let mut lines = vec![
            format!("[Profiler] {} — total {:.3}s", self.label, total.as_secs_f64()),
            format!("{:-<1$}", "", 78),
        ];
        for (label, (cumul, count)) in &self.labels {
            let avg = if *count > 0 {
                cumul.as_secs_f64() / *count as f64
            } else {
                0.0
            };
            lines.push(format!(
                "  {:50} {:>8.3}s total  {:>6} calls  {:>9.6}s avg",
                label,
                cumul.as_secs_f64(),
                count,
                avg,
            ));
        }
        lines.push(format!("{:-<1$}", "", 78));
        let accounted: Duration = self.labels.values().map(|(d, _)| *d).sum();
        lines.push(format!(
            "  {:50} {:>8.3}s total",
            "UNACCOUNTED",
            total.saturating_sub(accounted).as_secs_f64()
        ));
        for line in &lines {
            log::info!("{}", line);
        }
    }
}
