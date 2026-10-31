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
use super::check::{self, CheckStatus};
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
        // 条件任务创建后马上做第一次检测
        ScheduleKind::Condition => Some(now),
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
        ScheduleKind::Interval | ScheduleKind::Condition => {
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
            if schedule.kind == ScheduleKind::Condition {
                // 检测脚本可能跑得比较久：放到独立任务里，既不阻塞其他定时任务，
                // 也用"检测中"集合防止同一个任务的检测重叠执行。
                if !CHECKS_IN_FLIGHT.lock().unwrap().insert(schedule.id.clone()) {
                    continue;
                }
                let app = app_handle.clone();
                let path = db_path.clone();
                tokio::spawn(async move {
                    run_condition_check(&app, &schedule, &path).await;
                    CHECKS_IN_FLIGHT.lock().unwrap().remove(&schedule.id);
                });
            } else {
                fire_schedule(&app_handle, &schedule, now_ms, &db_path).await;
            }
        }
    }
}

/// 正在检测中的条件任务 id。
static CHECKS_IN_FLIGHT: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashSet<String>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// 把一次触发送达：发消息给工作组里的 Agent、写日志、通知前端。不碰数据库里的调度状态。
async fn deliver_trigger(app_handle: &AppHandle, schedule: &Schedule, message: &str) {
    // 1. 发消息到 workspace（如果有绑定）
    if let Some(workspace_id) = &schedule.workspace_id {
        let to = schedule.target_agent_id.as_deref().unwrap_or("all");
        send_workspace_message(app_handle, workspace_id, "system", to, message).await;
        insert_workspace_log(
            app_handle, workspace_id, None,
            "scheduled_trigger",
            format!("⏰ 定时任务「{}」触发：{}", schedule.name, message),
        ).await;
    }

    // 2. 推送事件到前端
    let _ = app_handle.emit("scheduler://triggered", ScheduleTriggeredEvent {
        schedule_id: schedule.id.clone(),
        schedule_name: schedule.name.clone(),
        workspace_id: schedule.workspace_id.clone(),
        target_agent_id: schedule.target_agent_id.clone(),
    });
}

async fn fire_schedule(app_handle: &AppHandle, schedule: &Schedule, now_ms: i64, db_path: &str) {
    log::info!("[scheduler] 触发定时任务「{}」(id={})", schedule.name, schedule.id);

    deliver_trigger(app_handle, schedule, &schedule.message).await;

    // 计算下次运行时间，更新 DB
    let next = compute_next_run_at(schedule, now_ms);
    let disable = schedule.kind == ScheduleKind::Once;
    if let Ok(conn) = rusqlite::Connection::open(db_path) {
        let _ = db::update_after_fire(&conn, &schedule.id, next, now_ms, disable);
    }
}

/// 一次检测之后对任务状态的改动。
#[derive(Debug, PartialEq, Eq)]
struct CheckDecision {
    /// 有明确结论时的"条件是否成立"；出错时为 `None`，保持原值。
    condition_met: Option<bool>,
    consecutive_errors: i64,
    fired: bool,
    disable: bool,
}

/// 根据这次检测的结论决定：要不要触发、错误计数、是否自动停用。纯函数，便于测试。
fn evaluate_check(schedule: &Schedule, status: &CheckStatus, now_ms: i64) -> CheckDecision {
    match status {
        CheckStatus::Error => {
            let errors = schedule.consecutive_errors + 1;
            CheckDecision { condition_met: None, consecutive_errors: errors, fired: false, disable: errors >= MAX_CONSECUTIVE_CHECK_ERRORS }
        }
        CheckStatus::Met | CheckStatus::NotMet => {
            let met = *status == CheckStatus::Met;
            let cooldown_ms = schedule.cooldown_minutes.unwrap_or(60).max(1) * 60_000;
            let fired = check::should_fire(
                schedule.trigger_mode.as_deref().unwrap_or("edge"),
                schedule.last_condition_met,
                met,
                schedule.last_run_at,
                now_ms,
                cooldown_ms,
            );
            CheckDecision { condition_met: Some(met), consecutive_errors: 0, fired, disable: false }
        }
    }
}

/// 条件任务的一次检测：跑脚本 → 判断要不要触发 → 更新检测状态并通知前端。
async fn run_condition_check(app_handle: &AppHandle, schedule: &Schedule, db_path: &str) {
    let shell = schedule.check_shell.as_deref().unwrap_or("sh");
    let script = schedule.check_script.as_deref().unwrap_or("");
    let outcome = check::run_check_script(shell, script, check::CHECK_TIMEOUT).await;
    let now_ms = chrono::Utc::now().timestamp_millis();
    log::info!(
        "[scheduler] 条件任务「{}」检测完成：{}（{} ms）",
        schedule.name, outcome.status.as_str(), outcome.duration_ms
    );

    let interval_ms = schedule.interval_minutes.unwrap_or(5).max(1) * 60_000;

    let CheckDecision { condition_met, consecutive_errors, fired, disable } =
        evaluate_check(schedule, &outcome.status, now_ms);

    if fired {
        let message = check::compose_trigger_message(&schedule.message, &outcome.output);
        deliver_trigger(app_handle, schedule, &message).await;
    }

    if disable {
        if let Some(workspace_id) = &schedule.workspace_id {
            insert_workspace_log(
                app_handle, workspace_id, None,
                "scheduled_trigger",
                format!(
                    "⚠️ 条件任务「{}」连续 {} 次检测出错，已自动停用：{}",
                    schedule.name, consecutive_errors, outcome.output
                ),
            ).await;
        }
    }

    if let Ok(conn) = rusqlite::Connection::open(db_path) {
        let update = db::CheckUpdate {
            next_run_at: now_ms + interval_ms,
            checked_at: now_ms,
            status: outcome.status.as_str(),
            output: &outcome.output,
            condition_met,
            consecutive_errors,
            fired,
            disable,
        };
        if let Err(e) = db::update_after_check(&conn, &schedule.id, &update) {
            log::error!("[scheduler] 保存检测结果失败: {}", e);
        }
    }

    let _ = app_handle.emit("scheduler://checked", ScheduleCheckedEvent {
        schedule_id: schedule.id.clone(),
        schedule_name: schedule.name.clone(),
        workspace_id: schedule.workspace_id.clone(),
        status: outcome.status.as_str().to_string(),
        fired,
        auto_disabled: disable,
    });
}

// ─── Tauri 命令 ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn schedule_create(
    request: CreateScheduleRequest,
    db_state: State<'_, DbState>,
) -> Result<Schedule, String> {
    create_schedule_record(request, None, &db_state).await
}

/// 创建并保存一个定时任务。`created_by` 为 `None` 表示用户创建；Agent 通过工具创建时
/// 传 `agent:<id>:<名字>`。界面命令和 Agent 工具共用这一处，校验和字段补全保持一致。
pub async fn create_schedule_record(
    request: CreateScheduleRequest,
    created_by: Option<String>,
    db_state: &DbState,
) -> Result<Schedule, String> {
    let request = normalize_condition_request(request)?;
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
        check_script:     request.check_script.clone(),
        check_shell:      request.check_shell.clone(),
        trigger_mode:     request.trigger_mode.clone(),
        cooldown_minutes: request.cooldown_minutes,
        last_check_at:    None,
        last_check_status: None,
        last_check_output: None,
        last_condition_met: false,
        consecutive_errors: 0,
        created_by,
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

/// 校验并补全条件任务的参数；其他类型的任务原样返回，并清掉不该带的条件字段。
fn normalize_condition_request(mut request: CreateScheduleRequest) -> Result<CreateScheduleRequest, String> {
    if request.kind != ScheduleKind::Condition {
        request.check_script = None;
        request.check_shell = None;
        request.trigger_mode = None;
        request.cooldown_minutes = None;
        return Ok(request);
    }
    let script = request.check_script.as_deref().map(str::trim).unwrap_or("");
    if script.is_empty() {
        return Err("条件触发任务需要填写检测脚本".to_string());
    }
    request.check_script = Some(script.to_string());
    let shell = request.check_shell.as_deref().unwrap_or("");
    if !CHECK_SHELLS.contains(&shell) {
        return Err(format!("请选择脚本解释器（{}）", CHECK_SHELLS.join(" / ")));
    }
    if request.workspace_id.is_none() {
        return Err("条件触发任务需要绑定工作组，触发时才有地方发消息".to_string());
    }
    request.interval_minutes = Some(request.interval_minutes.unwrap_or(5).max(1));
    let mode = request.trigger_mode.clone().unwrap_or_else(|| "edge".to_string());
    if mode != "edge" && mode != "repeat" {
        return Err("触发模式只能是 edge 或 repeat".to_string());
    }
    request.trigger_mode = Some(mode.clone());
    request.cooldown_minutes = if mode == "repeat" { Some(request.cooldown_minutes.unwrap_or(60).max(1)) } else { None };
    Ok(request)
}

/// 条件任务保存前的"试运行"：跑一次脚本，返回结论、输出和耗时，不触发任何任务。
#[tauri::command]
pub async fn schedule_test_script(shell: String, script: String) -> Result<TestScriptResult, String> {
    if !CHECK_SHELLS.contains(&shell.as_str()) {
        return Err(format!("不支持的脚本解释器：{shell}"));
    }
    if script.trim().is_empty() {
        return Err("请先填写检测脚本".to_string());
    }
    let outcome = check::run_check_script(&shell, script.trim(), check::CHECK_TIMEOUT).await;
    Ok(TestScriptResult {
        status: outcome.status.as_str().to_string(),
        output: outcome.output,
        exit_code: outcome.exit_code,
        duration_ms: outcome.duration_ms,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn condition_schedule() -> Schedule {
        Schedule {
            id: "s1".into(), name: "磁盘检测".into(), workspace_id: Some("w1".into()), target_agent_id: None,
            message: "磁盘快满了".into(), kind: ScheduleKind::Condition,
            interval_minutes: Some(5), at_time: None, weekday: None, once_at: None,
            check_script: Some("exit 0".into()), check_shell: Some("sh".into()),
            trigger_mode: Some("edge".into()), cooldown_minutes: None,
            last_check_at: None, last_check_status: None, last_check_output: None,
            last_condition_met: false, consecutive_errors: 0, created_by: None,
            next_run_at: 0, last_run_at: None, enabled: true, created_at: 0, updated_at: 0,
        }
    }

    fn request() -> CreateScheduleRequest {
        CreateScheduleRequest {
            name: "磁盘检测".into(), workspace_id: Some("w1".into()), target_agent_id: None,
            message: "磁盘快满了".into(), kind: ScheduleKind::Condition,
            interval_minutes: None, at_time: None, weekday: None, once_at: None,
            check_script: Some("  exit 0  ".into()), check_shell: Some("sh".into()),
            trigger_mode: None, cooldown_minutes: None,
        }
    }

    #[test]
    fn a_met_check_fires_on_the_first_success_then_stays_quiet_in_edge_mode() {
        let mut s = condition_schedule();
        let first = evaluate_check(&s, &CheckStatus::Met, 1_000);
        assert_eq!(first, CheckDecision { condition_met: Some(true), consecutive_errors: 0, fired: true, disable: false });

        s.last_condition_met = true; // 上一次检测已经成立
        let second = evaluate_check(&s, &CheckStatus::Met, 2_000);
        assert!(!second.fired, "一直成立不能反复触发");

        let cleared = evaluate_check(&s, &CheckStatus::NotMet, 3_000);
        assert_eq!(cleared.condition_met, Some(false));
        s.last_condition_met = false;
        assert!(evaluate_check(&s, &CheckStatus::Met, 4_000).fired, "恢复成立后再次触发");
    }

    #[test]
    fn errors_keep_the_last_known_state_and_disable_the_task_after_too_many_in_a_row() {
        let mut s = condition_schedule();
        s.last_condition_met = true;
        s.consecutive_errors = MAX_CONSECUTIVE_CHECK_ERRORS - 2;

        let one = evaluate_check(&s, &CheckStatus::Error, 1_000);
        assert_eq!(one.condition_met, None, "出错不能改变上次的结论");
        assert!(!one.fired && !one.disable);

        s.consecutive_errors = MAX_CONSECUTIVE_CHECK_ERRORS - 1;
        let last = evaluate_check(&s, &CheckStatus::Error, 2_000);
        assert!(last.disable, "连续出错达到上限要自动停用");
        assert_eq!(last.consecutive_errors, MAX_CONSECUTIVE_CHECK_ERRORS);

        // 一次成功的检测会清零错误计数
        s.consecutive_errors = 3;
        assert_eq!(evaluate_check(&s, &CheckStatus::NotMet, 3_000).consecutive_errors, 0);
    }

    #[test]
    fn repeat_mode_uses_the_cooldown_from_the_last_trigger() {
        let mut s = condition_schedule();
        s.trigger_mode = Some("repeat".into());
        s.cooldown_minutes = Some(10);
        s.last_condition_met = true;
        s.last_run_at = Some(0);
        assert!(!evaluate_check(&s, &CheckStatus::Met, 5 * 60_000).fired);
        assert!(evaluate_check(&s, &CheckStatus::Met, 10 * 60_000).fired);
    }

    #[test]
    fn condition_requests_are_validated_and_filled_with_defaults() {
        let ok = normalize_condition_request(request()).unwrap();
        assert_eq!(ok.check_script.as_deref(), Some("exit 0"), "脚本两端的空白要去掉");
        assert_eq!(ok.interval_minutes, Some(5));
        assert_eq!(ok.trigger_mode.as_deref(), Some("edge"));
        assert_eq!(ok.cooldown_minutes, None);

        let mut repeat = request();
        repeat.trigger_mode = Some("repeat".into());
        assert_eq!(normalize_condition_request(repeat).unwrap().cooldown_minutes, Some(60));

        let mut zero = request();
        zero.interval_minutes = Some(0);
        assert_eq!(normalize_condition_request(zero).unwrap().interval_minutes, Some(1), "间隔至少 1 分钟");

        let mut empty = request();
        empty.check_script = Some("   ".into());
        assert!(normalize_condition_request(empty).is_err());

        let mut bad_shell = request();
        bad_shell.check_shell = Some("fish".into());
        assert!(normalize_condition_request(bad_shell).is_err());

        let mut no_workspace = request();
        no_workspace.workspace_id = None;
        assert!(normalize_condition_request(no_workspace).is_err());

        let mut bad_mode = request();
        bad_mode.trigger_mode = Some("always".into());
        assert!(normalize_condition_request(bad_mode).is_err());
    }

    #[test]
    fn non_condition_requests_drop_any_script_fields() {
        let mut interval = request();
        interval.kind = ScheduleKind::Interval;
        interval.interval_minutes = Some(30);
        let normalized = normalize_condition_request(interval).unwrap();
        assert!(normalized.check_script.is_none() && normalized.check_shell.is_none(), "非条件任务不能带脚本");
    }

    #[test]
    fn a_new_condition_task_is_checked_right_away() {
        let before = chrono::Utc::now().timestamp_millis();
        let next = compute_initial_next_run_at(&normalize_condition_request(request()).unwrap()).unwrap();
        assert!(next >= before && next <= before + 5_000);
    }
}
