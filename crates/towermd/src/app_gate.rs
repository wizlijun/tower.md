//! Fixed local App boundary. This distinguishes the supported native client;
//! it is not cryptographic authentication or evidence of a human decision.
use axum::http::HeaderMap;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AppGateError {
    InstanceChanged,
    AppClientRequired,
    InvalidHumanId,
}
impl std::fmt::Display for AppGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InstanceChanged => "守护进程实例已改变",
            Self::AppClientRequired => "此操作只能由 App 发出",
            Self::InvalidHumanId => "本机人工身份格式非法",
        })
    }
}

/// No deserializer, Default, or public constructor can create this boundary.
pub(crate) struct AppGate {
    _private: (),
}

pub(crate) fn validate_app(
    headers: &HeaderMap,
    expected_instance: &str,
) -> Result<AppGate, AppGateError> {
    if expected_instance.is_empty()
        || headers
            .get("x-towermd-instance")
            .and_then(|value| value.to_str().ok())
            != Some(expected_instance)
    {
        return Err(AppGateError::InstanceChanged);
    }
    if headers
        .get("x-towermd-client")
        .and_then(|value| value.to_str().ok())
        != Some("app")
    {
        return Err(AppGateError::AppClientRequired);
    }
    Ok(AppGate { _private: () })
}

impl AppGate {
    /// The new action wire carries human_id, never a freely supplied actor.
    /// Match the existing shared human actor grammar so recorded facts round trip.
    pub(crate) fn human_actor(&self, human_id: &str) -> Result<String, AppGateError> {
        if human_id.is_empty()
            || human_id.contains([':', '/'])
            || human_id
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(AppGateError::InvalidHumanId);
        }
        Ok(format!("human:{human_id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn instance_and_fixed_app_client_are_both_required() {
        let mut headers = HeaderMap::new();
        assert!(matches!(
            validate_app(&headers, "instance"),
            Err(AppGateError::InstanceChanged)
        ));
        headers.insert("x-towermd-instance", "instance".parse().unwrap());
        assert!(matches!(
            validate_app(&headers, "instance"),
            Err(AppGateError::AppClientRequired)
        ));
        headers.insert("x-towermd-client", "cli".parse().unwrap());
        assert!(matches!(
            validate_app(&headers, "instance"),
            Err(AppGateError::AppClientRequired)
        ));
        headers.insert("x-towermd-client", "app".parse().unwrap());
        assert!(validate_app(&headers, "instance").is_ok());
        assert!(matches!(
            validate_app(&headers, "other-instance"),
            Err(AppGateError::InstanceChanged)
        ));
    }
    #[test]
    fn server_constructs_human_actor_and_rejects_actor_in_human_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-towermd-instance", "instance".parse().unwrap());
        headers.insert("x-towermd-client", "app".parse().unwrap());
        let gate = validate_app(&headers, "instance").unwrap();
        assert_eq!(
            gate.human_actor("local-owner").unwrap(),
            "human:local-owner"
        );
        for id in [
            "",
            "human:owner",
            "process:cli",
            "owner/name",
            "owner name",
            "owner\n",
        ] {
            assert_eq!(gate.human_actor(id), Err(AppGateError::InvalidHumanId));
        }
    }
}
