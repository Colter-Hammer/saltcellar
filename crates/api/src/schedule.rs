//! When bank syncs start on their own (docs/spec/m2-bank-data.md#sync-runs): the daily run at
//! `sync.time` in the household timezone, retries after a transient failure, and the catch-up a
//! few minutes after a start that finds the last successful sync over 20 hours old. The rules
//! are `domain::sync_schedule`'s; this module keeps what waits to run and runs it.
//!
//! [`tick`] does whatever is due by the state's clock; the server calls it every
//! [`TICK`], tests after moving a fake clock. A trigger that finds a sync running joins it: it
//! waits for it and asks the Bridge nothing.

use std::time::Duration;

use domain::sync_schedule::{self as rules, SyncTime};
use jiff::Timestamp;

use crate::sync::{self, Ran};
use crate::{ApiError, AppState};

/// How often the server looks for due work *(agent)*.
const TICK: Duration = Duration::from_secs(30);

/// What waits to run. Kept in memory: a restart forgets the retries, and the catch-up stands in
/// for them.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    retry: Option<Retry>,
    catch_up: Option<Timestamp>,
    /// The scheduled slot already run or joined, so it isn't run twice.
    handled_slot: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy)]
struct Retry {
    /// The failure the retries follow.
    failed_at: Timestamp,
    /// 0 for the first retry.
    attempt: usize,
    due: Timestamp,
}

impl AppState {
    /// Starts the schedule: plans this start's catch-up, then does what is due every 30 seconds
    /// for as long as the process runs.
    ///
    /// # Errors
    /// If the database fails while planning the catch-up.
    pub async fn start_sync_schedule(&self) -> Result<tokio::task::JoinHandle<()>, db::Error> {
        plan_catch_up(self).await?;
        let state = self.clone();
        Ok(tokio::spawn(async move {
            let mut every = tokio::time::interval(TICK);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                every.tick().await;
                if let Err(error) = tick(&state).await {
                    tracing::warn!(code = error.code(), "the bank sync schedule failed a step");
                }
            }
        }))
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Pending> {
        // A panic while holding it leaves nothing half-changed worth refusing.
        self.schedule
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Plans the catch-up for a start now: a few minutes on, when there's a connection and its last
/// successful sync is over 20 hours old.
pub(crate) async fn plan_catch_up(state: &AppState) -> Result<(), db::Error> {
    let connected = db::connection(&state.pool)
        .await?
        .is_some_and(|c| c.status == db::ConnectionStatus::Ok);
    let at = if connected {
        rules::catch_up_at(db::last_successful_sync(&state.pool).await?, state.now())
    } else {
        None
    };
    state.pending().catch_up = at;
    Ok(())
}

/// Does whatever is due now: the catch-up, the scheduled run, a retry.
pub(crate) async fn tick(state: &AppState) -> Result<(), ApiError> {
    if db::household(&state.pool).await?.is_none() {
        return Ok(());
    }
    let now = state.now();
    let catch_up = {
        let mut pending = state.pending();
        match pending.catch_up {
            Some(at) if at <= now => pending.catch_up.take(),
            _ => None,
        }
    };
    if catch_up.is_some() {
        // A sync since the start may have made it pointless.
        let last_ok = db::last_successful_sync(&state.pool).await?;
        if last_ok.is_none_or(|at| now.duration_since(at) > rules::CATCH_UP_AFTER) {
            start(state, db::SyncTrigger::CatchUp).await?;
        }
    }
    if let Some(slot) = scheduled_slot(state, state.now()).await? {
        state.pending().handled_slot = Some(slot);
        start(state, db::SyncTrigger::Scheduled).await?;
    }
    let now = state.now();
    let retry = {
        let pending = state.pending();
        pending.retry.filter(|r| r.due <= now)
    };
    if retry.is_some() {
        start(state, db::SyncTrigger::Retry).await?;
    }
    Ok(())
}

/// The scheduled slot to run now, if it is due and not handled yet.
async fn scheduled_slot(state: &AppState, now: Timestamp) -> Result<Option<Timestamp>, ApiError> {
    let Some(household) = db::household(&state.pool).await? else {
        return Ok(None);
    };
    let tz = jiff::tz::TimeZone::get(household.timezone.name())
        .map_err(|error| ApiError::internal(&error))?;
    let time = sync::sync_time(&db::sync_settings(&state.pool).await?);
    let slot = time.slot_at_or_before(now, &tz);
    if state.pending().handled_slot == Some(slot) {
        return Ok(None);
    }
    let last = db::last_scheduled_sync(&state.pool).await?;
    Ok(rules::scheduled_due(slot, last, now).then_some(slot))
}

/// A new sync time: a slot it puts in the past today isn't run until tomorrow.
pub(crate) async fn time_changed(state: &AppState, time: SyncTime) -> Result<(), ApiError> {
    let Some(household) = db::household(&state.pool).await? else {
        return Ok(());
    };
    let tz = jiff::tz::TimeZone::get(household.timezone.name())
        .map_err(|error| ApiError::internal(&error))?;
    let slot = time.slot_at_or_before(state.now(), &tz);
    state.pending().handled_slot = Some(slot);
    Ok(())
}

/// Runs a sync for `trigger`, or joins the one running. A connection that isn't there or is
/// paused, or a spent request budget, skips it quietly (the run logs why).
async fn start(state: &AppState, trigger: db::SyncTrigger) -> Result<(), ApiError> {
    let Ok(_running) = state.sync_lock.try_lock() else {
        // Join: the running sync stands for this one. A waiting retry stays, unless that
        // sync's success clears it.
        drop(state.sync_lock.lock().await);
        return Ok(());
    };
    let ran = sync::run(state, trigger).await;
    match ran {
        Ok(ran) => {
            after_run(state, trigger, ran);
            Ok(())
        }
        Err(error) if error.is_conflict() => {
            if trigger == db::SyncTrigger::Retry {
                state.pending().retry = None;
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// What a finished run means for the retries: a success clears them; a transient failure of a
/// run that started on its own plans the next one; any other failure stops them.
pub(crate) fn after_run(state: &AppState, trigger: db::SyncTrigger, ran: Ran) {
    let Ran::Finished { failed, transient } = ran else {
        return;
    };
    let now = state.now();
    let mut pending = state.pending();
    if !failed && !transient {
        pending.retry = None;
        return;
    }
    let automatic = matches!(
        trigger,
        db::SyncTrigger::Scheduled | db::SyncTrigger::CatchUp | db::SyncTrigger::Retry
    );
    if !automatic {
        // Sync now and back-fills don't plan retries; the person sees how it went *(agent)*.
        return;
    }
    if !transient {
        pending.retry = None;
        return;
    }
    let (failed_at, attempt) = match (trigger, pending.retry) {
        (db::SyncTrigger::Retry, Some(retry)) => (retry.failed_at, retry.attempt + 1),
        _ => (now, 0),
    };
    pending.retry = rules::retry_at(failed_at, attempt).map(|due| Retry {
        failed_at,
        attempt,
        due,
    });
}

#[cfg(test)]
impl AppState {
    /// When the next retry is due, if one waits.
    pub(crate) fn retry_due(&self) -> Option<Timestamp> {
        self.pending().retry.map(|r| r.due)
    }

    /// When this start's catch-up is due, if one waits.
    pub(crate) fn catch_up_due(&self) -> Option<Timestamp> {
        self.pending().catch_up
    }
}
