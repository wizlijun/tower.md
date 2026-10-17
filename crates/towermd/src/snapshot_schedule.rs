//! Rebuildable snapshot scheduling. All deadlines use a process monotonic clock;
//! Unix milliseconds are retained only for the user-visible failure timestamp.
use serde::Serialize;
use serde_json::{json, Value};
use towermd_core::hub_protocol::NewEvent;
use towermd_core::project_session::SnapshotSummary;
use towermd_core::snapshot_service::SnapshotAttempt;

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
    initialized: bool,
    fast_seq: Option<u64>,
    fast_max_seq: Option<u64>,
    periodic_key: Option<(u64, Option<String>, u64)>,
    periodic_deadline: Option<u64>,
    needs_preparation: bool,
    alert_eid: Option<String>,
    pending_alert: Option<NewEvent>,
    episode_basis: Option<Value>,
}

impl SnapshotSchedule {
    #[cfg(test)]
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
            self.fast_seq.get_or_insert(seq);
            self.fast_max_seq = self.fast_max_seq.max(Some(seq));
            self.event_deadline
                .get_or_insert(now.saturating_add(25_000));
        }
    }
    pub fn eligible(&self, now: u64) -> bool {
        self.next_attempt.map_or_else(
            || {
                self.deferred_since.is_some()
                    || self.event_deadline.is_some_and(|deadline| now >= deadline)
                    || self
                        .periodic_deadline
                        .is_some_and(|deadline| now >= deadline)
            },
            |deadline| now >= deadline,
        )
    }
    #[cfg(test)]
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
    pub fn written(&mut self, last: SnapshotLast, installed_fast_max: Option<u64>) {
        self.fast_max_seq = self.fast_max_seq.max(installed_fast_max);
        let later_fast_pending = self.fast_max_seq.is_some_and(|seq| seq > last.seq);
        self.last = Some(last);
        self.deferred_reason = None;
        self.deferred_since = None;
        self.first_failure_mono = None;
        self.next_attempt = None;
        // A completed checkpoint cannot cover a later already-observed event
        // or restart the monotonic deadline granted to that event.
        if !later_fast_pending {
            self.event_deadline = None;
            self.fast_seq = None;
            self.fast_max_seq = None;
        }
        self.periodic_deadline = None;
        self.periodic_key = None;
        self.alert_eid = None;
        self.pending_alert = None;
        self.episode_basis = None;
        self.failures = 0;
        self.alerted = false;
    }
    pub fn install(&mut self, summary: &SnapshotSummary, now: ClockReading, interval: u64) {
        let cold = !self.initialized;
        let closed = self
            .alert_eid
            .as_ref()
            .or_else(|| self.pending_alert.as_ref().map(|event| &event.eid))
            .is_some_and(|eid| summary.closed_alerts.contains(eid));
        if closed {
            if let Some(last) = &summary.last {
                self.written(
                    last.into(),
                    summary.fast_pending.as_ref().map(|pending| pending.max_seq),
                );
            }
        }
        self.initialized = true;
        self.consumed_seq = summary.cursor;
        self.last = summary.last.as_ref().map(SnapshotLast::from);
        if let Some(pending) = &summary.fast_pending {
            self.fast_max_seq = self.fast_max_seq.max(Some(pending.max_seq));
            // The first sequence owns this granted window. A completed prefix
            // may replace Summary's first event while higher observed fast
            // events still belong to the original monotonic deadline.
            if self.fast_seq.is_none() {
                self.fast_seq = Some(pending.seq);
                self.event_deadline = Some(if cold {
                    restored_deadline(pending.unix_ms, 25_000, now)
                } else {
                    now.mono_ms.saturating_add(25_000)
                });
            }
        } else {
            self.fast_seq = None;
            self.fast_max_seq = None;
            self.event_deadline = None;
        }
        let periodic_key = summary.periodic_pending_since.map(|since| {
            (
                since,
                summary.last.as_ref().map(|last| last.commit.clone()),
                interval,
            )
        });
        if self.periodic_key != periodic_key {
            self.periodic_key = periodic_key;
            self.periodic_deadline = summary.periodic_pending_since.map(|since| {
                let anchor = summary
                    .last
                    .as_ref()
                    .and_then(|last| last.written_at)
                    .unwrap_or(since);
                restored_deadline(anchor, interval, now)
            });
        }
        if let Some(alert) = &summary.alert {
            if self.alert_eid.is_none() {
                self.deferred_since = Some(alert.since);
                self.deferred_reason = Some(alert.reason.clone());
                self.alerted = true;
                self.alert_eid = Some(alert.eid.clone());
                self.pending_alert = None;
                self.episode_basis = Some(alert.basis.clone());
            }
        }
    }
    pub fn can_attempt(&self, now: u64) -> bool {
        self.next_attempt_at()
            .is_none_or(|deadline| now >= deadline)
    }
    pub fn prepare_required(&mut self) {
        self.needs_preparation = true;
    }
    pub fn prepared(&mut self) {
        self.needs_preparation = false;
    }
    pub fn preparation_required(&self) -> bool {
        self.needs_preparation
    }
    pub fn status(&self) -> Value {
        json!({"last":self.last,"deferred_reason":self.deferred_reason,"deferred_since":self.deferred_since})
    }
    pub fn failed(
        &mut self,
        reason: String,
        summary: &SnapshotSummary,
        attempt: Option<&SnapshotAttempt>,
        now: ClockReading,
        interval: u64,
    ) {
        let observed = attempt.and_then(SnapshotAttempt::persisted).is_some();
        if self.episode_basis.is_none() {
            let required = attempt
                .and_then(SnapshotAttempt::required_seq)
                .or(summary.pending_through)
                .or_else(|| summary.last.as_ref().map(|last| last.seq))
                .unwrap_or(summary.cursor);
            let mut basis = json!({"source":summary.last,"required_seq":required,"target":attempt.filter(|attempt| attempt.promotion()).map(|_| "main"),"persistence":if observed {"observed"} else {"unknown"}});
            if observed {
                basis["persisted"] = Value::Null;
            }
            self.episode_basis = Some(basis);
        }
        // An uncertain result cannot erase a known Written fact or turn an
        // earlier uncertainty into evidence that no commit existed.
        if !observed {
            self.episode_basis.as_mut().expect("episode basis")["persistence"] = json!("unknown");
        }
        if let Some(persisted) = attempt.and_then(SnapshotAttempt::persisted) {
            self.episode_basis.as_mut().expect("episode basis")["persisted"] = json!(persisted);
        }
        if let Some(inflight) = attempt.and_then(SnapshotAttempt::inflight) {
            self.episode_basis.as_mut().expect("episode basis")["inflight"] = json!(inflight);
        }
        self.deferred_at(reason, now, interval);
    }
    pub fn alert_event(&mut self, host: &str, interval: u64) -> Option<NewEvent> {
        if !self.alerted || self.alert_eid.is_some() {
            return None;
        }
        Some(self.pending_alert.get_or_insert_with(|| NewEvent {
            eid: uuid::Uuid::now_v7().to_string(), host: host.into(), actor: "process:towermd-snapshot".into(),
            kind: "alert.raised".into(), layer: "L3".into(), obj: "main".into(), guards: vec![],
            data: json!({"component":"snapshot","reason":self.deferred_reason,"since":self.deferred_since,"interval":interval,"basis":self.episode_basis}),
        }).clone())
    }
    pub fn next_attempt_at(&self) -> Option<u64> {
        self.next_attempt
    }
}

impl From<&towermd_core::project_session::SnapshotLast> for SnapshotLast {
    fn from(last: &towermd_core::project_session::SnapshotLast) -> Self {
        Self {
            commit: last.commit.clone(),
            seq: last.seq,
            epoch: last.epoch,
            written_at: last.written_at,
        }
    }
}
fn restored_deadline(anchor: u64, duration: u64, now: ClockReading) -> u64 {
    now.mono_ms
        .saturating_add(duration.saturating_sub(now.unix_ms.saturating_sub(anchor).min(duration)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_attempt_keeps_known_persistence_and_never_upgrades_certainty() {
        let mut schedule = SnapshotSchedule {
            episode_basis: Some(
                json!({"source":null,"required_seq":2,"persistence":"observed","persisted":{"commit":"previous-real-written"}}),
            ),
            ..Default::default()
        };
        let now = ClockReading {
            mono_ms: 1,
            unix_ms: 100,
        };
        schedule.failed(
            "unknown result".into(),
            &SnapshotSummary::default(),
            None,
            now,
            60_000,
        );
        let basis = schedule.episode_basis.as_ref().unwrap();
        assert_eq!(basis["persisted"]["commit"], "previous-real-written");
        assert_eq!(basis["persistence"], "unknown");
        schedule.failed(
            "still unknown".into(),
            &SnapshotSummary::default(),
            None,
            now,
            60_000,
        );
        assert_eq!(
            schedule.episode_basis.as_ref().unwrap()["persistence"],
            "unknown"
        );
    }
    #[test]
    fn fast_window_completion_is_a_prefix_over_observed_min_and_max() {
        let last = |seq| SnapshotLast {
            commit: "actual-completion-model".into(),
            seq,
            epoch: 1,
            written_at: None,
        };
        for first in 1..=3 {
            for max in first..=5 {
                for through in 0..=6 {
                    let mut schedule = SnapshotSchedule::default();
                    schedule.observe("hold.resolved", false, first, 100);
                    schedule.observe("proposal.resolved", false, max, 200);
                    schedule.written(last(through), None);
                    if max > through {
                        assert_eq!(
                            schedule.fast_seq,
                            Some(first),
                            "first={first} max={max} Q={through}"
                        );
                        assert_eq!(schedule.fast_max_seq, Some(max));
                        assert_eq!(schedule.event_deadline, Some(25_100));
                    } else {
                        assert!(schedule.fast_seq.is_none());
                        assert!(schedule.fast_max_seq.is_none());
                        assert!(schedule.event_deadline.is_none());
                        schedule.observe("hold.resolved", false, through + 1, 500);
                        assert_eq!(schedule.fast_seq, Some(through + 1));
                        assert_eq!(schedule.event_deadline, Some(25_500));
                    }
                }
            }
        }
        // A completion also consumes the actual installed summary's newer max,
        // even if the scheduler had not seen that summary before writer exit.
        let mut schedule = SnapshotSchedule::default();
        schedule.observe("hold.resolved", false, 3, 100);
        schedule.written(last(3), Some(9));
        let summary = SnapshotSummary {
            cursor: 9,
            fast_pending: Some(towermd_core::project_session::SnapshotPending {
                seq: 9,
                max_seq: 9,
                unix_ms: 500,
            }),
            ..Default::default()
        };
        schedule.install(
            &summary,
            ClockReading {
                mono_ms: 900,
                unix_ms: 900,
            },
            60_000,
        );
        assert_eq!(schedule.fast_seq, Some(3));
        assert_eq!(schedule.fast_max_seq, Some(9));
        assert_eq!(schedule.event_deadline, Some(25_100));

        schedule.install(
            &summary,
            ClockReading {
                mono_ms: 10_000,
                unix_ms: u64::MAX,
            },
            60_000,
        );
        assert_eq!(schedule.fast_seq, Some(3));
        assert_eq!(schedule.fast_max_seq, Some(9));
        assert_eq!(schedule.event_deadline, Some(25_100));

        // Live preparation has not observed a fast event yet. Its first verified
        // summary starts a window now, independent of the event's old Unix time.
        let mut unseen = SnapshotSchedule::default();
        unseen.install(
            &SnapshotSummary::default(),
            ClockReading {
                mono_ms: 0,
                unix_ms: 0,
            },
            60_000,
        );
        unseen.prepare_required();
        assert!(unseen.event_deadline.is_none());
        unseen.install(
            &summary,
            ClockReading {
                mono_ms: 500,
                unix_ms: u64::MAX,
            },
            60_000,
        );
        assert_eq!(unseen.fast_seq, Some(9));
        assert_eq!(unseen.fast_max_seq, Some(9));
        assert_eq!(unseen.event_deadline, Some(25_500));

        let mut schedule = SnapshotSchedule::default();
        schedule.observe("hold.resolved", false, 1, 100);
        for (kind, stale, seq) in [
            ("hold.resolved", true, 2),
            ("snapshot.written", false, 3),
            ("alert.raised", false, 4),
            ("future.audit", false, 5),
        ] {
            schedule.observe(kind, stale, seq, 500);
        }
        assert_eq!(schedule.fast_max_seq, Some(1));
        schedule.written(last(1), None);
        assert!(schedule.event_deadline.is_none());
    }

    #[test]
    fn restored_deadlines_are_bounded_and_live_unix_changes_do_not_move_them() {
        use towermd_core::project_session::SnapshotPending;
        let summary = SnapshotSummary {
            fast_pending: Some(SnapshotPending {
                seq: 1,
                max_seq: 1,
                unix_ms: 1_000,
            }),
            periodic_pending_since: Some(1_000),
            ..Default::default()
        };
        let mut future = SnapshotSchedule::default();
        future.install(
            &summary,
            ClockReading {
                mono_ms: 100,
                unix_ms: 0,
            },
            60_000,
        );
        assert_eq!(future.event_deadline, Some(25_100));
        assert_eq!(future.periodic_deadline, Some(60_100));
        future.install(
            &summary,
            ClockReading {
                mono_ms: 200,
                unix_ms: u64::MAX,
            },
            60_000,
        );
        assert_eq!(future.event_deadline, Some(25_100));
        assert_eq!(future.periodic_deadline, Some(60_100));
        let mut ancient = SnapshotSchedule::default();
        ancient.install(
            &summary,
            ClockReading {
                mono_ms: 100,
                unix_ms: u64::MAX,
            },
            60_000,
        );
        assert!(ancient.eligible(100));
        let mut saturated = SnapshotSchedule::default();
        saturated.install(
            &summary,
            ClockReading {
                mono_ms: u64::MAX - 1,
                unix_ms: 0,
            },
            60_000,
        );
        assert_eq!(saturated.event_deadline, Some(u64::MAX));
        assert_eq!(saturated.periodic_deadline, Some(u64::MAX));
    }
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
        s.written(
            SnapshotLast {
                commit: "c".into(),
                seq: 8,
                epoch: 3,
                written_at: None,
            },
            None,
        );
        assert_eq!(s.deferred_since, None);
        assert_eq!(s.next_attempt_at(), None);
        assert!(!s.deferred("remote".into(), 12, 10));
        assert_eq!(s.deferred_since, Some(12));
        assert_eq!(s.last.unwrap().written_at, None);
    }
}
