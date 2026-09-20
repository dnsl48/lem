//! Per-frame byte and decode-time counters.
//!
//! ADR 0003 keeps the JSON codec on the explicit condition that the PoC
//! measures it. These are those measurements: without them that decision
//! is an opinion rather than a deferral.

use std::time::Duration;

/// Running totals over the frames of one session.
#[derive(Debug, Default)]
pub struct Frames {
    count: u64,
    bytes: u64,
    max_bytes: usize,
    decode: Duration,
}

impl Frames {
    /// Record one frame: its framed size on the wire and the time spent
    /// turning it into typed instructions.
    pub fn record(&mut self, bytes: usize, decode: Duration) {
        self.count += 1;
        self.bytes += bytes as u64;
        self.max_bytes = self.max_bytes.max(bytes);
        self.decode += decode;
    }

    /// A one-line summary for stderr.
    pub fn report(&self) -> String {
        if self.count == 0 {
            return "0 frames".to_string();
        }
        let average_bytes = self.bytes / self.count;
        let average_decode = self.decode.as_micros() / u128::from(self.count);
        format!(
            "{} frames, {} B total, avg {average_bytes} B, max {} B, avg decode {average_decode}us",
            self.count, self.bytes, self.max_bytes
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_totals_and_averages() {
        let mut frames = Frames::default();
        frames.record(1000, Duration::from_micros(100));
        frames.record(3000, Duration::from_micros(300));

        let report = frames.report();
        assert!(report.contains("2 frames"), "{report}");
        assert!(report.contains("avg 2000 B"), "{report}");
        assert!(report.contains("max 3000 B"), "{report}");
        assert!(report.contains("avg decode 200us"), "{report}");
    }

    #[test]
    fn an_empty_report_does_not_divide_by_zero() {
        assert!(Frames::default().report().contains("0 frames"));
    }

    #[test]
    fn totals_accumulate_across_many_frames() {
        let mut frames = Frames::default();
        for n in 1..=10 {
            frames.record(n * 100, Duration::from_micros(10));
        }
        let report = frames.report();
        assert!(report.contains("10 frames"), "{report}");
        assert!(report.contains("max 1000 B"), "{report}");
    }
}
