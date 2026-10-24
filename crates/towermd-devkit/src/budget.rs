//! 12.1 budget interrupt. Limits come from `budget:` in `config.yaml`
//! (`run_usd`, `module_usd`); no limit means no budget check. An unknown cost
//! is never counted as 0: when a limit is set and the spend cannot be known,
//! the run is not admitted and a person is asked instead.
use serde::Serialize;
use serde_json::{json, Value};

use crate::run_record::{self, BUDGET_EXHAUSTED, BUDGET_UNKNOWN};
use crate::run_report::RUNNER;
use towermd_core::hub_protocol::NewEvent;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Budget {
    pub run_usd: Option<f64>,
    pub module_usd: Option<f64>,
}

impl Budget {
    /// Missing, non-positive or malformed limits mean no limit for that key.
    pub fn from_config(section: Option<&serde_yaml::Value>) -> Self {
        let limit = |key: &str| section.and_then(|s| s.get(key)).and_then(serde_yaml::Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0);
        Self { run_usd: limit("run_usd"), module_usd: limit("module_usd") }
    }
    pub fn is_set(&self) -> bool {
        self.run_usd.is_some() || self.module_usd.is_some()
    }
}

/// Spend recorded for one module: the reported costs, and how many
/// sessions ended without one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Spend {
    pub known_usd: f64,
    pub unknown_runs: u32,
}

pub fn module_spend(state: &Value, module: &str) -> Result<Spend, String> {
    let mut spend = Spend::default();
    for record in run_record::records(state)?.values().filter(|record| record.module == module) {
        spend.known_usd += record.known_cost_usd;
        spend.unknown_runs += record.unknown_cost;
    }
    Ok(spend)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The module has used up its budget.
    Exhausted,
    /// A limit is set but the spend cannot be known.
    Unknown,
}
impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::Exhausted => BUDGET_EXHAUSTED,
            Refusal::Unknown => BUDGET_UNKNOWN,
        }
    }
}

/// The limit for the next run, or why it may not start. `reports_cost` is
/// the adapter's capability.
pub fn admit(budget: Budget, spend: Spend, reports_cost: bool) -> Result<Option<f64>, Refusal> {
    if !budget.is_set() {
        return Ok(None);
    }
    if !reports_cost {
        return Err(Refusal::Unknown);
    }
    let Some(module) = budget.module_usd else { return Ok(budget.run_usd) };
    if spend.unknown_runs > 0 {
        return Err(Refusal::Unknown);
    }
    let left = module - spend.known_usd;
    if left <= 0.0 {
        return Err(Refusal::Exhausted);
    }
    Ok(Some(budget.run_usd.map_or(left, |run| run.min(left))))
}

/// The budget interrupt for a run that was not admitted, signed by the
/// runner. Nothing ran, so nothing else changes.
pub fn refusal_event(host: &str, task: &str, module: &str, refusal: Refusal, budget: Budget, spend: Spend) -> NewEvent {
    let eid = uuid::Uuid::now_v7().to_string();
    let title = match refusal {
        Refusal::Exhausted => "模块预算已用完，运行未启动",
        Refusal::Unknown => "设了预算但花费无法得知，运行未启动",
    };
    NewEvent {
        eid: eid.clone(), host: host.into(), actor: RUNNER.into(), kind: "alert.raised".into(), layer: "L5".into(),
        obj: eid, guards: Vec::new(),
        data: json!({"reason": refusal.reason(), "title": title, "detail": {
            "task": task, "module": module, "severity": "blocking", "error": title,
            "budget_usd": budget.module_usd.or(budget.run_usd), "known_usd": spend.known_usd,
            "unknown_runs": spend.unknown_runs,
        }}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(text: &str) -> serde_yaml::Value {
        serde_yaml::from_str(text).unwrap()
    }

    #[test]
    fn limits_read_from_config_and_bad_values_mean_no_limit() {
        let budget = Budget::from_config(Some(&yaml("run_usd: 2.5\nmodule_usd: 10")));
        assert_eq!(budget, Budget { run_usd: Some(2.5), module_usd: Some(10.0) });
        assert_eq!(Budget::from_config(Some(&yaml("run_usd: -1\nmodule_usd: x"))), Budget::default());
        assert_eq!(Budget::from_config(None), Budget::default());
    }

    #[test]
    fn unknown_cost_never_counts_as_zero() {
        let budget = Budget { run_usd: Some(2.0), module_usd: Some(5.0) };
        assert_eq!(admit(Budget::default(), Spend::default(), false), Ok(None));
        assert_eq!(admit(budget, Spend::default(), false), Err(Refusal::Unknown));
        assert_eq!(admit(budget, Spend { known_usd: 1.0, unknown_runs: 1 }, true), Err(Refusal::Unknown));
        assert_eq!(admit(budget, Spend { known_usd: 4.0, unknown_runs: 0 }, true), Ok(Some(1.0)));
        assert_eq!(admit(budget, Spend { known_usd: 1.0, unknown_runs: 0 }, true), Ok(Some(2.0)));
        assert_eq!(admit(budget, Spend { known_usd: 5.0, unknown_runs: 0 }, true), Err(Refusal::Exhausted));
        let run_only = Budget { run_usd: Some(2.0), module_usd: None };
        assert_eq!(admit(run_only, Spend { known_usd: 9.0, unknown_runs: 3 }, true), Ok(Some(2.0)));
    }

    #[test]
    fn a_refusal_is_a_runner_signed_alert() {
        let event = refusal_event("h", "T-1", "A", Refusal::Unknown, Budget { run_usd: Some(1.0), module_usd: None },
            Spend::default());
        assert_eq!((event.actor.as_str(), event.kind.as_str(), event.layer.as_str()), (RUNNER, "alert.raised", "L5"));
        assert_eq!(event.obj, event.eid);
        assert_eq!(event.data["reason"], BUDGET_UNKNOWN);
        assert_eq!(event.data["detail"]["task"], "T-1");
    }
}
