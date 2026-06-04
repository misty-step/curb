use crate::platform;
use crate::service::{ServiceError, StopExpectedIdentity};

pub(super) fn validate_expected_stop_identity(
    expected: &StopExpectedIdentity,
) -> Result<(), ServiceError> {
    if expected.pid == 0 {
        return Err(ServiceError::InvalidStop(
            "expected pid is required".to_string(),
        ));
    }
    if expected.started_at.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected process start time is required".to_string(),
        ));
    }
    if expected.owner.is_empty() {
        return Err(ServiceError::InvalidStop(
            "expected owner is required".to_string(),
        ));
    }
    if expected.executable.is_none() && expected.bundle_id.is_none() && expected.team_id.is_none() {
        return Err(ServiceError::InvalidStop(
            "expected executable or app identity is required".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_stop_expectation(
    expected: &StopExpectedIdentity,
    actual: &platform::Process,
) -> Result<(), ServiceError> {
    if actual.pid.get() != expected.pid {
        return Err(ServiceError::StopConflict("pid changed".to_string()));
    }
    if actual.started_at != expected.started_at {
        return Err(ServiceError::StopConflict(
            "process start time changed".to_string(),
        ));
    }
    if actual.username.as_deref() != Some(expected.owner.as_str()) {
        return Err(ServiceError::StopConflict(
            "process owner changed".to_string(),
        ));
    }
    if let Some(executable) = &expected.executable
        && actual.executable.as_ref() != Some(executable)
    {
        return Err(ServiceError::StopConflict("executable changed".to_string()));
    }
    if let Some(bundle_id) = &expected.bundle_id
        && actual.bundle_id.as_ref() != Some(bundle_id)
    {
        return Err(ServiceError::StopConflict("bundle id changed".to_string()));
    }
    if let Some(team_id) = &expected.team_id
        && actual.team_id.as_ref() != Some(team_id)
    {
        return Err(ServiceError::StopConflict("team id changed".to_string()));
    }
    if !actual.has_termination_identity() {
        return Err(ServiceError::StopConflict(
            "process identity is incomplete".to_string(),
        ));
    }
    Ok(())
}
