//! Onboarding and platform-capability presentation.
//!
//! This is the read-only presenter that turns the snapshot read-model plus
//! platform probes into the onboarding wizard and the capability cards. The
//! public surface is intentionally narrow: only the view entry points
//! (`onboarding_view`, `platform_capabilities`, `notification_view`) and the
//! View structs they return are exported. Every `*_step` / `*_capability`
//! helper is private to this module.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{Agent, Config, Mode};
use crate::platform::{self, NotificationCapability, TerminationCapability};
use crate::service::{
    AgentView, ConfigAgentView, ConfigView, RecoveryItemView, SessionView, Snapshot,
};
use crate::usage::SourceReport;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationView {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_test_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityView {
    pub available: bool,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    pub platform: String,
    pub notifications: CapabilityView,
    pub process_capture: CapabilityView,
    pub process_identity: CapabilityView,
    pub enforcement: CapabilityView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnboardingView {
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub mode: String,
    pub action: String,
    pub mode_can_terminate: bool,
    pub detected_providers: Vec<String>,
    pub detected_workers: Vec<String>,
    pub enforceable_agent_types: usize,
    pub watch_only_agent_types: usize,
    pub notifications: NotificationView,
    pub capabilities: PlatformCapabilities,
    pub sources: Vec<SourceReport>,
    pub final_sentence: String,
    pub steps: Vec<OnboardingStepView>,
    pub recovery: Vec<RecoveryItemView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OnboardingStepView {
    pub id: String,
    pub label: String,
    pub status: String,
    pub message: String,
}

pub fn notification_view(
    enabled: bool,
    capability: NotificationCapability,
    last: Option<NotificationView>,
) -> NotificationView {
    let mut view = new_notification_view(enabled, capability);
    if let Some(last) = last {
        view.last_test_at = last.last_test_at;
        view.last_error = last.last_error;
        if view.enabled && view.available && matches!(last.status.as_str(), "delivered" | "error") {
            view.status = last.status;
            view.message = last.message;
            view.available = last.available;
        }
    }
    view
}

fn new_notification_view(enabled: bool, capability: NotificationCapability) -> NotificationView {
    let mut status = capability.status;
    if status == "available" {
        status = "ready".to_string();
    }
    let mut view = NotificationView {
        enabled,
        available: enabled && capability.supported,
        status,
        message: capability.message,
        last_test_at: None,
        last_error: None,
    };
    if !enabled {
        view.status = "disabled".to_string();
        view.message = "local notifications are disabled in Curb policy".to_string();
        view.available = false;
    } else if !capability.supported {
        view.status = "unavailable".to_string();
    }
    view
}

pub fn onboarding_view(
    config: ConfigView,
    required: bool,
    notifications: NotificationView,
    termination: TerminationCapability,
    snapshot: Snapshot,
) -> OnboardingView {
    let enforceable_agent_types = config
        .agents
        .iter()
        .filter(|agent| agent.terminates)
        .count();
    let watch_only_agent_types = config.agents.len().saturating_sub(enforceable_agent_types);
    let capabilities = onboarding_capabilities(
        &config,
        &notifications,
        &termination,
        &snapshot,
        enforceable_agent_types,
    );
    let mode_can_terminate = config.mode == "enforcement"
        && enforceable_agent_types > 0
        && capabilities.enforcement.available;
    let steps = vec![
        config_step(&config),
        agent_step(&config),
        source_step(&snapshot.overview.sources, &capabilities.process_capture),
        notification_step(&config.mode, &notifications),
        safety_step(&config),
    ];
    let recovery = onboarding_recovery(
        &config,
        required,
        &notifications,
        &capabilities,
        &snapshot.sessions,
    );
    OnboardingView {
        required,
        config_path: config.path.clone(),
        mode: config.mode.clone(),
        action: action_label(&config.mode),
        mode_can_terminate,
        detected_providers: detected_providers(&snapshot),
        detected_workers: detected_workers(&snapshot),
        enforceable_agent_types,
        watch_only_agent_types,
        notifications,
        capabilities,
        sources: snapshot.overview.sources,
        final_sentence: onboarding_final_sentence(&config.mode),
        steps,
        recovery,
    }
}

pub fn platform_capabilities(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
    notifications: NotificationView,
    termination: TerminationCapability,
    agents: &[AgentView],
) -> PlatformCapabilities {
    PlatformCapabilities {
        platform: std::env::consts::OS.to_string(),
        notifications: notification_capability_view(&notifications),
        process_capture: process_capture_capability_from_platform(processes, capture_error),
        process_identity: process_identity_capability_from_platform(processes, capture_error),
        enforcement: platform_enforcement_capability(
            cfg,
            processes,
            capture_error,
            &termination,
            agents,
        ),
    }
}

fn onboarding_capabilities(
    config: &ConfigView,
    notifications: &NotificationView,
    termination: &TerminationCapability,
    snapshot: &Snapshot,
    enforceable_agent_types: usize,
) -> PlatformCapabilities {
    PlatformCapabilities {
        platform: std::env::consts::OS.to_string(),
        notifications: notification_capability_view(notifications),
        process_capture: process_capture_capability(&snapshot.overview.sources),
        process_identity: process_identity_capability(snapshot),
        enforcement: enforcement_capability(config, termination, enforceable_agent_types),
    }
}

fn notification_capability_view(notifications: &NotificationView) -> CapabilityView {
    CapabilityView {
        available: notifications.available,
        status: notifications.status.clone(),
        message: notifications.message.clone(),
    }
}

fn process_capture_capability(sources: &[SourceReport]) -> CapabilityView {
    if let Some(error) = sources
        .iter()
        .find(|source| source.provider == "processes")
        .and_then(|source| source.error.clone())
    {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: error,
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: "local process scan is available".to_string(),
    }
}

fn process_capture_capability_from_platform(
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
) -> CapabilityView {
    if let Some(error) = capture_error {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: format!("process capture failed: {error}"),
        };
    }
    let Some(processes) = processes else {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "process capture has not run yet".to_string(),
        };
    };
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: format!(
            "{} captured",
            format_count(processes.processes().count(), "process")
        ),
    }
}

fn process_identity_capability_from_platform(
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
) -> CapabilityView {
    if capture_error.is_some() {
        return CapabilityView {
            available: false,
            status: "error".to_string(),
            message: "process identity unavailable until capture succeeds".to_string(),
        };
    }
    let Some(processes) = processes else {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "process identity has not been sampled yet".to_string(),
        };
    };
    let total = processes.processes().count();
    if total == 0 {
        return CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "no processes captured yet".to_string(),
        };
    }
    let with_identity = processes
        .processes()
        .filter(|process| process.has_termination_identity())
        .count();
    if with_identity == 0 {
        return CapabilityView {
            available: false,
            status: "degraded".to_string(),
            message: "captured processes lack start-time or executable identity evidence"
                .to_string(),
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: format!(
            "{} with identity evidence",
            format_count(with_identity, "process")
        ),
    }
}

fn process_identity_capability(snapshot: &Snapshot) -> CapabilityView {
    let matched = snapshot.agents.iter().filter(|agent| agent.pid > 0).count();
    let revalidatable = snapshot
        .agents
        .iter()
        .filter(|agent| agent.process_started_at.is_some() && agent.pid > 0)
        .count();
    if revalidatable > 0 {
        CapabilityView {
            available: true,
            status: "ready".to_string(),
            message: format!(
                "{} include PID and start time",
                format_count(revalidatable, "matched worker")
            ),
        }
    } else if matched > 0 {
        CapabilityView {
            available: false,
            status: "action".to_string(),
            message: "matched workers are missing process start times; Curb will not stop them"
                .to_string(),
        }
    } else {
        CapabilityView {
            available: false,
            status: "waiting".to_string(),
            message: "no live worker identity evidence yet".to_string(),
        }
    }
}

fn platform_enforcement_capability(
    cfg: &Config,
    processes: Option<&platform::Snapshot>,
    capture_error: Option<&platform::PlatformError>,
    termination: &TerminationCapability,
    agents: &[AgentView],
) -> CapabilityView {
    if cfg.mode != Mode::Enforcement {
        return CapabilityView {
            available: false,
            status: "disabled".to_string(),
            message: "current mode will not terminate processes".to_string(),
        };
    }
    if !termination.supported {
        return CapabilityView {
            available: false,
            status: termination.status.clone(),
            message: termination.message.clone(),
        };
    }
    if !cfg.agents.iter().any(Agent::termination_allowed) {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "no enforceable agent types are configured".to_string(),
        };
    }
    if !process_identity_capability_from_platform(processes, capture_error).available {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "process identity is not strong enough for enforcement".to_string(),
        };
    }
    let enforceable = cfg
        .agents
        .iter()
        .filter(|agent| agent.termination_allowed())
        .map(|agent| agent.id.as_str())
        .collect::<BTreeSet<_>>();
    if !agents.iter().any(|agent| {
        enforceable.contains(agent.id.as_str())
            && agent.pid > 0
            && agent.process_started_at.is_some()
    }) {
        return CapabilityView {
            available: false,
            status: "blocked".to_string(),
            message: "no live enforceable worker is currently matched".to_string(),
        };
    }
    CapabilityView {
        available: true,
        status: "ready".to_string(),
        message: "enforcement can target revalidated worker processes only".to_string(),
    }
}

fn enforcement_capability(
    config: &ConfigView,
    termination: &TerminationCapability,
    enforceable_agent_types: usize,
) -> CapabilityView {
    if config.mode != "enforcement" {
        return CapabilityView {
            available: false,
            status: "disabled".to_string(),
            message: "current mode never terminates processes".to_string(),
        };
    }
    if enforceable_agent_types == 0 {
        return CapabilityView {
            available: false,
            status: "action".to_string(),
            message: "no enforceable worker matchers are configured".to_string(),
        };
    }
    CapabilityView {
        available: termination.supported,
        status: if termination.supported {
            "ready".to_string()
        } else {
            termination.status.clone()
        },
        message: if termination.supported {
            "enforcement can stop only revalidated worker process trees".to_string()
        } else {
            termination.message.clone()
        },
    }
}

fn config_step(config: &ConfigView) -> OnboardingStepView {
    match &config.path {
        Some(path) if !path.is_empty() => step("config", "Config", "done", format!("using {path}")),
        _ => step(
            "config",
            "Config",
            "action",
            "config path is not available".to_string(),
        ),
    }
}

fn agent_step(config: &ConfigView) -> OnboardingStepView {
    if config.agents.is_empty() {
        step(
            "agents",
            "Agents",
            "action",
            "no agent matchers are configured".to_string(),
        )
    } else {
        step(
            "agents",
            "Agents",
            "done",
            agent_count_message(&config.agents),
        )
    }
}

fn source_step(sources: &[SourceReport], capture: &CapabilityView) -> OnboardingStepView {
    if capture.status == "error" {
        return step("sources", "Sources", "action", capture.message.clone());
    }
    if sources.is_empty() {
        return step(
            "sources",
            "Sources",
            "waiting",
            "usage sources have not been scanned yet".to_string(),
        );
    }
    if let Some(source) = sources.iter().find(|source| source.error.is_some()) {
        return step(
            "sources",
            "Sources",
            "action",
            format!(
                "{}: {}",
                source.provider,
                source.error.as_deref().unwrap_or_default()
            ),
        );
    }
    let events = sources.iter().map(|source| source.events).sum::<usize>();
    let files = sources.iter().map(|source| source.files).sum::<usize>();
    if events == 0 {
        return step(
            "sources",
            "Sources",
            "waiting",
            "scanned usage sources; no local usage events found yet".to_string(),
        );
    }
    step(
        "sources",
        "Sources",
        "done",
        format!(
            "{} from {}",
            format_count(events, "usage event"),
            format_count(files, "file")
        ),
    )
}

fn notification_step(mode: &str, notifications: &NotificationView) -> OnboardingStepView {
    if mode == "visibility" {
        return step(
            "notifications",
            "Notifications",
            "waiting",
            "visibility mode records activity without requiring notifications".to_string(),
        );
    }
    if !notifications.enabled {
        return step(
            "notifications",
            "Notifications",
            "action",
            "local notifications are disabled".to_string(),
        );
    }
    if !notifications.available {
        return step(
            "notifications",
            "Notifications",
            "action",
            notifications.message.clone(),
        );
    }
    step(
        "notifications",
        "Notifications",
        "done",
        notifications.message.clone(),
    )
}

fn onboarding_recovery(
    config: &ConfigView,
    required: bool,
    notifications: &NotificationView,
    capabilities: &PlatformCapabilities,
    sessions: &[SessionView],
) -> Vec<RecoveryItemView> {
    let config_path = config.path.clone();
    let mut items = Vec::new();
    if required {
        items.push(recovery_item(
            "setup",
            "First-run setup",
            "required",
            match &config_path {
                Some(path) => {
                    format!("Curb is using safe defaults until setup is confirmed at {path}.")
                }
                None => "Curb is using safe defaults until setup is confirmed.".to_string(),
            },
            command_for_config("curb init", config_path.as_deref()),
            config_path.clone(),
        ));
    }
    if notifications.enabled && !notifications.available {
        items.push(recovery_item(
            "notifications",
            "Notifications",
            &notifications.status,
            notifications.message.clone(),
            command_for_config("curb doctor", config_path.as_deref()),
            config_path.clone(),
        ));
    }
    for capability in [
        (
            "process-capture",
            "Process capture",
            &capabilities.process_capture,
        ),
        (
            "process-identity",
            "Process identity",
            &capabilities.process_identity,
        ),
        ("enforcement", "Enforcement", &capabilities.enforcement),
    ] {
        if !capability.2.available && capability.2.status != "disabled" {
            items.push(recovery_item(
                capability.0,
                capability.1,
                &capability.2.status,
                capability.2.message.clone(),
                command_for_config("curb doctor", config_path.as_deref()),
                config_path.clone(),
            ));
        }
    }
    if sessions
        .iter()
        .any(|session| session.alert != "ok" && session.pid.is_none())
    {
        items.push(recovery_item(
            "process-correlation",
            "Process correlation",
            "uncorrelated",
            "One or more over-limit sessions do not have a sealed live worker identity, so stop is disabled.".to_string(),
            command_for_config("curb scan --json", config_path.as_deref()),
            config_path,
        ));
    }
    items
}

fn recovery_item(
    id: &str,
    label: &str,
    status: &str,
    message: String,
    command: Option<String>,
    path: Option<String>,
) -> RecoveryItemView {
    RecoveryItemView {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        message,
        action: recovery_action(command.as_deref(), path.as_deref()),
        command,
        path,
        runbook: Some("docs/user-guide.md#recovery-surface".to_string()),
    }
}

fn recovery_action(command: Option<&str>, path: Option<&str>) -> String {
    match (command, path) {
        (Some(command), Some(path)) => format!("Run `{command}` and inspect {path}."),
        (Some(command), None) => format!("Run `{command}`."),
        (None, Some(path)) => format!("Inspect {path}."),
        (None, None) => "Open the linked runbook.".to_string(),
    }
}

fn command_for_config(command: &str, config_path: Option<&str>) -> Option<String> {
    Some(match config_path {
        Some(path) if !path.is_empty() => format!("{command} --config {path}"),
        _ => command.to_string(),
    })
}

fn safety_step(config: &ConfigView) -> OnboardingStepView {
    if let Some(agent) = config
        .agents
        .iter()
        .find(|agent| agent.kind == "app" && agent.terminates)
    {
        return step(
            "safety",
            "Safety",
            "action",
            format!("{} is an app root but is enforceable", agent.label),
        );
    }
    step(
        "safety",
        "Safety",
        "done",
        "desktop app roots are watch-only; Curb stops only enforceable workers".to_string(),
    )
}

fn detected_providers(snapshot: &Snapshot) -> Vec<String> {
    let mut providers = Vec::new();
    for provider in snapshot
        .overview
        .sources
        .iter()
        .map(|source| source.provider.as_str())
        .chain(
            snapshot
                .sessions
                .iter()
                .map(|session| session.provider.as_str()),
        )
    {
        push_unique(&mut providers, provider);
    }
    providers
}

fn detected_workers(snapshot: &Snapshot) -> Vec<String> {
    let mut workers = Vec::new();
    for agent in &snapshot.agents {
        let label = if agent.label.is_empty() {
            agent.id.as_str()
        } else {
            agent.label.as_str()
        };
        push_unique(&mut workers, label);
    }
    workers
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !values.iter().any(|seen| seen == value) {
        values.push(value.to_string());
    }
}

fn onboarding_final_sentence(mode: &str) -> String {
    match mode {
        "alert" => "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only.".to_string(),
        "enforcement" => "Curb can stop only correlated enforceable workers after policy and grace checks. Desktop app roots are watch-only.".to_string(),
        _ => "Curb will record local agent activity. It will not notify or stop any process in Visibility mode. Desktop app roots are watch-only.".to_string(),
    }
}

fn action_label(mode: &str) -> String {
    match mode {
        "visibility" => "record only; no warnings or kills",
        "alert" => "notify only; never kill",
        "enforcement" => "enforcement enabled",
        other => other,
    }
    .to_string()
}

fn agent_count_message(agents: &[ConfigAgentView]) -> String {
    let enforceable = agents.iter().filter(|agent| agent.terminates).count();
    let watch_only = agents.len().saturating_sub(enforceable);
    if watch_only == 0 {
        format_count(enforceable, "enforceable agent")
    } else {
        format!(
            "{}, {}",
            format_count(enforceable, "enforceable agent"),
            format_count(watch_only, "watch-only agent")
        )
    }
}

fn format_count(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

fn step(
    id: impl Into<String>,
    label: impl Into<String>,
    status: impl Into<String>,
    message: String,
) -> OnboardingStepView {
    OnboardingStepView {
        id: id.into(),
        label: label.into(),
        status: status.into(),
        message,
    }
}
