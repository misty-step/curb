use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;

mod auth;
mod dispatch;
mod http_types;
mod response;
mod routes;
mod server;
mod token_store;
mod wire;

use curb_core::onboarding::{NotificationView, OnboardingView};
use curb_core::platform::Platform;
use curb_core::runtime::{Runtime, RuntimeError, TurnQuery};
use curb_core::service::{
    AckRequest, AckView, AlertView, ConfigUpdate, ConfigView, EventView, ReadinessView,
    ServiceError, SessionView, Snapshot, StopRequest, StopView, TurnView,
};

pub const TOKEN_COOKIE: &str = "curb_token";

pub use http_types::{HeaderMap, Request, Response};
pub use server::Server;
pub use token_store::load_or_create_token;

pub trait Backend {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError>;
    fn readiness(&self) -> Result<ReadinessView, ApiError>;
    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError>;
    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError>;
    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError>;
    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError>;
    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError>;
    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError>;
    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError>;
    fn config(&self) -> Result<ConfigView, ApiError>;
    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError>;
    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError>;
    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError>;
    fn notification_health(&self) -> Result<NotificationView, ApiError>;
    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError>;
}

impl<P: Platform> Backend for Runtime<P> {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        self.snapshot(now).map_err(ApiError::from)
    }

    fn readiness(&self) -> Result<ReadinessView, ApiError> {
        Ok(self.readiness())
    }

    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        self.rescan(now).map_err(ApiError::from)
    }

    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError> {
        self.session(key, now).map_err(ApiError::from)
    }

    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError> {
        self.turns(key, query, now).map_err(ApiError::from)
    }

    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError> {
        self.events(limit).map_err(ApiError::from)
    }

    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
        self.alerts(limit, now).map_err(ApiError::from)
    }

    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError> {
        self.acknowledge_session(key, request, now)
            .map_err(ApiError::from)
    }

    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError> {
        self.stop_session(key, request, now).map_err(ApiError::from)
    }

    fn config(&self) -> Result<ConfigView, ApiError> {
        self.config_view().map_err(ApiError::from)
    }

    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
        self.update_config(update).map_err(ApiError::from)
    }

    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        self.onboarding(now).map_err(ApiError::from)
    }

    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        self.complete_onboarding(now).map_err(ApiError::from)
    }

    fn notification_health(&self) -> Result<NotificationView, ApiError> {
        self.notification_health().map_err(ApiError::from)
    }

    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
        self.test_notification(now).map_err(ApiError::from)
    }
}

impl<B: Backend> Backend for Arc<B> {
    fn snapshot(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        (**self).snapshot(now)
    }

    fn readiness(&self) -> Result<ReadinessView, ApiError> {
        (**self).readiness()
    }

    fn rescan(&self, now: DateTime<Utc>) -> Result<Snapshot, ApiError> {
        (**self).rescan(now)
    }

    fn session(&self, key: &str, now: DateTime<Utc>) -> Result<SessionView, ApiError> {
        (**self).session(key, now)
    }

    fn turns(
        &self,
        key: &str,
        query: TurnQuery,
        now: DateTime<Utc>,
    ) -> Result<Vec<TurnView>, ApiError> {
        (**self).turns(key, query, now)
    }

    fn events(&self, limit: usize) -> Result<Vec<EventView>, ApiError> {
        (**self).events(limit)
    }

    fn alerts(&self, limit: usize, now: DateTime<Utc>) -> Result<Vec<AlertView>, ApiError> {
        (**self).alerts(limit, now)
    }

    fn acknowledge_session(
        &self,
        key: &str,
        request: AckRequest,
        now: DateTime<Utc>,
    ) -> Result<AckView, ApiError> {
        (**self).acknowledge_session(key, request, now)
    }

    fn stop_session(
        &self,
        key: &str,
        request: StopRequest,
        now: DateTime<Utc>,
    ) -> Result<StopView, ApiError> {
        (**self).stop_session(key, request, now)
    }

    fn config(&self) -> Result<ConfigView, ApiError> {
        (**self).config()
    }

    fn update_config(&self, update: ConfigUpdate) -> Result<ConfigView, ApiError> {
        (**self).update_config(update)
    }

    fn onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        (**self).onboarding(now)
    }

    fn complete_onboarding(&self, now: DateTime<Utc>) -> Result<OnboardingView, ApiError> {
        (**self).complete_onboarding(now)
    }

    fn notification_health(&self) -> Result<NotificationView, ApiError> {
        (**self).notification_health()
    }

    fn test_notification(&self, now: DateTime<Utc>) -> Result<NotificationView, ApiError> {
        (**self).test_notification(now)
    }
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("api config: {0}")]
    Config(String),
    #[error("session not found")]
    SessionNotFound,
    #[error("invalid acknowledgement: {0}")]
    InvalidAck(String),
    #[error("invalid stop request: {0}")]
    InvalidStop(String),
    #[error("invalid config update: {0}")]
    InvalidConfig(String),
    #[error("session cannot be stopped safely: {0}")]
    StopConflict(String),
    #[error("local notifications are disabled")]
    NotificationsDisabled(NotificationView),
    #[error("local notifications are unavailable")]
    NotificationsUnavailable(NotificationView),
    #[error("{0}")]
    Internal(String),
}

impl From<RuntimeError> for ApiError {
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Service(ServiceError::SessionNotFound) => Self::SessionNotFound,
            RuntimeError::Service(ServiceError::InvalidAck(message)) => Self::InvalidAck(message),
            RuntimeError::Service(ServiceError::InvalidStop(message)) => Self::InvalidStop(message),
            RuntimeError::Service(ServiceError::InvalidConfig(message)) => {
                Self::InvalidConfig(message)
            }
            RuntimeError::Service(ServiceError::StopConflict(message)) => {
                Self::StopConflict(message)
            }
            RuntimeError::NotificationsDisabled(view) => Self::NotificationsDisabled(view),
            RuntimeError::NotificationsUnavailable(view) => Self::NotificationsUnavailable(view),
            other => Self::Internal(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
