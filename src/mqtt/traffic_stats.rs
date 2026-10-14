//! Process-lifetime MQTT payload byte counters + short-window throughput.
//!
//! Counts publish **payload** sizes only (not MQTT headers / topic strings).
//! Scope: app process lifetime (not reset on connect/disconnect).

use std::collections::VecDeque;
use std::time::Instant;

/// Sliding window for throughput (~2s of samples).
const WINDOW_SECS: f64 = 2.0;
/// Cap samples so idle ticks cannot grow unboundedly.
const WINDOW_CAP: usize = 240;

/// Seconds used for ribbon projections from the current rate.
pub const SECS_PER_HOUR: f64 = 3_600.0;
pub const SECS_PER_DAY: f64 = 86_400.0;
/// Calendar-agnostic month ≈ 30 days.
pub const SECS_PER_MONTH: f64 = 2_592_000.0;

/// Live traffic totals + rate for the Stats ribbon.
#[derive(Debug)]
pub struct TrafficStats {
    pub bytes_recv: u64,
    pub bytes_sent: u64,
    /// `(Instant, cumulative recv+sent)` samples for the sliding window.
    samples: VecDeque<(Instant, u64)>,
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self {
            bytes_recv: 0,
            bytes_sent: 0,
            samples: VecDeque::new(),
        }
    }
}

impl TrafficStats {
    /// Inbound MQTT publish payload bytes (raw wire size before soft-limit placeholder).
    pub fn record_recv(&mut self, n: u64) {
        self.bytes_recv = self.bytes_recv.saturating_add(n);
        self.push_sample();
    }

    /// Outbound MQTT publish payload bytes.
    pub fn record_sent(&mut self, n: u64) {
        self.bytes_sent = self.bytes_sent.saturating_add(n);
        self.push_sample();
    }

    /// Append a sample + prune (call each UI frame so idle throughput decays to 0).
    pub fn tick(&mut self) {
        self.push_sample();
    }

    /// True while throughput is non-zero (light repaint so Thru / projections move).
    pub fn needs_repaint(&self) -> bool {
        self.throughput_bps() > 0.0
    }

    fn total_bytes(&self) -> u64 {
        self.bytes_recv.saturating_add(self.bytes_sent)
    }

    fn push_sample(&mut self) {
        let now = Instant::now();
        self.samples.push_back((now, self.total_bytes()));
        while self.samples.len() > WINDOW_CAP {
            self.samples.pop_front();
        }
        while let Some((t, _)) = self.samples.front() {
            if now.duration_since(*t).as_secs_f64() > WINDOW_SECS {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Combined in+out bytes/sec over the sliding window (0 when idle / too little data).
    pub fn throughput_bps(&self) -> f64 {
        let (Some((t0, b0)), Some((t1, b1))) = (self.samples.front(), self.samples.back()) else {
            return 0.0;
        };
        let dt = t1.duration_since(*t0).as_secs_f64();
        if dt < 0.05 {
            return 0.0;
        }
        let delta = b1.saturating_sub(*b0) as f64;
        delta / dt
    }

    /// Project volume if the current rate held for `seconds` (hour/day/month).
    pub fn project_bytes(&self, seconds: f64) -> u64 {
        let rate = self.throughput_bps();
        if !rate.is_finite() || rate <= 0.0 || seconds <= 0.0 {
            return 0;
        }
        (rate * seconds).round() as u64
    }
}

/// Human units: B → KB → MB; GB only once ≥ 10 GB.
pub fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TEN_GB: u64 = 10 * 1024 * 1024 * 1024;

    if n >= TEN_GB {
        format!("{:.2} GB", n as f64 / GB)
    } else if (n as f64) >= MB {
        format!("{:.2} MB", n as f64 / MB)
    } else if (n as f64) >= KB {
        format!("{:.1} KB", n as f64 / KB)
    } else {
        format!("{n} B")
    }
}

/// Same scaling as [`format_bytes`], with a `/s` suffix for throughput.
pub fn format_rate(bps: f64) -> String {
    let n = if bps.is_finite() && bps > 0.0 {
        bps.round().max(0.0) as u64
    } else {
        0
    };
    format!("{}/s", format_bytes(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_stays_on_mb_below_10_gb() {
        let nine_gb = 9 * 1024 * 1024 * 1024;
        let s = format_bytes(nine_gb);
        assert!(s.ends_with(" MB"), "got {s}");
        let ten_gb = 10 * 1024 * 1024 * 1024;
        let s2 = format_bytes(ten_gb);
        assert!(s2.ends_with(" GB"), "got {s2}");
    }

    #[test]
    fn format_small_is_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
    }

    #[test]
    fn project_from_rate() {
        let mut s = TrafficStats::default();
        // Simulate ~1000 B/s by recording and faking window ends.
        s.bytes_recv = 2000;
        s.samples.clear();
        let t0 = Instant::now() - std::time::Duration::from_secs(2);
        s.samples.push_back((t0, 0));
        s.samples.push_back((Instant::now(), 2000));
        let rate = s.throughput_bps();
        assert!((rate - 1000.0).abs() < 50.0, "rate={rate}");
        assert_eq!(s.project_bytes(SECS_PER_HOUR), (rate * SECS_PER_HOUR).round() as u64);
    }
}
