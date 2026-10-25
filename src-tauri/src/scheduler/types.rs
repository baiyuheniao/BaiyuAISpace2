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
    /// 每隔 `interval_minutes` 分钟运行一次检测脚本，条件成立（退出码为 0）时触发。
    Condition,
}

impl ScheduleKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScheduleKind::Once => "once",
            ScheduleKind::Interval => "interval",
            ScheduleKind::Daily => "daily",
            ScheduleKind::Weekly => "weekly",
            ScheduleKind::Condition => "condition",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "interval" => ScheduleKind::Interval,
            "daily" => ScheduleKind::Daily,
            "weekly" => ScheduleKind::Weekly,
            "condition" => ScheduleKind::Condition,
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
    /// 仅当 `kind == Interval | Condition` 时使用。单位：分钟（条件任务里是检测间隔）。
    pub interval_minutes: Option<i64>,
    /// 仅当 `kind == Daily | Weekly` 时使用。格式为本地时间的 "HH:MM"。
    pub at_time: Option<String>,
    /// 仅当 `kind == Weekly` 时使用。0 = 周一 … 6 = 周日。
    pub weekday: Option<i64>,
    /// 仅当 `kind == Once` 时使用。Unix 毫秒时间戳。
    pub once_at: Option<i64>,
    /// 仅当 `kind == Condition` 时使用：检测脚本内容。退出码 0 表示条件成立。
    #[serde(default)]
    pub check_script: Option<String>,
    /// 仅当 `kind == Condition` 时使用：脚本解释器，见 [`CHECK_SHELLS`]。
    #[serde(default)]
    pub check_shell: Option<String>,
    /// 仅当 `kind == Condition` 时使用：`edge`（条件由不成立变成立时触发一次）
    /// 或 `repeat`（条件成立期间每隔 `cooldown_minutes` 触发一次）。
    #[serde(default)]
    pub trigger_mode: Option<String>,
    /// 仅当 `trigger_mode == repeat` 时使用。单位：分钟。
    #[serde(default)]
    pub cooldown_minutes: Option<i64>,
    /// 上一次检测的时间（Unix 毫秒）。
    #[serde(default)]
    pub last_check_at: Option<i64>,
    /// 上一次检测的结论：`met` / `not_met` / `error`。
    #[serde(default)]
    pub last_check_status: Option<String>,
    /// 上一次检测脚本的输出（已截断）；出错时是错误原因。
    #[serde(default)]
    pub last_check_output: Option<String>,
    /// 最近一次有明确结论（成立/不成立）的检测结果，边沿触发据此判断"由不成立变成立"。
    #[serde(default)]
    pub last_condition_met: bool,
    /// 连续检测出错的次数；达到上限会自动停用该任务。
    #[serde(default)]
    pub consecutive_errors: i64,
    /// Unix 毫秒时间戳 —— 调度循环下一次应该触发（条件任务则是检测）该任务的时刻。
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
    #[serde(default)]
    pub check_script: Option<String>,
    #[serde(default)]
    pub check_shell: Option<String>,
    #[serde(default)]
    pub trigger_mode: Option<String>,
    #[serde(default)]
    pub cooldown_minutes: Option<i64>,
}

/// 条件任务允许的脚本解释器。
pub const CHECK_SHELLS: [&str; 4] = ["powershell", "cmd", "sh", "python"];

/// 条件任务连续检测出错多少次后自动停用。
pub const MAX_CONSECUTIVE_CHECK_ERRORS: i64 = 5;

/// 作为 `scheduler://triggered` Tauri 事件发出的数据载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleTriggeredEvent {
    pub schedule_id: String,
    pub schedule_name: String,
    pub workspace_id: Option<String>,
    pub target_agent_id: Option<String>,
}

/// 一次检测完成后作为 `scheduler://checked` 事件发给前端，用来刷新任务的检测状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCheckedEvent {
    pub schedule_id: String,
    pub schedule_name: String,
    pub workspace_id: Option<String>,
    pub status: String,
    pub fired: bool,
    /// 连续出错达到上限、任务已被自动停用。
    pub auto_disabled: bool,
}

/// 试运行检测脚本的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestScriptResult {
    /// `met` / `not_met` / `error`
    pub status: String,
    pub output: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}
