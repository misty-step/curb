use std::path::PathBuf;
use std::sync::Mutex;

use crate::config::Config;
use crate::runtime::RuntimeError;
use crate::service::{self, ConfigUpdate, ConfigView};

pub(crate) struct ConfigStore {
    cfg: Mutex<Config>,
    path: Option<PathBuf>,
}

impl ConfigStore {
    pub(crate) fn new(cfg: Config) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            path: None,
        }
    }

    pub(crate) fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub(crate) fn get(&self) -> Config {
        self.cfg.lock().expect("config mutex poisoned").clone()
    }

    pub(crate) fn view(&self) -> ConfigView {
        let cfg = self.get();
        service::config_view(self.path.as_deref(), &cfg)
    }

    pub(crate) fn update(&self, update: ConfigUpdate) -> Result<ConfigView, RuntimeError> {
        let path = self
            .path
            .as_ref()
            .ok_or(RuntimeError::ConfigPathUnavailable)?;
        let mut next = self.get();
        service::apply_config_update(&mut next, update)?;
        next.save(path)?;
        *self.cfg.lock().expect("config mutex poisoned") = next.clone();
        Ok(service::config_view(Some(path), &next))
    }
}
