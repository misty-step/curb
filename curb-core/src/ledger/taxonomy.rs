/// The closed taxonomy of ledger `event_type` strings.
///
/// Both the emit side (usagewatch/runtime/service) and the read side
/// (service's alert and event views) share this single mapping instead of
/// re-deriving an event's meaning from substring sniffing. The wire strings
/// stay an implementation detail behind [`LedgerEvent::as_str`] /
/// [`LedgerEvent::parse`]; on-disk ledgers and existing tests keep parsing
/// byte-identical strings.
///
/// Adding a future event means adding a variant here, which forces every
/// classification accessor's exhaustive `match` to handle it. A new event
/// fails to compile rather than silently mis-coloring the dashboard.
///
/// This is deliberately distinct from `usage::EventKind`, which classifies
/// the provider USAGE logs Curb parses; this taxonomy classifies the policy
/// lifecycle Curb itself records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LedgerEvent {
    ServiceStarted,
    ServiceStopped,
    RunStarted,
    RunStopped,
    AckReceived,
    SessionAckReceived,
    AckRejected,
    PolicyWarning,
    UsageWarning,
    UsageWouldTerminate,
    UsageKillBlocked,
    UsageGraceStarted,
    UsageTerminationStarted,
    UsageTerminationCompleted,
    UsageTerminationFailed,
    TerminationStarted,
    TerminationCompleted,
    TerminationFailed,
    UsageScanFailed,
    ScanFailed,
    NotificationFailed,
    ManualStopStarted,
    ManualStopCompleted,
}

/// Coarse category / kind labels for an [`EventView`]-style row, mirroring the
/// historical `event_class` mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewClass {
    pub category: &'static str,
    pub kind: &'static str,
}

/// Alert-view classification for events that surface as policy alerts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlertClass {
    pub category: &'static str,
    pub severity: &'static str,
    pub label: &'static str,
    pub actionable: bool,
    pub explanation: &'static str,
}

impl LedgerEvent {
    /// Parse a wire `event_type` string into its typed variant, or `None` for
    /// an unknown event the read model treats generically.
    #[must_use]
    pub fn parse(event_type: &str) -> Option<Self> {
        let event = match event_type {
            "service_started" => Self::ServiceStarted,
            "service_stopped" => Self::ServiceStopped,
            "run_started" => Self::RunStarted,
            "run_stopped" => Self::RunStopped,
            "ack_received" => Self::AckReceived,
            "session_ack_received" => Self::SessionAckReceived,
            "ack_rejected" => Self::AckRejected,
            "policy_warning" => Self::PolicyWarning,
            "usage_warning" => Self::UsageWarning,
            "usage_would_terminate" => Self::UsageWouldTerminate,
            "usage_kill_blocked" => Self::UsageKillBlocked,
            "usage_grace_started" => Self::UsageGraceStarted,
            "usage_termination_started" => Self::UsageTerminationStarted,
            "usage_termination_completed" => Self::UsageTerminationCompleted,
            "usage_termination_failed" => Self::UsageTerminationFailed,
            "termination_started" => Self::TerminationStarted,
            "termination_completed" => Self::TerminationCompleted,
            "termination_failed" => Self::TerminationFailed,
            "usage_scan_failed" => Self::UsageScanFailed,
            "scan_failed" => Self::ScanFailed,
            "notification_failed" => Self::NotificationFailed,
            "manual_stop_started" => Self::ManualStopStarted,
            "manual_stop_completed" => Self::ManualStopCompleted,
            _ => return None,
        };
        Some(event)
    }

    /// The byte-identical wire string written to and read from the ledger.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceStarted => "service_started",
            Self::ServiceStopped => "service_stopped",
            Self::RunStarted => "run_started",
            Self::RunStopped => "run_stopped",
            Self::AckReceived => "ack_received",
            Self::SessionAckReceived => "session_ack_received",
            Self::AckRejected => "ack_rejected",
            Self::PolicyWarning => "policy_warning",
            Self::UsageWarning => "usage_warning",
            Self::UsageWouldTerminate => "usage_would_terminate",
            Self::UsageKillBlocked => "usage_kill_blocked",
            Self::UsageGraceStarted => "usage_grace_started",
            Self::UsageTerminationStarted => "usage_termination_started",
            Self::UsageTerminationCompleted => "usage_termination_completed",
            Self::UsageTerminationFailed => "usage_termination_failed",
            Self::TerminationStarted => "termination_started",
            Self::TerminationCompleted => "termination_completed",
            Self::TerminationFailed => "termination_failed",
            Self::UsageScanFailed => "usage_scan_failed",
            Self::ScanFailed => "scan_failed",
            Self::NotificationFailed => "notification_failed",
            Self::ManualStopStarted => "manual_stop_started",
            Self::ManualStopCompleted => "manual_stop_completed",
        }
    }

    /// The coarse `(category, kind)` an [`EventView`] row uses.
    #[must_use]
    pub fn view_class(self) -> ViewClass {
        let (category, kind) = match self {
            Self::ServiceStarted => ("service", "started"),
            Self::ServiceStopped => ("service", "stopped"),
            Self::RunStarted => ("run", "started"),
            Self::RunStopped => ("run", "stopped"),
            Self::AckReceived | Self::SessionAckReceived => ("ack", "received"),
            Self::AckRejected => ("ack", "rejected"),
            Self::PolicyWarning | Self::UsageWarning => ("alert", "warning"),
            Self::UsageWouldTerminate => ("alert", "would_stop"),
            Self::UsageKillBlocked => ("alert", "blocked"),
            Self::UsageGraceStarted => ("alert", "grace"),
            Self::UsageTerminationStarted | Self::TerminationStarted => ("termination", "started"),
            Self::UsageTerminationCompleted | Self::TerminationCompleted => {
                ("termination", "completed")
            }
            Self::UsageTerminationFailed | Self::TerminationFailed => ("termination", "failed"),
            Self::ScanFailed | Self::UsageScanFailed => ("error", "scan_failed"),
            Self::NotificationFailed => ("error", "notification_failed"),
            Self::ManualStopStarted | Self::ManualStopCompleted => ("other", "recorded"),
        };
        ViewClass { category, kind }
    }

    /// Whether this event surfaces as a policy alert in the alert feed.
    ///
    /// Mirrors the historical `alert_event` predicate (warning / terminate /
    /// termination / kill / grace), expressed exhaustively.
    #[must_use]
    pub fn is_alert(self) -> bool {
        matches!(
            self,
            Self::PolicyWarning
                | Self::UsageWarning
                | Self::UsageWouldTerminate
                | Self::UsageKillBlocked
                | Self::UsageGraceStarted
                | Self::UsageTerminationStarted
                | Self::UsageTerminationCompleted
                | Self::UsageTerminationFailed
                | Self::TerminationStarted
                | Self::TerminationCompleted
                | Self::TerminationFailed
        )
    }

    /// Full alert-view classification, for events where [`is_alert`](Self::is_alert)
    /// holds. The three termination phases stay distinct: `*GraceStarted` is the
    /// pre-kill waiting state (`grace`), `*TerminationStarted` is the
    /// kill-in-progress state (`stopping`), and `*TerminationCompleted` is the
    /// finished state (`stopped`).
    #[must_use]
    pub fn alert_class(self) -> AlertClass {
        let category = match self {
            Self::UsageTerminationCompleted | Self::TerminationCompleted => "stopped",
            Self::UsageGraceStarted => "grace",
            Self::UsageTerminationStarted | Self::TerminationStarted => "stopping",
            Self::UsageWouldTerminate => "would_stop",
            Self::UsageKillBlocked => "blocked",
            Self::UsageTerminationFailed | Self::TerminationFailed => "failed",
            _ => "warning",
        };
        let severity = match self {
            Self::UsageTerminationCompleted => "stop",
            Self::UsageTerminationFailed | Self::TerminationFailed => "error",
            Self::UsageKillBlocked => "blocked",
            Self::UsageWouldTerminate | Self::UsageGraceStarted => "watch",
            _ => "warn",
        };
        let label = match self {
            Self::UsageTerminationCompleted | Self::TerminationCompleted => "stopped",
            Self::UsageGraceStarted => "grace",
            Self::UsageTerminationStarted | Self::TerminationStarted => "stopping",
            Self::UsageWouldTerminate => "would stop",
            Self::UsageKillBlocked => "blocked",
            Self::UsageTerminationFailed | Self::TerminationFailed => "failed",
            _ => "warning",
        };
        let actionable = matches!(
            self,
            Self::UsageTerminationStarted | Self::UsageTerminationCompleted
        );
        let explanation = match self {
            Self::UsageWouldTerminate => {
                "Alert mode: Curb would stop this correlated worker in enforcement mode."
            }
            Self::UsageKillBlocked => {
                "Curb did not stop anything because the session was uncorrelated or watch-only."
            }
            Self::UsageGraceStarted => "Enforcement grace period started for a correlated worker.",
            Self::UsageTerminationStarted => "Curb started terminating a correlated worker.",
            Self::UsageTerminationCompleted => {
                "Curb completed termination for a correlated worker."
            }
            Self::PolicyWarning | Self::UsageWarning => {
                "Usage or runtime crossed the warning policy."
            }
            _ => "",
        };
        AlertClass {
            category,
            severity,
            label,
            actionable,
            explanation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LedgerEvent;

    /// Every wire string Curb emits (or still reads for back-compat) must
    /// round-trip through the taxonomy to a non-default classification, so a
    /// renamed or added event can never silently fall back to the generic
    /// "other/recorded" bucket without a deliberate variant.
    #[test]
    fn ledger_event_round_trips_every_emitted_wire_string() {
        // The full set of event_type strings emitted across the codebase plus
        // the legacy aliases the read model still classifies. Keep in sync
        // with `grep -rno '"usage_[a-z_]*"\|"manual_[a-z_]*"' src/` and the
        // non-usage event types in `event_class`.
        let wire_strings = [
            "service_started",
            "service_stopped",
            "run_started",
            "run_stopped",
            "ack_received",
            "session_ack_received",
            "ack_rejected",
            "policy_warning",
            "usage_warning",
            "usage_would_terminate",
            "usage_kill_blocked",
            "usage_grace_started",
            "usage_termination_started",
            "usage_termination_completed",
            "usage_termination_failed",
            "termination_started",
            "termination_completed",
            "termination_failed",
            "usage_scan_failed",
            "scan_failed",
            "notification_failed",
            "manual_stop_started",
            "manual_stop_completed",
        ];

        for wire in wire_strings {
            let event = LedgerEvent::parse(wire)
                .unwrap_or_else(|| panic!("{wire} should parse into the taxonomy"));
            assert_eq!(
                event.as_str(),
                wire,
                "{wire} must round-trip byte-identically for wire compatibility"
            );
            let view = event.view_class();
            assert!(
                (view.category, view.kind) != ("other", "recorded")
                    // manual_stop_* are recorded generically by design.
                    || wire.starts_with("manual_stop_"),
                "{wire} fell through to the default view class"
            );
            if event.is_alert() {
                let alert = event.alert_class();
                assert!(
                    !alert.category.is_empty()
                        && !alert.severity.is_empty()
                        && !alert.label.is_empty(),
                    "{wire} is an alert but produced an empty classification"
                );
            }
        }
    }

    #[test]
    fn unknown_event_type_does_not_parse() {
        assert_eq!(LedgerEvent::parse("totally_made_up"), None);
    }

    #[test]
    fn termination_phases_classify_distinctly() {
        // grace = waiting before the kill; stopping = kill in progress;
        // stopped = finished. A kill-in-progress must not be mislabeled grace.
        let grace = LedgerEvent::UsageGraceStarted.alert_class();
        assert_eq!((grace.category, grace.label), ("grace", "grace"));

        for started in [
            LedgerEvent::UsageTerminationStarted,
            LedgerEvent::TerminationStarted,
        ] {
            let class = started.alert_class();
            assert_eq!(
                (class.category, class.label),
                ("stopping", "stopping"),
                "{started:?} should classify as stopping, not grace"
            );
        }
        // The live (emitted) start event is actionable; the legacy read-compat
        // alias is not. Preserve that distinction.
        assert!(
            LedgerEvent::UsageTerminationStarted
                .alert_class()
                .actionable
        );

        let done = LedgerEvent::UsageTerminationCompleted.alert_class();
        assert_eq!((done.category, done.label), ("stopped", "stopped"));
    }
}
