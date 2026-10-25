//! M3 scheduler and integrator (FR-11, FR-14, FR-30–32): who signs what.
//! Everything here is a projection of accepted events; the daemon keeps
//! only disposable process handles.

/// The scheduler claims, launches and releases dispatched runs.
pub const SCHEDULER: &str = "process:towermd-scheduler";
/// The integrator verifies and fast-forwards `main`, one task at a time.
pub const INTEGRATOR: &str = "process:towermd-integrator";
