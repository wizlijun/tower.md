//! Stable error codes of CLI contract C0 and the exit status of each.
use serde::{Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Usage,
    TooLong,
    NotFound,
    NotInitialized,
    LeaseBusy,
    StaleEpoch,
    Conflict,
    Pending,
    Deferred,
    GuardBlocked,
    GateFailed,
    NeedsProposal,
    Rejected,
    NeedsHuman,
    DaemonUnavailable,
    HubOffline,
    Internal,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 17] = [
        ErrorCode::Usage,
        ErrorCode::TooLong,
        ErrorCode::NotFound,
        ErrorCode::NotInitialized,
        ErrorCode::LeaseBusy,
        ErrorCode::StaleEpoch,
        ErrorCode::Conflict,
        ErrorCode::Pending,
        ErrorCode::Deferred,
        ErrorCode::GuardBlocked,
        ErrorCode::GateFailed,
        ErrorCode::NeedsProposal,
        ErrorCode::Rejected,
        ErrorCode::NeedsHuman,
        ErrorCode::DaemonUnavailable,
        ErrorCode::HubOffline,
        ErrorCode::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Usage => "usage",
            ErrorCode::TooLong => "too_long",
            ErrorCode::NotFound => "not_found",
            ErrorCode::NotInitialized => "not_initialized",
            ErrorCode::LeaseBusy => "lease_busy",
            ErrorCode::StaleEpoch => "stale_epoch",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Pending => "pending",
            ErrorCode::Deferred => "deferred",
            ErrorCode::GuardBlocked => "guard_blocked",
            ErrorCode::GateFailed => "gate_failed",
            ErrorCode::NeedsProposal => "needs_proposal",
            ErrorCode::Rejected => "rejected",
            ErrorCode::NeedsHuman => "needs_human",
            ErrorCode::DaemonUnavailable => "daemon_unavailable",
            ErrorCode::HubOffline => "hub_offline",
            ErrorCode::Internal => "internal",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == text)
    }

    /// Exit status: 0 ok, 1 internal, 2 usage, 3 missing, 4 state conflict,
    /// 5 not yet, 6 stopped by a guard, gate or rule, 7 needs a person,
    /// 8 unreachable.
    pub fn exit(self) -> u8 {
        match self {
            ErrorCode::Internal => 1,
            ErrorCode::Usage | ErrorCode::TooLong => 2,
            ErrorCode::NotFound | ErrorCode::NotInitialized => 3,
            ErrorCode::LeaseBusy | ErrorCode::StaleEpoch | ErrorCode::Conflict => 4,
            ErrorCode::Pending | ErrorCode::Deferred => 5,
            ErrorCode::GuardBlocked | ErrorCode::GateFailed | ErrorCode::NeedsProposal | ErrorCode::Rejected => 6,
            ErrorCode::NeedsHuman => 7,
            ErrorCode::DaemonUnavailable | ErrorCode::HubOffline => 8,
        }
    }

    /// Codes for which repeating the same call later can succeed unchanged.
    pub fn transient(self) -> bool {
        matches!(
            self,
            ErrorCode::Pending
                | ErrorCode::Deferred
                | ErrorCode::DaemonUnavailable
                | ErrorCode::HubOffline
                | ErrorCode::LeaseBusy
        )
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCode;

    #[test]
    fn exit_statuses_follow_the_c0_table() {
        let table = [
            ("internal", 1),
            ("usage", 2),
            ("too_long", 2),
            ("not_found", 3),
            ("not_initialized", 3),
            ("lease_busy", 4),
            ("stale_epoch", 4),
            ("conflict", 4),
            ("pending", 5),
            ("deferred", 5),
            ("guard_blocked", 6),
            ("gate_failed", 6),
            ("needs_proposal", 6),
            ("rejected", 6),
            ("needs_human", 7),
            ("daemon_unavailable", 8),
            ("hub_offline", 8),
        ];
        assert_eq!(table.len(), ErrorCode::ALL.len());
        for (name, exit) in table {
            assert_eq!(ErrorCode::parse(name).unwrap().exit(), exit, "{name}");
        }
    }

    #[test]
    fn names_round_trip_through_parse_and_json() {
        for code in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(code.as_str()), Some(code));
            assert_eq!(serde_json::to_value(code).unwrap(), code.as_str());
        }
        assert_eq!(ErrorCode::parse("approved"), None);
    }

    #[test]
    fn only_waiting_offline_and_busy_codes_are_transient() {
        let transient: Vec<_> = ErrorCode::ALL.into_iter().filter(|c| c.transient()).map(ErrorCode::as_str).collect();
        assert_eq!(transient, ["lease_busy", "pending", "deferred", "daemon_unavailable", "hub_offline"]);
    }
}
