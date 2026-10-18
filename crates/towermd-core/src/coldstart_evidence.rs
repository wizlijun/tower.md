//! Durable initial verification declarations. These never certify Task completion.
use crate::{
    catalog::Catalog,
    coldstart::{prepare_scan, ScanInput},
    coldstart_verification::{aggregate_verification, verification_commands, ModuleVerification},
    hub_protocol::{Event, NewEvent},
    recovery_contract::{
        finish::VerificationLogRef,
        verification::{VerificationCommand, VerificationReport},
    },
    verifier::SealedVerification,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const ACTOR: &str = "process:towermd-verifier";

/// Constructed only by scanning the actual fixed Git input, never from request JSON.
pub struct PreparedColdstart {
    input: ScanInput,
    scan_id: String,
    scan: Value,
}
impl PreparedColdstart {
    pub fn prepare(input: ScanInput, scan_id: &str) -> Result<Self, String> {
        token(scan_id)?;
        let scan = prepare_scan(&input)?;
        verification_commands(&input, &scan)?;
        Ok(Self {
            input,
            scan_id: scan_id.into(),
            scan,
        })
    }
    pub fn input(&self) -> &ScanInput {
        &self.input
    }
    pub fn scan(&self) -> &Value {
        &self.scan
    }
    pub fn commands(&self) -> Result<Vec<VerificationCommand>, String> {
        verification_commands(&self.input, &self.scan)
    }
    pub fn freeze(
        self,
        sealed: &SealedVerification,
        host: &str,
        generated_at: u64,
    ) -> Result<FrozenColdstart, String> {
        if sealed.root() != self.input.root
            || sealed.common_dir() != self.input.common_dir
            || sealed.commands() != self.commands()?
        {
            return Err("sealed verifier coordinates/commands differ from fixed scan".into());
        }
        freeze_report(self, sealed.report(), host, generated_at)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceIdentity {
    evidence_id: String,
    event_eid: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedReport {
    schema: u32,
    scan_id: String,
    input: ScanInput,
    scan: Value,
    report: VerificationReport,
    evidence_ids: BTreeMap<String, EvidenceIdentity>,
    generated_at: String,
}

/// All identities and bytes are fixed before the first outbox pending record.
/// No deserializer or public raw-event constructor can mint this producer input.
pub struct FrozenColdstart {
    report_event: NewEvent,
    evidence_events: Vec<NewEvent>,
    package: SharedReport,
}

/// Read-only job response reconstructed from durable declarations. This DTO is
/// deliberately not accepted by any producer or completion API.
#[derive(Clone, Debug, Serialize)]
pub struct RecordedColdstart {
    pub input: ScanInput,
    pub scan: Value,
    pub report: VerificationReport,
    pub report_eid: String,
    pub scan_id: String,
    pub evidence: Vec<Value>,
}

/// An actual reader-confirmed complete scan declaration, not Task verification
/// authority. Only read_completed can construct this value.
pub struct CompletedColdstart {
    recorded: RecordedColdstart,
}
impl CompletedColdstart {
    pub fn input(&self) -> &ScanInput {
        &self.recorded.input
    }
    pub fn scan(&self) -> &Value {
        &self.recorded.scan
    }
    pub fn report(&self) -> &VerificationReport {
        &self.recorded.report
    }
    pub fn scan_id(&self) -> &str {
        &self.recorded.scan_id
    }
    pub fn report_eid(&self) -> &str {
        &self.recorded.report_eid
    }
    pub fn evidence(&self) -> &[Value] {
        &self.recorded.evidence
    }
}

pub fn read_completed(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    scan_id: &str,
) -> Result<Option<CompletedColdstart>, String> {
    let history = crate::recovery_git::read_recovery_history(catalog, inputs)?;
    let Some(event) = history.coldstart_for_scan(scan_id) else {
        return Ok(None);
    };
    let package: SharedReport = serde_json::from_value(event.data["coldstart_report"].clone())
        .map_err(|e| e.to_string())?;
    let mut evidence = Vec::new();
    for module in package.scan["modules"]
        .as_array()
        .ok_or("completed scan lacks modules")?
    {
        let id = module["id"]
            .as_str()
            .ok_or("completed scan module lacks id")?;
        let identity = package
            .evidence_ids
            .get(id)
            .ok_or("completed scan lacks mapped identity")?;
        let Some(event) = history.coldstart_original(&identity.event_eid) else {
            return Ok(None);
        };
        evidence.push(event.data["evidence"].clone());
    }
    inputs.probe(catalog)?;
    Ok(Some(CompletedColdstart {
        recorded: RecordedColdstart {
            input: package.input,
            scan: package.scan,
            report: package.report,
            report_eid: event.eid.clone(),
            scan_id: package.scan_id,
            evidence,
        },
    }))
}
impl FrozenColdstart {
    pub fn input(&self) -> &ScanInput {
        &self.package.input
    }
    pub fn scan(&self) -> &Value {
        &self.package.scan
    }
    pub fn report(&self) -> &VerificationReport {
        &self.package.report
    }
    pub fn report_eid(&self) -> &str {
        &self.report_event.eid
    }
    pub fn scan_id(&self) -> &str {
        &self.package.scan_id
    }
    pub fn evidence(&self) -> Vec<Value> {
        self.evidence_events
            .iter()
            .map(|e| e.data["evidence"].clone())
            .collect()
    }
    pub fn recorded(&self) -> RecordedColdstart {
        RecordedColdstart {
            input: self.package.input.clone(),
            scan: self.package.scan.clone(),
            report: self.package.report.clone(),
            report_eid: self.report_event.eid.clone(),
            scan_id: self.package.scan_id.clone(),
            evidence: self.evidence(),
        }
    }
    pub(crate) fn events(&self) -> impl Iterator<Item = &NewEvent> {
        std::iter::once(&self.report_event).chain(self.evidence_events.iter())
    }
    pub(crate) fn matches_catalog(&self, catalog: &Catalog) -> Result<(), String> {
        if self.package.input.project_id != catalog.config.project_id()
            || self.package.input.common_dir != catalog.common_dir
        {
            return Err("coldstart producer project/coordinates changed".into());
        }
        Ok(())
    }
}

fn freeze_report(
    prepared: PreparedColdstart,
    report: &VerificationReport,
    host: &str,
    generated_at: u64,
) -> Result<FrozenColdstart, String> {
    token(host)?;
    let results = aggregate_verification(&prepared.input, &prepared.scan, report)?;
    let package = SharedReport {
        schema: 1,
        scan_id: prepared.scan_id,
        input: prepared.input,
        scan: prepared.scan,
        report: report.clone(),
        evidence_ids: results
            .iter()
            .map(|result| {
                (
                    result.module.clone(),
                    EvidenceIdentity {
                        evidence_id: format!("E-{}", uuid::Uuid::now_v7()),
                        event_eid: uuid::Uuid::now_v7().to_string(),
                    },
                )
            })
            .collect(),
        generated_at: crate::hub::timestamp(generated_at)
            .map_err(|e| e.to_string())?
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    };
    let report_event = NewEvent {
        eid: uuid::Uuid::now_v7().to_string(),
        host: host.into(),
        actor: ACTOR.into(),
        kind: "coldstart.reported".into(),
        layer: "L5".into(),
        obj: package.scan_id.clone(),
        data: json!({"coldstart_report":package}),
        guards: vec![],
    };
    crate::hub::validate_input(&report_event).map_err(|e| e.to_string())?;
    let evidence_events = evidence_events(&report_event, &package, &results)?;
    Ok(FrozenColdstart {
        report_event,
        evidence_events,
        package,
    })
}

fn evidence_events(
    report_event: &NewEvent,
    package: &SharedReport,
    results: &[ModuleVerification],
) -> Result<Vec<NewEvent>, String> {
    results.iter().map(|result| {
        let identity = package.evidence_ids.get(&result.module).ok_or("report lacks module identity")?;
        let log = VerificationLogRef { report_eid: report_event.eid.clone(), report_sha256: result.report_hash.clone() };
        Ok(NewEvent {
            eid: identity.event_eid.clone(), host: report_event.host.clone(), actor: ACTOR.into(),
            kind: "evidence.recorded".into(), layer: "L5".into(), obj: identity.evidence_id.clone(), guards: vec![],
            data: json!({"evidence":{
                "type":"Evidence", "id":identity.evidence_id, "kind":"test", "module":result.module,
                "commit":package.input.revision, "tree":package.input.tree, "scan_id":package.scan_id,
                "shared":result.shared, "result":result.result, "log":log,
                "generated":{"by":ACTOR, "at":package.generated_at},
                "coldstart":{"schema":1, "execution_indices":result.execution_indices,
                    "diagnostics":result.diagnostics, "local_only":true, "host":report_event.host}
            }}),
        })
    }).collect()
}

fn token(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("coldstart identity must be a nonempty token".into());
    }
    Ok(())
}

pub(crate) fn is_coldstart_evidence(event: &Event) -> bool {
    event.kind == "evidence.recorded" && event.data["evidence"]["coldstart"]["schema"] == 1
}

fn envelope(event: &Event) -> Result<(), String> {
    if event.actor != ACTOR
        || event.layer != "L5"
        || !event.guards.is_empty()
        || event.epoch.is_some()
        || event.cost_usd.is_some()
        || event.legacy
    {
        return Err("invalid coldstart event envelope".into());
    }
    no_authorization(&event.data)
}

fn no_authorization(data: &Value) -> Result<(), String> {
    for value in [data, &data["evidence"], &data["evidence"]["coldstart"]] {
        for key in [
            "task",
            "run",
            "gate",
            "gates",
            "fences",
            "epoch",
            "base",
            "authority",
            "baseline",
            "plan_hash",
            "commands_hash",
            "review",
            "completed_eid",
        ] {
            if value.get(key).is_some() {
                return Err(format!(
                    "coldstart declaration contains authorization field {key}"
                ));
            }
        }
    }
    Ok(())
}

/// Local declaration-only validation for deterministic projection. No Git or trust
/// inference: the actual-history reader separately validates report references.
pub(crate) fn projected_evidence(event: &Event) -> Result<Option<(String, Value)>, String> {
    if event.stale || !is_coldstart_evidence(event) {
        return Ok(None);
    }
    envelope(event)?;
    let evidence = &event.data["evidence"];
    if crate::types::lookup("Evidence").is_none()
        || evidence["type"] != "Evidence"
        || evidence["kind"] != "test"
    {
        return Err("coldstart requires registered Evidence type and test kind".into());
    }
    let id = evidence["id"]
        .as_str()
        .ok_or("coldstart Evidence lacks id")?;
    token(id)?;
    if event.obj != id {
        return Err("coldstart Evidence id differs from event obj".into());
    }
    for key in ["module", "scan_id"] {
        token(
            evidence[key]
                .as_str()
                .ok_or("coldstart Evidence lacks identity")?,
        )?;
    }
    for key in ["commit", "tree"] {
        crate::recovery_contract::object_id(
            evidence[key]
                .as_str()
                .ok_or("coldstart Evidence lacks commit/tree")?,
        )?;
    }
    if !matches!(
        evidence["result"].as_str(),
        Some("passed" | "failed" | "no_tests")
    ) || !evidence["shared"].is_boolean()
        || evidence["generated"]["by"] != ACTOR
        || evidence["coldstart"]["local_only"] != true
        || evidence["coldstart"]["host"] != event.host
    {
        return Err("invalid coldstart Evidence result/generated/storage metadata".into());
    }
    chrono::DateTime::parse_from_rfc3339(
        evidence["generated"]["at"]
            .as_str()
            .ok_or("coldstart Evidence lacks generated.at")?,
    )
    .map_err(|e| e.to_string())?;
    let log: VerificationLogRef =
        serde_json::from_value(evidence["log"].clone()).map_err(|e| e.to_string())?;
    token(&log.report_eid)?;
    crate::recovery_contract::sha256_hex(&log.report_sha256)?;
    if !evidence["coldstart"]["execution_indices"]
        .as_array()
        .is_some_and(|a| a.iter().all(|v| v.as_u64().is_some()))
        || !evidence["coldstart"]["diagnostics"].is_array()
    {
        return Err("invalid coldstart execution indices/diagnostics".into());
    }
    Ok(Some((id.into(), evidence.clone())))
}

/// Validate only packages found in already authenticated actual original events.
/// Old Git source availability is deliberately not a global admission condition.
pub(crate) fn validate_history(
    project_id: &str,
    originals: &BTreeMap<String, Event>,
) -> Result<(), String> {
    let mut expected = BTreeMap::<String, (u64, NewEvent)>::new();
    let mut scan_ids = BTreeSet::new();
    let mut evidence_ids = BTreeSet::new();
    for event in originals
        .values()
        .filter(|e| !e.stale && e.kind == "coldstart.reported")
    {
        envelope(event)?;
        let package: SharedReport = serde_json::from_value(event.data["coldstart_report"].clone())
            .map_err(|e| format!("invalid coldstart report package: {e}"))?;
        if package.schema != 1
            || event.obj != package.scan_id
            || package.input.project_id != project_id
            || !scan_ids.insert(package.scan_id.clone())
        {
            return Err("coldstart report schema/project/scan identity mismatch".into());
        }
        token(&package.scan_id)?;
        chrono::DateTime::parse_from_rfc3339(&package.generated_at).map_err(|e| e.to_string())?;
        let results = aggregate_verification(&package.input, &package.scan, &package.report)?;
        let modules: BTreeSet<_> = results.iter().map(|m| m.module.as_str()).collect();
        if modules != package.evidence_ids.keys().map(String::as_str).collect() {
            return Err("coldstart report module identity map mismatch".into());
        }
        let report_original = crate::recovery_outbox::original_of(event);
        for (module, identity) in &package.evidence_ids {
            token(&identity.evidence_id)?;
            token(&identity.event_eid)?;
            if !evidence_ids.insert(identity.evidence_id.clone())
                || identity.event_eid == event.eid
                || !modules.contains(module.as_str())
            {
                return Err("coldstart report repeats evidence identity".into());
            }
        }
        for evidence in evidence_events(&report_original, &package, &results)? {
            if expected
                .insert(evidence.eid.clone(), (event.seq, evidence))
                .is_some()
            {
                return Err("coldstart report repeats event identity".into());
            }
        }
    }
    for event in originals
        .values()
        .filter(|e| !e.stale && is_coldstart_evidence(e))
    {
        projected_evidence(event)?;
        let (report_seq, expected) = expected
            .get(&event.eid)
            .ok_or("coldstart Evidence lacks accepted report/mapping")?;
        if event.seq <= *report_seq
            || event.host != expected.host
            || event.obj != expected.obj
            || !contains_fields(&event.data, &expected.data)
        {
            return Err(
                "coldstart Evidence differs from report hash/mapping/results/original order".into(),
            );
        }
    }
    // A mapped event may be absent during a partial append, but an existing event
    // with that identity must never silently take another role or body.
    for (eid, (_, expected)) in expected {
        if let Some(event) = originals.get(&eid) {
            if !event.stale && (!is_coldstart_evidence(event) || event.obj != expected.obj) {
                return Err("coldstart mapped identity occupied by another event".into());
            }
        }
    }
    Ok(())
}

fn contains_fields(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual| contains_fields(actual, value))
        }),
        _ => actual == expected,
    }
}

// A recovery constructor, private to core: re-read real project-bound durability
// instead of deserializing a job DTO into a producer capability.
pub(crate) fn restore_recording(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    scan_id: &str,
) -> Result<Option<FrozenColdstart>, String> {
    token(scan_id)?;
    let history = crate::recovery_git::read_recovery_history(catalog, inputs)?;
    let accepted = history
        .coldstart_for_scan(scan_id)
        .map(crate::recovery_outbox::original_of);
    let mut pending = inputs
        .pending()
        .iter()
        .filter(|e| e.kind == "coldstart.reported" && e.obj == scan_id);
    let first = pending.next();
    if pending.next().is_some() {
        return Err("multiple pending coldstart reports for scan".into());
    }
    if accepted.as_ref().zip(first).is_some_and(|(a, b)| a != b) {
        return Err("pending coldstart report conflicts with accepted original".into());
    }
    let Some(report_event) = accepted.or_else(|| first.cloned()) else {
        return Ok(None);
    };
    let package: SharedReport =
        serde_json::from_value(report_event.data["coldstart_report"].clone())
            .map_err(|e| format!("invalid durable coldstart report: {e}"))?;
    if package.schema != 1
        || package.scan_id != scan_id
        || package.input.project_id != catalog.config.project_id()
        || report_event.actor != ACTOR
        || report_event.layer != "L5"
        || !report_event.guards.is_empty()
    {
        return Err("durable coldstart report identity mismatch".into());
    }
    no_authorization(&report_event.data)?;
    token(&report_event.host)?;
    chrono::DateTime::parse_from_rfc3339(&package.generated_at).map_err(|e| e.to_string())?;
    let results = aggregate_verification(&package.input, &package.scan, &package.report)?;
    if results.len() != package.evidence_ids.len() {
        return Err("durable coldstart module map mismatch".into());
    }
    let mut evidence_events = evidence_events(&report_event, &package, &results)?;
    let mut ids = BTreeSet::new();
    for event in &mut evidence_events {
        crate::hub::validate_input(event).map_err(|e| e.to_string())?;
        if !ids.insert(event.obj.clone())
            || !ids.insert(event.eid.clone())
            || event.eid == report_event.eid
        {
            return Err("durable coldstart repeats identity".into());
        }
        let accepted = history
            .coldstart_original(&event.eid)
            .map(crate::recovery_outbox::original_of);
        let pending = inputs.pending().iter().find(|e| e.eid == event.eid);
        if accepted.as_ref().zip(pending).is_some_and(|(a, b)| a != b) {
            return Err("pending coldstart Evidence differs from accepted original".into());
        }
        if let Some(existing) = accepted.or_else(|| pending.cloned()) {
            no_authorization(&existing.data)?;
            let _: VerificationLogRef =
                serde_json::from_value(existing.data["evidence"]["log"].clone())
                    .map_err(|e| e.to_string())?;
            if existing.eid != event.eid
                || existing.host != event.host
                || existing.actor != event.actor
                || existing.kind != event.kind
                || existing.layer != event.layer
                || existing.obj != event.obj
                || existing.guards != event.guards
                || !contains_fields(&existing.data, &event.data)
            {
                return Err("durable coldstart Evidence differs from frozen mapping".into());
            }
            *event = existing;
        }
    }
    inputs.probe(catalog)?;
    Ok(Some(FrozenColdstart {
        report_event,
        evidence_events,
        package,
    }))
}

/// Read-only durable-intent probe for a missing disposable job index. This does
/// not open an outbox writer, a hub, or an execution capability.
pub fn recording_exists(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
    scan_id: &str,
) -> Result<bool, String> {
    Ok(restore_recording(catalog, inputs, scan_id)?.is_some())
}

pub(crate) fn source_diagnostics(
    catalog: &Catalog,
    originals: &BTreeMap<String, Event>,
) -> BTreeMap<String, Vec<String>> {
    let mut diagnostics = BTreeMap::new();
    for report in originals
        .values()
        .filter(|e| !e.stale && e.kind == "coldstart.reported")
    {
        let revision = report.data["coldstart_report"]["input"]["revision"]
            .as_str()
            .unwrap_or_default();
        if crate::repo::git(
            &catalog.root,
            &["cat-file", "-e", &format!("{revision}^{{tree}}")],
        )
        .is_err()
        {
            for event in originals.values().filter(|event| {
                !event.stale
                    && is_coldstart_evidence(event)
                    && event.data["evidence"]["log"]["report_eid"] == report.eid
            }) {
                diagnostics.insert(event.obj.clone(), vec!["source_unavailable: initial verification source cannot be traced in this clone".into()]);
            }
        }
    }
    diagnostics
}

pub fn trace_diagnostics(
    catalog: &Catalog,
    inputs: &crate::recovery_outbox::RecoveryInputs,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    Ok(crate::recovery_git::read_recovery_history(catalog, inputs)?
        .coldstart_diagnostics()
        .clone())
}

/// Retain the complete original receipt across recovery replays. The wrapper is
/// internal recovery audit, not a new verifier assertion or a second confirmation.
pub(crate) fn recovery_receipt(original: &Event, host: &str) -> Result<NewEvent, String> {
    crate::recovery_outbox::validate_actual(
        &crate::recovery_outbox::original_of(original),
        original,
    )?;
    if original.stale || !(original.kind == "coldstart.reported" || is_coldstart_evidence(original))
    {
        return Err("coldstart recovery requires accepted coldstart original".into());
    }
    let digest = crate::recovery_contract::canonical_hash(original)?;
    Ok(NewEvent {
        eid: crate::events::derived_eid(&digest, "coldstart.receipt.restored")?,
        host: host.into(),
        actor: "process:towermd-daemon".into(),
        kind: "coldstart.receipt.restored".into(),
        layer: "L5".into(),
        obj: original.obj.clone(),
        guards: vec![],
        data: json!({"schema":1,"original":original}),
    })
}

pub(crate) fn restored_original(event: &Event) -> Result<Option<Event>, String> {
    if event.kind != "coldstart.receipt.restored" || event.stale {
        return Ok(None);
    }
    if event.actor != "process:towermd-daemon"
        || event.layer != "L5"
        || !event.guards.is_empty()
        || event.epoch.is_some()
        || event.cost_usd.is_some()
        || event.data["schema"] != 1
    {
        return Err("invalid coldstart recovery receipt envelope".into());
    }
    let original = crate::recovery_contract::deserialize_event(event.data["original"].clone())
        .map_err(|e| format!("invalid coldstart original receipt: {e}"))?;
    let canonical = recovery_receipt(&original, &event.host)?;
    if crate::recovery_outbox::original_of(event) != canonical || event.seq <= original.seq {
        return Err("coldstart restored receipt differs from exact original".into());
    }
    Ok(Some(original))
}

#[cfg(test)]
#[path = "coldstart_evidence_tests.rs"]
mod tests;
