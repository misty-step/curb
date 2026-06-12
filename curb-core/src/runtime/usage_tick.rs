use std::sync::Mutex;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::governor::{GovernorEngine, GovernorReport};
use crate::ledger::{Event, Ledger, LedgerEvent};
use crate::local_enforcer::{self, LocalEnforcer};
use crate::platform::Platform;
use crate::runtime::RuntimeError;
use crate::service::Snapshot;
use crate::usage::Reader;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsageTickReport {
    pub snapshot: Snapshot,
    pub governor: GovernorReport,
}

pub(crate) fn usage_scan<P: Platform>(
    cfg: &Config,
    reader: &Reader,
    platform: &P,
    governor: &Mutex<GovernorEngine>,
    now: DateTime<Utc>,
    rescan: impl FnOnce() -> Result<Snapshot, RuntimeError>,
) -> Result<Snapshot, RuntimeError> {
    Ok(usage_scan_report(cfg, reader, platform, governor, now, rescan)?.snapshot)
}

pub(crate) fn usage_scan_report<P: Platform>(
    cfg: &Config,
    reader: &Reader,
    platform: &P,
    governor: &Mutex<GovernorEngine>,
    now: DateTime<Utc>,
    rescan: impl FnOnce() -> Result<Snapshot, RuntimeError>,
) -> Result<UsageTickReport, RuntimeError> {
    let scan = reader.scan_since(Some(lookback_start(cfg, now)))?;
    let processes = platform.capture()?;
    let sessions = local_enforcer::build_policy_sessions(cfg, &scan.events, &processes, now)?;
    let enforcer = LocalEnforcer::new(cfg, platform, &processes);
    let governor = governor
        .lock()
        .expect("governor mutex poisoned")
        .scan(cfg, &sessions, &enforcer, now)?;
    Ok(UsageTickReport {
        snapshot: rescan()?,
        governor,
    })
}

pub(crate) fn usage_tick<P: Platform>(
    cfg: &Config,
    reader: &Reader,
    platform: &P,
    governor: &Mutex<GovernorEngine>,
    now: DateTime<Utc>,
    rescan: impl FnMut() -> Result<Snapshot, RuntimeError>,
) -> Result<Snapshot, RuntimeError> {
    Ok(usage_tick_report(cfg, reader, platform, governor, now, rescan)?.snapshot)
}

pub(crate) fn usage_tick_report<P: Platform>(
    cfg: &Config,
    reader: &Reader,
    platform: &P,
    governor: &Mutex<GovernorEngine>,
    now: DateTime<Utc>,
    mut rescan: impl FnMut() -> Result<Snapshot, RuntimeError>,
) -> Result<UsageTickReport, RuntimeError> {
    match usage_scan_report(cfg, reader, platform, governor, now, &mut rescan) {
        Ok(report) => Ok(report),
        Err(error) => {
            append_usage_scan_failed(cfg, &error)?;
            Ok(UsageTickReport {
                snapshot: rescan()?,
                governor: GovernorReport::default(),
            })
        }
    }
}

pub(crate) fn lookback_start(cfg: &Config, now: DateTime<Utc>) -> DateTime<Utc> {
    now - chrono::Duration::from_std(cfg.usage.lookback.as_std()).unwrap()
}

fn append_usage_scan_failed(cfg: &Config, error: &RuntimeError) -> Result<(), RuntimeError> {
    let mut event = Event::new(LedgerEvent::UsageScanFailed).with_message(error.to_string());
    event.mode = Some(cfg.mode.to_string());
    Ledger::open(&cfg.ledger.path)?.append(event)?;
    Ok(())
}
