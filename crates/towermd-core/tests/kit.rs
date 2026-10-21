use serde_json::json;
use towermd_core::{kit::Kit, project_state};

struct Widget;
impl Kit for Widget {
    fn state_groups(&self) -> &'static [&'static str] {
        &["Widget"]
    }
}

#[test]
fn core_initial_holds_only_core_groups() {
    assert_eq!(
        project_state::core_initial(),
        json!({"Hold":{}, "Proposal":{}, "Prompt":{}, "Alert":{}})
    );
}

#[test]
fn kit_groups_join_the_initial_projection() {
    let state = project_state::initial_with(&[&Widget]);
    assert_eq!(state["Widget"], json!({}));
    assert_eq!(state.as_object().unwrap().len(), 5);
}
