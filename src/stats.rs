//! Performance numbers, so a change to the picture's path is judged by
//! measurement. Logged under `mote::stats` (`MOTE_LOG=mote::stats=info`)
//! every few seconds and when the session ends; without that, nothing is
//! timed and nothing is logged.

use std::time::{Duration, Instant};

const EVERY: Duration = Duration::from_secs(5);

/// Whether anyone is listening; checked once, so the hot path pays nothing.
pub(crate) fn enabled() -> bool {
    tracing::enabled!(target: "mote::stats", tracing::Level::INFO)
}

/// Durations of one step of the path, between reports.
#[derive(Default)]
struct Series {
    samples: Vec<Duration>,
}

impl Series {
    fn add(&mut self, duration: Duration) {
        self.samples.push(duration);
    }

    /// `mean/p95/max` in milliseconds, emptying the series.
    fn take(&mut self) -> Option<String> {
        if self.samples.is_empty() {
            return None;
        }
        let samples = &mut self.samples;
        samples.sort_unstable();
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let mean = samples.iter().map(|d| ms(*d)).sum::<f64>() / samples.len() as f64;
        let p95 = ms(samples[(samples.len() * 95 / 100).min(samples.len() - 1)]);
        let max = ms(samples[samples.len() - 1]);
        samples.clear();
        Some(format!("{mean:.2}/{p95:.2}/{max:.2}"))
    }
}

/// Named series and counters for one side of a session, reported together.
pub(crate) struct Stats {
    side: &'static str,
    series: Vec<(&'static str, Series)>,
    counts: Vec<(&'static str, u64)>,
    since: Instant,
    on: bool,
}

impl Stats {
    pub(crate) fn new(side: &'static str) -> Self {
        Self {
            side,
            series: Vec::new(),
            counts: Vec::new(),
            since: Instant::now(),
            on: enabled(),
        }
    }

    /// The time since `start`, under `name`.
    pub(crate) fn time(&mut self, name: &'static str, start: Instant) {
        if self.on {
            self.duration(name, start.elapsed());
        }
    }

    pub(crate) fn duration(&mut self, name: &'static str, duration: Duration) {
        if !self.on {
            return;
        }
        match self.series.iter_mut().find(|(n, _)| *n == name) {
            Some((_, series)) => series.add(duration),
            None => {
                let mut series = Series::default();
                series.add(duration);
                self.series.push((name, series));
            }
        }
        self.maybe_report();
    }

    pub(crate) fn count(&mut self, name: &'static str, by: u64) {
        if !self.on {
            return;
        }
        match self.counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += by,
            None => self.counts.push((name, by)),
        }
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.since.elapsed() >= EVERY {
            self.report();
        }
    }

    /// Log what was gathered since the last report: counters per second,
    /// durations as mean/p95/max in milliseconds.
    pub(crate) fn report(&mut self) {
        if !self.on {
            return;
        }
        let seconds = self.since.elapsed().as_secs_f64().max(0.001);
        self.since = Instant::now();
        let mut line = Vec::new();
        for (name, count) in &mut self.counts {
            line.push(format!("{name} {:.1}/s", *count as f64 / seconds));
            *count = 0;
        }
        for (name, series) in &mut self.series {
            if let Some(text) = series.take() {
                line.push(format!("{name} {text} ms"));
            }
        }
        if !line.is_empty() {
            tracing::info!(target: "mote::stats", "{} over {seconds:.1} s: {}", self.side, line.join(", "));
        }
    }
}

impl Drop for Stats {
    fn drop(&mut self) {
        self.report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_reports_mean_p95_and_max() {
        let mut series = Series::default();
        for ms in 1..=100 {
            series.add(Duration::from_millis(ms));
        }
        assert_eq!(series.take().as_deref(), Some("50.50/96.00/100.00"));
        assert_eq!(series.take(), None);
    }
}
