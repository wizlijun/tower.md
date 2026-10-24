//! Question projection adapter. All durable fields are derived from canonical
//! events; the top-level groups are the same groups consumed by the canvas.
use serde_json::{json, Value};
use crate::{hub::Event, question_model::QuestionProjection};

/// Written only once a prompt is handled, so older logs replay byte-identically.
pub const HANDLED: &str = "PromptHandled";

pub fn read(state: &Value) -> Result<QuestionProjection, String> {
    let fields = [
        ("holds", "Hold"), ("answers", "Answer"),
        ("decisions", "QuestionDecision"), ("prompts", "Prompt"),
        ("prompt_actors", "PromptActor"), ("deliveries", "Delivery"),
        ("proposals", "QuestionProposal"), ("rules", "QuestionRule"),
        ("handled", HANDLED),
    ];
    let mut value = serde_json::Map::new();
    for (field, group) in fields {
        value.insert(field.into(), state.get(group).cloned().unwrap_or_else(|| json!({})));
    }
    serde_json::from_value(Value::Object(value)).map_err(|e| e.to_string())
}

pub fn apply(state: &mut Value, event: &Event) -> Result<(), String> {
    let mut projection = read(state)?;
    projection.apply(event)?;
    // Stage every derived field before replacing the projection, including
    // proposals already owned by other business reducers.
    let mut next = state.clone();
    next["Hold"] = serde_json::to_value(&projection.holds).map_err(|e| e.to_string())?;
    next["Answer"] = serde_json::to_value(&projection.answers).map_err(|e| e.to_string())?;
    next["QuestionDecision"] = serde_json::to_value(&projection.decisions).map_err(|e| e.to_string())?;
    next["Prompt"] = serde_json::to_value(&projection.prompts).map_err(|e| e.to_string())?;
    next["PromptActor"] = serde_json::to_value(&projection.prompt_actors).map_err(|e| e.to_string())?;
    next["Delivery"] = serde_json::to_value(&projection.deliveries).map_err(|e| e.to_string())?;
    next["QuestionProposal"] = serde_json::to_value(&projection.proposals).map_err(|e| e.to_string())?;
    next["QuestionRule"] = serde_json::to_value(&projection.rules).map_err(|e| e.to_string())?;
    if !projection.handled.is_empty() || next.get(HANDLED).is_some() {
        next[HANDLED] = serde_json::to_value(&projection.handled).map_err(|e| e.to_string())?;
    }
    if next.get("Proposal").is_none() { next["Proposal"] = json!({}); }
    let proposals = next["Proposal"].as_object_mut().ok_or("提案投影不是对象")?;
    for (id, proposal) in projection.proposals {
        let value = json!({"type":"Proposal","id":id,"status":"draft",
            "source_event":proposal.source_event,"text":proposal.text,"targets":proposal.targets});
        if let Some(old) = proposals.get(&id) {
            if old["source_event"] != value["source_event"] || old["text"] != value["text"] || old["targets"] != value["targets"] {
                return Err("问答提案与已有提案 ID 冲突".into());
            }
            // Its independent approval lifecycle may already have advanced.
        } else { proposals.insert(id, value); }
    }
    *state = next;
    Ok(())
}

/// A timeout changes the waiting protocol, never the need for a decision.
/// Producers consult this alongside task gates without rewriting task lifecycle
/// state (which may independently be released, paused or require review).
pub fn blocks_run(state: &Value, run: &str) -> Result<bool, String> {
    let projection = read(state)?;
    Ok(projection.holds.iter().any(|(id, question)| {
        question.binding.run == run
            && projection.answers.get(id).is_none_or(|answer| !answer.effective())
    }))
}

/// Releasing and reclaiming a task cannot discard its outstanding questions.
pub fn blocks_task(state: &Value, task: &str) -> Result<bool, String> {
    let projection = read(state)?;
    Ok(projection.holds.iter().any(|(id, question)| {
        question.binding.task == task
            && projection.answers.get(id).is_none_or(|answer| !answer.effective())
    }))
}
