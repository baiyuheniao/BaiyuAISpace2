// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};

/// How a schedule repeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// Fire once at `once_at`, then disable.
    Once,
    /// Fire every `interval_minutes` minutes starting from creation.
    Interval,
    /// Fire every day at `at_time` (local wall-clock "HH:MM").
    Daily,
    /// Fire every week on `weekday` at `at_time`.
    Weekly,
}

impl ScheduleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScheduleKind::Once => "once",
            ScheduleKind::Interval => "interval",
            ScheduleKind::Daily => "daily",
            ScheduleKind::Weekly => "weekly",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "interval" => ScheduleKind::Interval,
            "daily" => ScheduleKind::Daily,
            "weekly" => ScheduleKind::Weekly,
            _ => ScheduleKind::Once,
        }
    }
}

/// A persisted scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// The workspace this schedule is scoped to. `None` = future non-workspace use.
    pub workspace_id: Option<String>,
    /// Which agent receives the message. `None` = broadcast to all agents in workspace.
    pub target_agent_id: Option<String>,
    /// The message content sent to the agent when the schedule fires.
    pub message: String,
    pub kind: ScheduleKind,
    /// Used when `kind == Interval`. Unit: minutes.
    pub interval_minutes: Option<i64>,
    /// Used when `kind == Daily | Weekly`. Format: "HH:MM" in local time.
    pub at_time: Option<String>,
    /// Used when `kind == Weekly`. 0 = Monday … 6 = Sunday.
    pub weekday: Option<i64>,
    /// Used when `kind == Once`. Unix milliseconds.
    pub once_at: Option<i64>,
    /// Unix milliseconds — when the scheduler loop should next fire this task.
    pub next_run_at: i64,
    pub last_run_at: Option<i64>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduleRequest {
    pub name: String,
    pub workspace_id: Option<String>,
    pub target_agent_id: Option<String>,
    pub message: String,
    pub kind: ScheduleKind,
    pub interval_minutes: Option<i64>,
    pub at_time: Option<String>,
    pub weekday: Option<i64>,
    pub once_at: Option<i64>,
}

/// Payload emitted as `scheduler://triggered` Tauri event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleTriggeredEvent {
    pub schedule_id: String,
    pub schedule_name: String,
    pub workspace_id: Option<String>,
    pub target_agent_id: Option<String>,
}
