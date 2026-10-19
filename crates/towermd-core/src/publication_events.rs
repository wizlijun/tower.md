//! Typed durable publication facts. Only the writer's actual preparation intent
//! and the shared actual-publication reader can reach these append boundaries.
use super::ProjectSession;
use crate::{
    catalog::Catalog,
    hub_protocol::{AppendReceipt, Event, HubError, NewEvent},
    recovery_contract::{
        self,
        recorded::{
            BaselineCreated, PublicationPrepareAction, PublicationPrepared,
            PublicationPreparedResult, RequestBody, RequestMeta, RequestReceipt,
        },
    },
    recovery_git::{self, VerifiedPublication},
    recovery_outbox::{self, RecoveryOutbox},
    request_ledger::RequestIdentity,
    snapshot_service::PublicationPreparation,
};
use std::sync::Mutex;

pub(crate) struct RecordedPublicationPreparation {
    original: Event,
}
impl RecordedPublicationPreparation {
    pub(crate) fn original(&self) -> &Event {
        &self.original
    }
}

/// Exactly the two shared internal actions, never arbitrary Event production.
pub(crate) fn decode_request(project: &str, event: &NewEvent) -> Result<RequestIdentity, String> {
    recovery_contract::validate_recorded_data(project, &event.kind, &event.data)?;
    if event.layer != "L1" || event.actor != "process:towermd-baseline" {
        return Err("publication request differs from shared layer/actor envelope".into());
    }
    match event.kind.as_str() {
        "baseline.publication_prepared" => {
            let record: PublicationPrepared =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if event.eid != record.p3_receipt.result_ref.prepared_eid
                || event.host != record.request_body.host
                || event.obj != record.publication_id
                || event.guards.len() != 1
                || event.guards[0].name != "main"
            {
                return Err("publication preparation differs from original envelope/fence".into());
            }
            Ok(RequestIdentity::Preparation {
                body: record.request_body,
                meta: record.p3_request,
            })
        }
        "baseline.created" => {
            let record: BaselineCreated =
                serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
            if event.eid != crate::events::derived_eid(&record.result.commit, "baseline.created")?
                || event.host != record.request_body.host
                || event.obj != record.result.publication_id
                || !event.guards.is_empty()
            {
                return Err("baseline created differs from canonical shared envelope".into());
            }
            Ok(RequestIdentity::Created {
                body: record.request_body,
                meta: record.p3_request,
            })
        }
        _ => Err("not a supported typed publication request".into()),
    }
}

fn planned(
    intent: &PublicationPreparation,
    project: &str,
) -> Result<(RequestIdentity, NewEvent), String> {
    let request_id = uuid::Uuid::now_v7().to_string();
    let body = RequestBody {
        project_id: project.into(),
        actor: "process:towermd-baseline".into(),
        host: intent.host().into(),
        run: None,
        action: PublicationPrepareAction::PreparePublication {
            publication_id: intent.approval_eid().into(),
            approval_eid: intent.approval_eid().into(),
            binding: intent.binding().clone(),
            authority_revision: intent.authority_revision().into(),
            content_hash: intent.binding().content_hash.clone(),
        },
    };
    let meta = RequestMeta {
        request_id: request_id.clone(),
        body_hash: recovery_contract::canonical_hash(&body)?,
    };
    let eid = recovery_contract::recorded::request_event_eid(
        &request_id,
        "baseline.publication_prepared",
    )?;
    let result = PublicationPreparedResult {
        prepared_eid: eid.clone(),
        publication_id: intent.approval_eid().into(),
        authority_revision: intent.authority_revision().into(),
        binding: intent.binding().clone(),
        content_hash: intent.binding().content_hash.clone(),
    };
    let record = PublicationPrepared {
        publication_id: intent.approval_eid().into(),
        approval_eid: intent.approval_eid().into(),
        binding: intent.binding().clone(),
        authority_revision: intent.authority_revision().into(),
        content_hash: intent.binding().content_hash.clone(),
        request_body: body,
        p3_request: meta.clone(),
        p3_receipt: RequestReceipt {
            request_id,
            body_hash: meta.body_hash,
            result_ref: result,
        },
    };
    let event = NewEvent {
        eid,
        host: intent.host().into(),
        actor: "process:towermd-baseline".into(),
        kind: "baseline.publication_prepared".into(),
        layer: "L1".into(),
        obj: intent.approval_eid().into(),
        guards: vec![intent.fence().clone()],
        data: serde_json::to_value(record).map_err(|e| e.to_string())?,
    };
    let request = decode_request(project, &event)?;
    Ok((request, event))
}

fn fresh(
    live: &mut ProjectSession,
    intent: &PublicationPreparation,
    now: u64,
) -> Result<(), HubError> {
    let head = live.hub.subscribe(live.cursor, now)?.head;
    if !head
        .leases
        .get("main")
        .is_some_and(|lease| lease.fence() == *intent.fence() && lease.expires_at > now)
    {
        return Err(HubError::Invalid(
            "publication preparation requires the actual fresh main fence".into(),
        ));
    }
    Ok(())
}

impl ProjectSession {
    pub(crate) fn prepare_publication_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        intent: &PublicationPreparation,
        now: u64,
    ) -> Result<RecordedPublicationPreparation, String> {
        if intent.authority_revision() != catalog.revision
            || intent.binding().project_id != catalog.config.project_id()
        {
            return Err("publication intent differs from current project/authority".into());
        }
        let inputs = outbox.read_inputs()?;
        let history = recovery_git::read_recovery_history(catalog, &inputs)?;
        let approval = history
            .baseline_review_original(intent.approval_eid())
            .ok_or("publication lacks actual original approval")?;
        if approval != intent.approval()
            || approval.stale
            || !approval.guards.is_empty()
            || approval.kind != "baseline.approved"
        {
            return Err("publication intent differs from original active approval".into());
        }
        let record: recovery_contract::recorded::BaselineReviewRecorded =
            serde_json::from_value(approval.data.clone()).map_err(|e| e.to_string())?;
        recovery_contract::validate_recorded_data(
            catalog.config.project_id(),
            "baseline.approved",
            &approval.data,
        )?;
        if approval.layer != "L1"
            || approval.obj != intent.binding().object_id
            || approval.actor != record.request_body.actor
            || approval.host != record.request_body.host
            || record.approval.binding != *intent.binding()
        {
            return Err("publication approval differs from exact shared binding/envelope".into());
        }
        let mut request_id = intent.approval_eid().to_string();
        let result=Self::continue_shared(session,catalog,&inputs,now,|live| {
            let attempts=live.baseline_requests.as_ref().ok_or_else(||HubError::Invalid("request history is not prepared".into()))?.preparations(intent.approval_eid());
            let mut matching=Vec::new();
            for (identity, original) in attempts {
                let record:PublicationPrepared=serde_json::from_value(original.data.clone()).map_err(|e|HubError::Invalid(e.to_string()))?;
                if record.binding!=*intent.binding() || record.content_hash!=intent.binding().content_hash || record.approval_eid!=intent.approval_eid() {
                    return Err(HubError::Invalid("publication attempt changed original approval/content".into()));
                }
                request_id=record.p3_request.request_id;
                // Reconcile the original exact guards and actual receipt before
                // considering a new authority/fence attempt. Never rebind it.
                let actual=live.baseline_requests.as_ref().unwrap().retry_original(catalog,&identity,&mut *live.hub,outbox,now)?
                    .ok_or_else(||HubError::Uncertain("original publication attempt lacks receipt".into()))?;
                if record.authority_revision==intent.authority_revision() && original.guards==[intent.fence().clone()] {
                    if original.host!=intent.host() { return Err(HubError::Invalid("same publication authority/fence cannot change original host".into())); }
                    if actual.stale { return Err(HubError::Invalid("original publication attempt is stale; a new authority/fence is required".into())); }
                    matching.push(actual);
                }
            }
            fresh(live,intent,now)?;
            if !matching.is_empty() {
                matching.sort_by_key(|event|event.seq);
                let selected = matching.remove(0);
                request_id = selected.data["p3_request"]["request_id"].as_str().ok_or_else(|| HubError::Invalid("publication preparation lost its typed request id".into()))?.to_owned();
                return Ok(Some(selected));
            }
            let (identity,event)=planned(intent,catalog.config.project_id()).map_err(HubError::Invalid)?;
            let RequestIdentity::Preparation{meta,..}=&identity else {unreachable!()};
            request_id=meta.request_id.clone();
            live.baseline_requests.as_mut().unwrap().reserve(identity)?;
            match recovery_outbox::append_durable(&mut *live.hub,outbox,event,now)? {
                AppendReceipt::Event(actual) if !actual.stale => Ok(Some(*actual)),
                AppendReceipt::Event(_) => Err(HubError::Invalid("publication preparation accepted as stale".into())),
                AppendReceipt::Duplicate{..}=>Err(HubError::Uncertain("publication preparation lacks actual receipt".into())),
            }
        }).map_err(|e|e.to_string())?;
        let original = Self::finish_baseline_request_shared(
            session,
            catalog,
            outbox,
            &request_id,
            result,
            now,
        )?
        .ok_or("publication preparation did not produce an actual receipt")?;
        Ok(RecordedPublicationPreparation { original })
    }

    pub(crate) fn record_publication_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        proof: &VerifiedPublication,
        now: u64,
    ) -> Result<BaselineCreated, String> {
        Self::record_publication_at_prefix_shared(session, catalog, outbox, proof, None, now)
    }

    pub(crate) fn record_publication_at_prefix_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        proof: &VerifiedPublication,
        expected_prefix: Option<u64>,
        now: u64,
    ) -> Result<BaselineCreated, String> {
        let event = recovery_git::baseline_created_event(proof)?;
        let identity = decode_request(catalog.config.project_id(), &event)?;
        let expected: BaselineCreated =
            serde_json::from_value(event.data.clone()).map_err(|e| e.to_string())?;
        let inputs = outbox.read_inputs()?;
        // Recheck actual main/tag/commit and project context outside the lock.
        let history = recovery_git::read_recovery_history(catalog, &inputs)?;
        let current = recovery_git::prove_publication(catalog, &history, &expected.result.commit)?;
        if recovery_git::baseline_created_event(&current)? != event {
            return Err("publication proof changed before recording".into());
        }
        let result = Self::continue_shared(session, catalog, &inputs, now, |live| {
            let ledger = live
                .baseline_requests
                .as_mut()
                .ok_or_else(|| HubError::Invalid("request history is not prepared".into()))?;
            ledger
                .reserve(identity.clone())
                .map_err(HubError::Invalid)?;
            if let Some(actual) =
                ledger.retry_original(catalog, &identity, &mut *live.hub, outbox, now)?
            {
                if recovery_outbox::original_of(&actual) != event {
                    return Err(HubError::Invalid(
                        "canonical publication differs from original accepted event".into(),
                    ));
                }
                return Ok(Some(actual));
            }
            if expected_prefix.is_some_and(|q| live.hub.contiguous_seq() != q) {
                return Err(HubError::Uncertain("publication population Q changed before canonical creation; save a fresh prefix".into()));
            }
            match recovery_outbox::append_durable(&mut *live.hub, outbox, event.clone(), now)? {
                AppendReceipt::Event(actual) => {
                    if expected_prefix.is_some_and(|q| q.checked_add(1) != Some(actual.seq)) {
                        return Err(HubError::Uncertain("publication creation did not immediately follow its durable Q".into()));
                    }
                    Ok(Some(*actual))
                },
                AppendReceipt::Duplicate { .. } => Err(HubError::Uncertain(
                    "publication completion lacks actual receipt".into(),
                )),
            }
        })
        .map_err(|e| e.to_string())?;
        let actual = Self::finish_baseline_request_shared(
            session,
            catalog,
            outbox,
            &expected.p3_request.request_id,
            result,
            now,
        )?
        .ok_or("publication completion did not produce an actual receipt")?;
        if actual.stale || recovery_outbox::original_of(&actual) != event {
            return Err("publication completion differs from canonical actual event".into());
        }
        serde_json::from_value(actual.data).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
#[path = "publication_events_tests.rs"]
mod tests;

impl ProjectSession {
    pub(crate) fn record_publication_rechecks_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        outbox: &mut RecoveryOutbox,
        plan: &recovery_git::PublicationRecheckPlan,
        created: &Event,
        now: u64,
    ) -> Result<(), String> {
        if created.stale
            || recovery_outbox::original_of(created) != plan.created
            || plan.through_seq.checked_add(1) != Some(created.seq)
        {
            return Err(
                "task recheck requires the exact original S immediately after durable Q".into(),
            );
        }
        let inputs = outbox.read_inputs()?;
        let mut originals = Vec::new();
        for fact in &plan.facts {
            if !plan.tasks.contains_key(&fact.task) {
                return Err("recheck fact lacks actual S Task".into());
            }
            let original = crate::proposals::required_event(
                catalog.config.project_id(),
                fact,
                &plan.source_commit,
                created,
            )?;
            let record = crate::proposals::decode_required(catalog.config.project_id(), &original)?;
            let identity = RequestIdentity::RecheckRequired {
                body: record.request_body,
                meta: record.p3_request,
            };
            originals.push((original, identity));
        }
        Self::continue_shared(session, catalog, &inputs, now, |live| {
            let ledger = live
                .baseline_requests
                .as_mut()
                .ok_or("recheck request ledger is not prepared")?;
            for (original, identity) in originals {
                ledger.reserve(identity.clone())?;
                if let Some(actual) = ledger
                    .retry_original(catalog, &identity, &mut *live.hub, outbox, now)
                    .map_err(|error| error.to_string())?
                {
                    if recovery_outbox::original_of(&actual) != original {
                        return Err("recheck original publication/source body changed".into());
                    }
                } else {
                    match recovery_outbox::append_durable(&mut *live.hub, outbox, original, now)
                        .map_err(|error| error.to_string())?
                    {
                        AppendReceipt::Event(actual) if !actual.stale => {}
                        _ => {
                            return Err(
                                "task recheck marker lacks its actual accepted receipt".into()
                            )
                        }
                    }
                }
            }
            Ok::<_, String>(())
        })
        .map_err(|error| error.to_string())??;
        Self::sync_shared(session, catalog, &outbox.read_inputs()?, now)
            .map_err(|error| error.to_string())
    }
}

impl ProjectSession {
    /// Observe only this canonical Git publication's actual accepted tail role.
    /// This may repair confirmed-fsync uncertainty; it cannot invent a new S.
    pub(crate) fn publication_actual_shared(
        session: &Mutex<Self>,
        catalog: &Catalog,
        inputs: &crate::recovery_outbox::RecoveryInputs,
        canonical: &NewEvent,
        now: u64,
    ) -> Result<Option<Event>, String> {
        if canonical.kind != "baseline.created" {
            return Err("not a canonical publication request".into());
        }
        Self::continue_shared(session, catalog, inputs, now, |live| {
            let matching = live
                .tail
                .iter()
                .filter(|event| event.eid == canonical.eid)
                .collect::<Vec<_>>();
            if matching.len() > 1 {
                return Err("multiple actual publication sequence roles".into());
            }
            let Some(actual) = matching.first() else {
                return Ok(None);
            };
            if recovery_outbox::original_of(actual) != *canonical || actual.stale {
                return Err("actual publication differs from its canonical Git proof".into());
            }
            Ok(Some((**actual).clone()))
        })
        .map_err(|error| error.to_string())?
    }
}
