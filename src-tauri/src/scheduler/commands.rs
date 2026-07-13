// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use chrono::{Local, NaiveTime, Datelike};

use crate::db::DbState;
use super::types::*;
use super::db;
use crate::workspace::commands::{send_workspace_message, insert_workspace_log};

// ─── next_run_at 计算 ────────────────────────────────────────────────

/// 为新创建的定时任务计算首次（或下一次）的 `next_run_at`。
/// 仅当数据不一致时（例如 `Once` 类型却没有 `once_at`）才会返回 `None`。
pub fn compute_initial_next_run_at(req: &CreateScheduleRequest) -> Option<i64> {
    let now = chrono::Utc::now().timestamp_millis();
    match req.kind {
        ScheduleKind::Once => req.once_at,
        ScheduleKind::Interval => {
            let mins = req.interval_minutes.unwrap_or(60);
            Some(now + mins * 60_000)
        }
        ScheduleKind::Daily => {
            let at = req.at_time.as_deref()?;
            next_daily_occurrence(at, now)
        }
        ScheduleKind::Weekly => {
            let at = req.at_time.as_deref()?;
            let wd = req.weekday.unwrap_or(0);
            next_weekly_occurrence(at, wd as u32, now)
        }
    }
}

/// 在 `after_ms` 之后（含）最近一次到达 `HH:MM`（本地时间）的时刻。
fn next_daily_occurrence(at_time: &str, after_ms: i64) -> Option<i64> {
    let t = NaiveTime::parse_from_str(at_time, "%H:%M").ok()?;
    let after = chrono::DateTime::from_timestamp_millis(after_ms)?;
    let local_after = after.with_timezone(&Local);
    let candidate = local_after.date_naive().and_time(t);
    let candidate_ms = candidate.and_local_timezone(Local).single()?.timestamp_millis();
    if candidate_ms > after_ms {
        Some(candidate_ms)
    } else {
        let tomorrow = (local_after + chrono::Duration::days(1)).date_naive().and_time(t);
        Some(tomorrow.and_local_timezone(Local).single()?.timestamp_millis())
    }
}

/// 下一次到达星期 `wd`（0=周一…6=周日）本地时间 `HH:MM` 的时刻。
fn next_weekly_occurrence(at_time: &str, wd: u32, after_ms: i64) -> Option<i64> {
    let t = NaiveTime::parse_from_str(at_time, "%H:%M").ok()?;
    let after = chrono::DateTime::from_timestamp_millis(after_ms)?;
    let local_after = after.with_timezone(&Local);
    let current_wd = local_after.weekday().num_days_from_monday();
    let mut days_ahead = wd as i64 - current_wd as i64;
    if days_ahead < 0 { days_ahead += 7; }
    let candidate = (local_after + chrono::Duration::days(days_ahead)).date_naive().and_time(t);
    let candidate_ms = candidate.and_local_timezone(Local).single()?.timestamp_millis();
    if candidate_ms > after_ms {
        Some(candidate_ms)
    } else {
        let next = (local_after + chrono::Duration::days(days_ahead + 7)).date_naive().and_time(t);
        Some(next.and_local_timezone(Local).single()?.timestamp_millis())
    }
}

/// 在定时任务刚刚触发之后，计算下一次的 `next_run_at`。
pub fn compute_next_run_at(schedule: &Schedule, fired_at_ms: i64) -> Option<i64> {
    match schedule.kind {
        ScheduleKind::Once => None, // 触发后会被禁用
        ScheduleKind::Interval => {
            let mins = schedule.interval_minutes.unwrap_or(60);
            Some(fired_at_ms + mins * 60_000)
        }
        ScheduleKind::Daily => {
            let at = schedule.at_time.as_deref()?;
            let fired = chrono::DateTime::from_timestamp_millis(fired_at_ms)?;
            let fired_local = fired.with_timezone(&Local);
            let t = NaiveTime::parse_from_str(at, "%H:%M").ok()?;
            let tomorrow = (fired_local + chrono::Duration::days(1)).date_naive().and_time(t);
            Some(tomorrow.and_local_timezone(Local).single()?.timestamp_millis())
        }
        ScheduleKind::Weekly => {
            let at = schedule.at_time.as_deref()?;
            let wd = schedule.weekday.unwrap_or(0);
            let fired = chrono::DateTime::from_timestamp_millis(fired_at_ms)?;
            let fired_local = fired.with_timezone(&Local);
            let t = NaiveTime::parse_from_str(at, "%H:%M").ok()?;
            let next_week_base = (fired_local + chrono::Duration::days(7)).date_naive();
            let current_wd = next_week_base.weekday().num_days_from_monday() as i64;
            let mut days_ahead = wd - current_wd;
            if days_ahead < 0 { days_ahead += 7; }
            let target = (next_week_base + chrono::Duration::days(days_ahead)).and_time(t);
            Some(target.and_local_timezone(Local).single()?.timestamp_millis())
        }
    }
}

// ─── 后台调度循环 ──────────────────────────────────────────────

pub async fn run_scheduler_loop(app_handle: AppHandle, cancel: CancellationToken) {
    log::info!("[scheduler] 调度循环已启动，每 30 秒检查一次");
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("[scheduler] 调度循环已停止");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let db_state = app_handle.state::<DbState>();
        let db_path = {
            let db = db_state.0.lock().await;
            db.path.clone()
        };

        let due = match rusqlite::Connection::open(&db_path) {
            Ok(conn) => db::list_due_schedules(&conn, now_ms).unwrap_or_default(),
            Err(e) => { log::error!("[scheduler] 打开数据库失败: {}", e); continue; }
        };

        for schedule in due {
            fire_schedule(&app_handle, &schedule, now_ms, &db_path).await;
        }
    }
}

async fn fire_schedule(app_handle: &AppHandle, schedule: &Schedule, now_ms: i64, db_path: &str) {
    log::info!("[scheduler] 触发定时任务「{}」(id={})", schedule.name, schedule.id);

    // 1. 发消息到 workspace（如果有绑定）
    if let Some(workspace_id) = &schedule.workspace_id {
        let to = schedule.target_agent_id.as_deref().unwrap_or("all");
        send_workspace_message(app_handle, workspace_id, "system", to, &schedule.message).await;
        insert_workspace_log(
            app_handle, workspace_id, None,
            "scheduled_trigger",
            format!("⏰ 定时任务「{}」触发：{}", schedule.name, schedule.message),
        ).await;
    }

    // 2. 推送事件到前端
    let _ = app_handle.emit("scheduler://triggered", ScheduleTriggeredEvent {
        schedule_id: schedule.id.clone(),
        schedule_name: schedule.name.clone(),
        workspace_id: schedule.workspace_id.clone(),
        target_agent_id: schedule.target_agent_id.clone(),
    });

    // 3. 计算下次运行时间，更新 DB
    let next = compute_next_run_at(schedule, now_ms);
    let disable = schedule.kind == ScheduleKind::Once;
    if let Ok(conn) = rusqlite::Connection::open(db_path) {
        let _ = db::update_after_fire(&conn, &schedule.id, next, now_ms, disable);
    }
}

// ─── Tauri 命令 ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn schedule_create(
    request: CreateScheduleRequest,
    db_state: State<'_, DbState>,
) -> Result<Schedule, String> {
    let next_run_at = compute_initial_next_run_at(&request)
        .ok_or_else(|| "无法计算下次运行时间，请检查调度参数".to_string())?;

    let now = chrono::Utc::now().timestamp_millis();
    let schedule = Schedule {
        id:               Uuid::new_v4().to_string(),
        name:             request.name.clone(),
        workspace_id:     request.workspace_id.clone(),
        target_agent_id:  request.target_agent_id.clone(),
        message:          request.message.clone(),
        kind:             request.kind.clone(),
        interval_minutes: request.interval_minutes,
        at_time:          request.at_time.clone(),
        weekday:          request.weekday,
        once_at:          request.once_at,
        next_run_at,
        last_run_at:      None,
        enabled:          true,
        created_at:       now,
        updated_at:       now,
    };

    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| e.to_string())?;
    db::insert_schedule(&conn, &schedule).map_err(|e| e.to_string())?;
    Ok(schedule)
}

#[tauri::command]
pub async fn schedule_list(
    workspace_id: Option<String>,
    db_state: State<'_, DbState>,
) -> Result<Vec<Schedule>, String> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| e.to_string())?;
    db::list_schedules(&conn, workspace_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn schedule_delete(
    id: String,
    db_state: State<'_, DbState>,
) -> Result<(), String> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| e.to_string())?;
    db::delete_schedule(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn schedule_toggle(
    id: String,
    db_state: State<'_, DbState>,
) -> Result<Schedule, String> {
    let db = db_state.0.lock().await;
    let conn = rusqlite::Connection::open(&db.path).map_err(|e| e.to_string())?;
    db::toggle_schedule(&conn, &id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("定时任务 {} 不存在", id))
}
