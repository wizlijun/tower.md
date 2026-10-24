//! Gate enforcement (core spec FR-120–124). The declaration is read from the
//! authoritative main, never from the working tree a delegate can edit; a
//! broken declaration blocks. Where a project declares gates, each judgement
//! made at an enforcement point is recorded as `kind: gate` evidence. The
//! waivers in force at that moment (FR-72) are part of the judgement, so the
//! evidence names every waiver it relied on.
use super::business::now;
use super::*;
use towermd_core::{gates::{self, GateFile, Judgement, Recorded}, hub_protocol::NewEvent, project_runtime::ProjectRuntime, quiet::instant, waivers};
use towermd_devkit::dev_state::KITS;

/// The judgement of `object` against main's `gates.yaml`, recorded when the
/// project declares gates and `record` holds or the gates block.
pub(super) fn enforce(server:&Server,runtime:&mut ProjectRuntime,catalog:&Catalog,object:&str,stage:&str,record:bool)->Result<Judgement,String> {
    let file=GateFile::from_text(catalog.read_file(gates::GATES_PATH)?.as_deref()).waive(in_force(runtime.projection())?);
    let judgement=gates::judge_object(runtime.projection(),KITS,&file,object)?;
    if !file.is_empty() && (record || judgement.declared_blocks().is_some()) {
        let data=Recorded::Gate{object:object.into(),version:judgement.version.clone(),stage:stage.into(),
            revision:Some(catalog.revision.clone()),verdict:judgement.verdict,gates:judgement.gates.clone(),error:judgement.error.clone()};
        let receipt=runtime.append(NewEvent{eid:Uuid::now_v7().to_string(),host:server.endpoint.host.clone(),actor:gates::TOWER.into(),
            kind:gates::RECORDED.into(),layer:gates::LAYER.into(),obj:object.into(),
            data:serde_json::to_value(data).map_err(|e|e.to_string())?,guards:vec![]},now).map_err(|e|e.to_string())?;
        if receipt.stale {return Err("门禁证据未生效".into());}
    }
    Ok(judgement)
}

/// The waivers in force now; a lapsed one is simply left out.
pub(super) fn in_force(projection:&Value)->Result<Vec<waivers::Waiver>,String> {
    Ok(waivers::active(&waivers::read(projection)?,instant(now())))
}

/// Refuses with `gate_failed` when `gates.yaml` blocks `object`.
pub(super) fn require_clear(judgement:&Judgement)->Result<(),ApiError> {
    match judgement.declared_blocks() {
        Some(reason)=>Err(rejected_as(ErrorCode::GateFailed)(reason)),
        None=>Ok(()),
    }
}
