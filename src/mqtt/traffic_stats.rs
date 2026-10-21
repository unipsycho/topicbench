//! Process-lifetime MQTT payload byte counters + smoothed throughput.
//!
//! Counts publish **payload** sizes only (not MQTT headers / topic strings).
//! Scope: app process lifetime (not reset on connect/disconnect).
//!
//! Ribbon labels refresh at **1 Hz** via [`TrafficStats::refresh_display_if_due`].
//! Thru + `/h` `/d` `/mo` use an **EMA** of combined in+out rate
//! (`τ ≈ 20s`) so bursts do not whip the numbers around every second.

use std::time::{Duration, Instant};

/// EMA time constant for Thru / projections (seconds).
/// ~20s damps burstiness without feeling stuck (mid of the 15–30s band).
pub const EMA_TAU_SECS: f64 = 20.0;
/// How often ribbon Recv/Sent/Thru / projections reformat.
const DISPLAY_INTERVAL: Duration = Duration::from_secs(1);

/// Seconds used for ribbon projections from the smoothed rate.
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

/// Live traffic totals + smoothed rate for the Stats ribbon.
#[derive(Debug)]
pub struct TrafficStats {
    pub bytes_recv: u64,
    pub bytes_sent: u64,
    /// Inbound MQTT publish count (process lifetime).
    pub msgs_recv: u64,
    /// Last sample used to derive an instantaneous rate for the EMA.
    last_sample_at: Instant,
    last_sample_total: u64,
    last_sample_msgs: u64,
    /// Exponential moving average of combined in+out bytes/sec.
    ema_bps: f64,
    /// EMA of inbound messages/sec (status strip msgs/min).
    ema_mps: f64,
    ema_ready: bool,
    /// Last time [`Self::display`] was rebuilt.
    last_display: Instant,
    display: StatsDisplay,
}

impl Default for TrafficStats {
    fn default() -> Self {
        let now = Instant::now();
        // Force an immediate first snapshot on the first UI tick.
        let last_display = now
            .checked_sub(DISPLAY_INTERVAL)
            .unwrap_or(now);
        Self {
            bytes_recv: 0,
            bytes_sent: 0,
            msgs_recv: 0,
            last_sample_at: now,
            last_sample_total: 0,
            last_sample_msgs: 0,
            ema_bps: 0.0,
            ema_mps: 0.0,
            ema_ready: false,
            last_display,
            display: StatsDisplay::default(),
        }
    }
}

impl TrafficStats {
    /// Inbound MQTT publish payload bytes (raw wire size before soft-limit placeholder).
    pub fn record_recv(&mut self, n: u64) {
        self.bytes_recv = self.bytes_recv.saturating_add(n);
    }

    /// Outbound MQTT publish payload bytes.
    pub fn record_sent(&mut self, n: u64) {
        self.bytes_sent = self.bytes_sent.saturating_add(n);
    }

    /// Inbound MQTT publish count (coalesced batches may be > 1).
    pub fn record_msgs(&mut self, n: u64) {
        self.msgs_recv = self.msgs_recv.saturating_add(n);
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
        self.update_ema();
        let rate = self.throughput_bps();
        self.display = StatsDisplay {
            recv: format_bytes(self.bytes_recv),
            sent: format_bytes(self.bytes_sent),
            thru: format_rate(rate),
            // Projections: 1 decimal on KB/MB/GB from the same EMA rate.
            per_h: format_bytes_1dp(project_from_rate(rate, SECS_PER_HOUR)),
            per_d: format_bytes_1dp(project_from_rate(rate, SECS_PER_DAY)),
            per_mo: format_bytes_1dp(project_from_rate(rate, SECS_PER_MONTH)),
        };
        self.last_display = Instant::now();
        true
    }

    /// Schedule the next 1 Hz Stats refresh (even while idle, so Thru decays).
    pub fn wants_periodic_refresh(&self) -> bool {
        self.bytes_recv > 0
            || self.bytes_sent > 0
            || self.msgs_recv > 0
            || self.ema_ready
            || self.ema_bps > 0.0
            || self.ema_mps > 0.0
    }

    /// Smoothed inbound messages per minute (same EMA τ as Thru).
    pub fn msgs_per_min(&self) -> f64 {
        if self.ema_ready && self.ema_mps.is_finite() && self.ema_mps > 0.0 {
            self.ema_mps * 60.0
        } else {
            0.0
        }
    }

    /// Status-strip label, e.g. `12 msg/min` or `3.4 msg/min`.
    pub fn format_msgs_per_min(&self) -> String {
        let m = self.msgs_per_min();
        if m < 10.0 {
            format!("{m:.1} msg/min")
        } else {
            format!("{} msg/min", m.round() as u64)
        }
    }

    /// Delay until the next display refresh is due (for `request_repaint_after`).
    pub fn time_until_display_refresh(&self) -> Duration {
        DISPLAY_INTERVAL.saturating_sub(self.last_display.elapsed())
    }

    fn total_bytes(&self) -> u64 {
        self.bytes_recv.saturating_add(self.bytes_sent)
    }

    /// Fold the latest interval into the EMA (`τ = EMA_TAU_SECS`).
    fn update_ema(&mut self) {
        let now = Instant::now();
        let total = self.total_bytes();
        let msgs = self.msgs_recv;
        let dt = now.duration_since(self.last_sample_at).as_secs_f64();
        if dt < 0.001 {
            return;
        }
        let instant_bps = (total.saturating_sub(self.last_sample_total) as f64) / dt;
        let instant_mps = (msgs.saturating_sub(self.last_sample_msgs) as f64) / dt;
        if !self.ema_ready {
            self.ema_bps = instant_bps.max(0.0);
            self.ema_mps = instant_mps.max(0.0);
            self.ema_ready = true;
        } else {
            let alpha = 1.0 - (-dt / EMA_TAU_SECS).exp();
            self.ema_bps = alpha * instant_bps.max(0.0) + (1.0 - alpha) * self.ema_bps;
            self.ema_mps = alpha * instant_mps.max(0.0) + (1.0 - alpha) * self.ema_mps;
            if !self.ema_bps.is_finite() || self.ema_bps < 0.0 {
                self.ema_bps = 0.0;
            }
            if !self.ema_mps.is_finite() || self.ema_mps < 0.0 {
                self.ema_mps = 0.0;
            }
        }
        self.last_sample_at = now;
        self.last_sample_total = total;
        self.last_sample_msgs = msgs;
    }

    /// Smoothed combined in+out bytes/sec (EMA).
    pub fn throughput_bps(&self) -> f64 {
        if self.ema_ready && self.ema_bps.is_finite() && self.ema_bps > 0.0 {
            self.ema_bps
        } else {
            0.0
        }
    }

    /// Project volume if the smoothed rate held for `seconds` (hour/day/month).
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

fn format_scaled(n: u64, decimals: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TEN_GB: u64 = 10 * 1024 * 1024 * 1024;

    if n >= TEN_GB {
        format!("{:.*} GB", decimals, n as f64 / GB)
    } else if (n as f64) >= MB {
        format!("{:.*} MB", decimals, n as f64 / MB)
    } else if (n as f64) >= KB {
        format!("{:.*} KB", decimals, n as f64 / KB)
    } else {
        format!("{n} B")
    }
}

/// Human units for totals / Thru: B → KB → MB; GB only once ≥ 10 GB (2 dp on MB/GB).
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

/// Projection strings: same unit steps, **one** decimal on KB/MB/GB.
pub fn format_bytes_1dp(n: u64) -> String {
    format_scaled(n, 1)
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
    fn projection_format_one_decimal() {
        let n = (12.34 * 1024.0 * 1024.0) as u64;
        assert_eq!(format_bytes_1dp(n), "12.3 MB");
        let kb = (12.34 * 1024.0) as u64;
        assert_eq!(format_bytes_1dp(kb), "12.3 KB");
    }

    #[test]
    fn ema_smooths_toward_instant() {
        let mut s = TrafficStats::default();
        // Seed: 1000 B over ~1s → instant ~1000 B/s.
        s.bytes_recv = 1000;
        s.last_sample_at = Instant::now() - Duration::from_secs(1);
        s.last_sample_total = 0;
        s.update_ema();
        let first = s.throughput_bps();
        assert!((first - 1000.0).abs() < 50.0, "first={first}");

        // Idle interval: instant 0 — EMA should drop but not to zero in one second.
        s.last_sample_at = Instant::now() - Duration::from_secs(1);
        s.last_sample_total = s.total_bytes();
        s.update_ema();
        let second = s.throughput_bps();
        assert!(second < first, "second={second} first={first}");
        assert!(second > first * 0.5, "should still be >50% after 1s with τ=20: {second}");
    }

    #[test]
    fn display_refreshes_at_most_once_per_second() {
        let mut s = TrafficStats::default();
        assert!(s.refresh_display_if_due());
        s.record_recv(100);
        assert!(!s.refresh_display_if_due());
        assert_eq!(s.display().recv, "0 B");
        s.last_display = Instant::now()
            .checked_sub(DISPLAY_INTERVAL)
            .unwrap_or_else(Instant::now);
        assert!(s.refresh_display_if_due());
        assert_eq!(s.display().recv, "100 B");
    }
}
