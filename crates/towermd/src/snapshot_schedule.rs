//! Rebuildable snapshot scheduling. All deadlines use a process monotonic clock;
//! Unix milliseconds are retained only for the user-visible failure timestamp.
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SnapshotLast {
    pub commit: String,
    pub seq: u64,
    pub epoch: u64,
    pub written_at: Option<u64>,
}

#[derive(Clone, Copy)]
pub(crate) struct ClockReading {
    pub mono_ms: u64,
    pub unix_ms: u64,
}

#[derive(Default)]
pub(crate) struct SnapshotSchedule {
    last: Option<SnapshotLast>,
    deferred_reason: Option<String>,
    deferred_since: Option<u64>,
    first_failure_mono: Option<u64>,
    next_attempt: Option<u64>,
    event_deadline: Option<u64>,
    failures: u32,
    alerted: bool,
    consumed_seq: u64,
}

impl SnapshotSchedule {
    pub fn observe(&mut self, kind: &str, stale: bool, seq: u64, now: u64) {
        if seq <= self.consumed_seq {
            return;
        }
        self.consumed_seq = seq;
        if !stale
            && matches!(
                kind,
                "merge.done" | "hold.resolved" | "proposal.resolved" | "baseline.created"
            )
        {
            self.event_deadline
                .get_or_insert(now.saturating_add(25_000));
        }
    }
    pub fn eligible(&self, now: u64) -> bool {
        self.next_attempt.map_or_else(
            || self.event_deadline.is_some_and(|deadline| now >= deadline),
            |deadline| now >= deadline,
        )
    }
    pub fn deferred(&mut self, reason: String, now: u64, interval: u64) -> bool {
        self.deferred_at(
            reason,
            ClockReading {
                mono_ms: now,
                unix_ms: now,
            },
            interval,
        )
    }
    pub fn deferred_at(&mut self, reason: String, now: ClockReading, interval: u64) -> bool {
        self.deferred_reason = Some(reason);
        self.deferred_since.get_or_insert(now.unix_ms);
        let first = *self.first_failure_mono.get_or_insert(now.mono_ms);
        self.failures = self.failures.saturating_add(1);
        let delay = 1_000u64
            .saturating_mul(
                1u64.checked_shl(self.failures.saturating_sub(1))
                    .unwrap_or(u64::MAX),
            )
            .min(interval);
        self.next_attempt = Some(now.mono_ms.saturating_add(delay));
        if !self.alerted && now.mono_ms.saturating_sub(first) > interval {
            self.alerted = true;
            true
        } else {
            false
        }
    }
    pub fn written(&mut self, last: SnapshotLast) {
        self.last = Some(last);
        self.deferred_reason = None;
        self.deferred_since = None;
        self.first_failure_mono = None;
        self.next_attempt = None;
        self.event_deadline = None;
        self.failures = 0;
        self.alerted = false;
    }
    pub fn next_attempt_at(&self) -> Option<u64> {
        self.next_attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_events_debounce_at_first_event_deadline() {
        let mut s = SnapshotSchedule::default();
        s.observe("hold.resolved", false, 1, 1_000);
        s.observe("proposal.resolved", false, 2, 20_000);
        assert!(!s.eligible(25_999));
        assert!(s.eligible(26_000));
    }
    #[test]
    fn deferral_is_exponential_capped_and_alerts_once() {
        let mut s = SnapshotSchedule::default();
        assert!(!s.deferred("busy".into(), 0, 8_000));
        assert_eq!(s.next_attempt_at(), Some(1_000));
        assert!(!s.deferred("busy".into(), 1_000, 8_000));
        assert_eq!(s.next_attempt_at(), Some(3_000));
        assert!(!s.deferred("busy".into(), 3_000, 8_000));
        assert_eq!(s.next_attempt_at(), Some(7_000));
        assert!(!s.deferred("busy".into(), 7_000, 8_000));
        assert_eq!(s.next_attempt_at(), Some(15_000));
        assert!(s.deferred("busy".into(), 15_000, 8_000));
        assert_eq!(s.next_attempt_at(), Some(23_000));
        assert!(!s.deferred("busy".into(), 23_000, 8_000));
    }
    #[test]
    fn duplicate_stale_and_non_business_events_do_not_start_window() {
        let mut s = SnapshotSchedule::default();
        for (seq, kind) in [
            "heartbeat",
            "snapshot.written",
            "snapshot.ack",
            "alert.raised",
            "task.state",
        ]
        .iter()
        .enumerate()
        {
            s.observe(kind, false, seq as u64 + 1, 0);
        }
        s.observe("hold.resolved", true, 6, 0);
        s.observe("hold.resolved", false, 6, 0);
        assert!(!s.eligible(u64::MAX));
        s.observe("baseline.created", false, 7, 100);
        assert!(s.eligible(25_100));
    }
    #[test]
    fn wall_clock_changes_and_failure_reasons_do_not_move_episode() {
        let mut s = SnapshotSchedule::default();
        assert!(!s.deferred_at(
            "busy".into(),
            ClockReading {
                mono_ms: 10,
                unix_ms: 50_000
            },
            2_000
        ));
        assert!(!s.deferred_at(
            "remote".into(),
            ClockReading {
                mono_ms: 1_010,
                unix_ms: 1
            },
            2_000
        ));
        assert_eq!(s.deferred_since, Some(50_000));
        assert!(s.deferred_at(
            "manual".into(),
            ClockReading {
                mono_ms: 2_011,
                unix_ms: u64::MAX
            },
            2_000
        ));
        assert_eq!(s.deferred_since, Some(50_000));
    }
    #[test]
    fn saturated_backoff_small_interval_and_success_start_new_episode() {
        let mut s = SnapshotSchedule::default();
        s.deferred("busy".into(), 100, 5);
        assert_eq!(s.next_attempt_at(), Some(105));
        s.failures = u32::MAX;
        s.deferred("busy".into(), u64::MAX - 1, u64::MAX);
        assert_eq!(s.next_attempt_at(), Some(u64::MAX));
        s.written(SnapshotLast {
            commit: "c".into(),
            seq: 8,
            epoch: 3,
            written_at: None,
        });
        assert_eq!(s.deferred_since, None);
        assert_eq!(s.next_attempt_at(), None);
        assert!(!s.deferred("remote".into(), 12, 10));
        assert_eq!(s.deferred_since, Some(12));
        assert_eq!(s.last.unwrap().written_at, None);
    }
}
