// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use serde::{Deserialize, Serialize};

/// 定时任务的重复方式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    /// 在 `once_at` 时刻触发一次，之后自动禁用。
    Once,
    /// 从创建时刻起，每隔 `interval_minutes` 分钟触发一次。
    Interval,
    /// 每天在 `at_time`（本地墙钟时间 "HH:MM"）触发。
    Daily,
    /// 每周在 `weekday` 这一天的 `at_time` 触发。
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

/// 持久化保存的定时任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// 该定时任务所属的工作组。`None` 表示预留给未来非工作组场景使用。
    pub workspace_id: Option<String>,
    /// 消息发给哪个 Agent。`None` 表示广播给工作组内所有 Agent。
    pub target_agent_id: Option<String>,
    /// 任务触发时发给 Agent 的消息内容。
    pub message: String,
    pub kind: ScheduleKind,
    /// 仅当 `kind == Interval` 时使用。单位：分钟。
    pub interval_minutes: Option<i64>,
    /// 仅当 `kind == Daily | Weekly` 时使用。格式为本地时间的 "HH:MM"。
    pub at_time: Option<String>,
    /// 仅当 `kind == Weekly` 时使用。0 = 周一 … 6 = 周日。
    pub weekday: Option<i64>,
    /// 仅当 `kind == Once` 时使用。Unix 毫秒时间戳。
    pub once_at: Option<i64>,
    /// Unix 毫秒时间戳 —— 调度循环下一次应该触发该任务的时刻。
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

/// 作为 `scheduler://triggered` Tauri 事件发出的数据载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleTriggeredEvent {
    pub schedule_id: String,
    pub schedule_name: String,
    pub workspace_id: Option<String>,
    pub target_agent_id: Option<String>,
}
