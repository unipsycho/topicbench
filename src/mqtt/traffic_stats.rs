//! Process-lifetime MQTT payload byte counters + short-window throughput.
//!
//! Counts publish **payload** sizes only (not MQTT headers / topic strings).
//! Scope: app process lifetime (not reset on connect/disconnect).
//!
//! Ribbon labels refresh at **1 Hz** via [`TrafficStats::refresh_display_if_due`];
//! the throughput window itself stays ~2s.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Sliding window for throughput (~2s of samples).
const WINDOW_SECS: f64 = 2.0;
/// Cap samples so idle ticks cannot grow unboundedly.
const WINDOW_CAP: usize = 240;
/// How often ribbon Recv/Sent/Thru / projections reformat.
const DISPLAY_INTERVAL: Duration = Duration::from_secs(1);

/// Seconds used for ribbon projections from the current rate.
pub const SECS_PER_HOUR: f64 = 3_600.0;
pub const SECS_PER_DAY: f64 = 86_400.0;
/// Calendar-agnostic month ≈ 30 days.
pub const SECS_PER_MONTH: f64 = 2_592_000.0;

/// Frozen ribbon strings (updated at most once per second).
#[derive(Debug, Clone)]
pub struct StatsDisplay {
    pub recv: String,
    pub sent: String,
    pub thru: String,
    pub per_h: String,
    pub per_d: String,
    pub per_mo: String,
}

impl Default for StatsDisplay {
    fn default() -> Self {
        Self {
            recv: "0 B".into(),
            sent: "0 B".into(),
            thru: "0 B/s".into(),
            per_h: "0 B".into(),
            per_d: "0 B".into(),
            per_mo: "0 B".into(),
        }
    }
}

/// Live traffic totals + rate for the Stats ribbon.
#[derive(Debug)]
pub struct TrafficStats {
    pub bytes_recv: u64,
    pub bytes_sent: u64,
    /// `(Instant, cumulative recv+sent)` samples for the sliding window.
    samples: VecDeque<(Instant, u64)>,
    /// Last time [`Self::display`] was rebuilt.
    last_display: Instant,
    display: StatsDisplay,
}

impl Default for TrafficStats {
    fn default() -> Self {
        // Force an immediate first snapshot on the first UI tick.
        let last_display = Instant::now()
            .checked_sub(DISPLAY_INTERVAL)
            .unwrap_or_else(Instant::now);
        Self {
            bytes_recv: 0,
            bytes_sent: 0,
            samples: VecDeque::new(),
            last_display,
            display: StatsDisplay::default(),
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

    /// Frozen ribbon labels (Recv / Sent / Thru / projections).
    pub fn display(&self) -> &StatsDisplay {
        &self.display
    }

    /// Rebuild ribbon strings at most once per second. Returns true when updated.
    pub fn refresh_display_if_due(&mut self) -> bool {
        if self.last_display.elapsed() < DISPLAY_INTERVAL {
            return false;
        }
        // Sample at display cadence so idle throughput decays without per-frame ticks.
        self.push_sample();
        let rate = self.throughput_bps();
        self.display = StatsDisplay {
            recv: format_bytes(self.bytes_recv),
            sent: format_bytes(self.bytes_sent),
            thru: format_rate(rate),
            per_h: format_bytes(project_from_rate(rate, SECS_PER_HOUR)),
            per_d: format_bytes(project_from_rate(rate, SECS_PER_DAY)),
            per_mo: format_bytes(project_from_rate(rate, SECS_PER_MONTH)),
        };
        self.last_display = Instant::now();
        true
    }

    /// Schedule the next 1 Hz Stats refresh (even while idle, so Thru → 0).
    pub fn wants_periodic_refresh(&self) -> bool {
        self.bytes_recv > 0
            || self.bytes_sent > 0
            || self.samples.len() >= 2
            || self.throughput_bps() > 0.0
    }

    /// Delay until the next display refresh is due (for `request_repaint_after`).
    pub fn time_until_display_refresh(&self) -> Duration {
        DISPLAY_INTERVAL.saturating_sub(self.last_display.elapsed())
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
        project_from_rate(self.throughput_bps(), seconds)
    }
}

fn project_from_rate(rate: f64, seconds: f64) -> u64 {
    if !rate.is_finite() || rate <= 0.0 || seconds <= 0.0 {
        return 0;
    }
    (rate * seconds).round() as u64
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
    fn project_from_rate_window() {
        let mut s = TrafficStats::default();
        s.bytes_recv = 2000;
        s.samples.clear();
        let t0 = Instant::now() - std::time::Duration::from_secs(2);
        s.samples.push_back((t0, 0));
        s.samples.push_back((Instant::now(), 2000));
        let rate = s.throughput_bps();
        assert!((rate - 1000.0).abs() < 50.0, "rate={rate}");
        assert_eq!(s.project_bytes(SECS_PER_HOUR), (rate * SECS_PER_HOUR).round() as u64);
    }

    #[test]
    fn display_refreshes_at_most_once_per_second() {
        let mut s = TrafficStats::default();
        assert!(s.refresh_display_if_due());
        s.record_recv(100);
        // Immediate second call must not reformat yet.
        assert!(!s.refresh_display_if_due());
        assert_eq!(s.display().recv, "0 B");
        // Force due by rewinding the clock.
        s.last_display = Instant::now()
            .checked_sub(DISPLAY_INTERVAL)
            .unwrap_or_else(Instant::now);
        assert!(s.refresh_display_if_due());
        assert_eq!(s.display().recv, "100 B");
    }
}
