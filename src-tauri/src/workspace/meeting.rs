// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent Team Mode 的会议协调：点名发言制。
//!
//! 与会 Agent 调用 `workspace_meeting_checkin` 签到后，工具调用就挂起在
//! `reply` 上（挂起的等待不产生任何模型调用）。协调器每轮只放行**当前被
//! 点名的发言人**：把它错过的发言按 `seen` 进度一次性补给它，并要求它在
//! 下一次签到里带上自己的发言；其余听众继续挂起，直到轮到自己或散会。
//! 这样一场 S 条发言、N 人参加的会议只需要约 S + N 次模型调用——旧的
//! 屏障式协议每条发言都放行全员重新签到，成本是 S × N，其中绝大多数调用
//! 只是空喊"我还在听"（默认 30 条发言 × 5 人 ≈ 150 次真实 API 调用，
//! 约 120 次毫无信息量）。
//!
//! 发言另以静默广播消息落库（不唤醒任何 Agent，见
//! `send_workspace_message_silent`），既让前端时间线实时可见，也让散会后的
//! 历史重建保留会议内容，避免"会上说过的话散会就忘"。
//!
//! 结束条件：主持人（发起人）在签到时传 `end_meeting=true`；或发言数达到
//! 上限（默认 30，发起时可自定义）后，协调器把发言权交给主持人做总结收场。
//! 被点名的发言人有签到时限，超时标记缺席、发言权顺延；缺席者之后补签到会
//! 自动重新入会，并在下次被点名或散会时补收错过的发言。
//!
//! 散会时还原与会者状态：入场前在休眠的还原成休眠（开会不该吞掉"任务已
//! 完成"的声明，否则"全员休眠→触发验收"会被一场会议打断），其余复位 Idle。

use super::commands::{
    insert_workspace_log, load_agent, send_workspace_message_silent, set_agent_status,
};
use super::types::AgentStatus;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, oneshot};

pub const DEFAULT_MAX_SPEECHES: u32 = 30;
pub const MAX_MAX_SPEECHES: u32 = 200;
/// 被点名的发言人从放行到交回发言的时限，超时标记缺席、发言权顺延。
const TURN_TIMEOUT_SECS: u64 = 180;

/// 一次签到：与会 Agent 调用 `workspace_meeting_checkin`（发起人则是
/// `workspace_meeting` 本身）后，其工具调用挂起在 `reply` 上等协调器放行。
pub struct MeetingCheckIn {
    pub agent_id: String,
    /// 轮到该 Agent 发言时的发言内容；听众签到为 None。
    pub content: Option<String>,
    /// 主持人专用：结束会议。非主持人传了会被忽略。
    pub end_meeting: bool,
    pub reply: oneshot::Sender<serde_json::Value>,
}

pub struct MeetingHandle {
    pub meeting_id: String,
    pub tx: mpsc::Sender<MeetingCheckIn>,
}

/// 每个工作组同时最多一场会议，key 是 workspace_id。
#[derive(Default)]
pub struct MeetingsState(pub Arc<Mutex<HashMap<String, MeetingHandle>>>);

pub struct MeetingConfig {
    pub meeting_id: String,
    pub workspace_id: String,
    pub topic: String,
    pub initiator_id: String,
    /// (agent_id, 名字)，同时是发言顺序：发起人在前，其余按创建顺序。
    pub participants: Vec<(String, String)>,
    pub max_speeches: u32,
    /// 入场前处于休眠状态的与会者，散会时还原成休眠而不是 Idle。
    pub sleeping_before: HashSet<String>,
}

struct Waiter {
    reply: oneshot::Sender<serde_json::Value>,
    /// 签到携带的发言内容；被采纳后置 None，避免下次被点名时重复采纳。
    content: Option<String>,
    end_meeting: bool,
}

/// 从 `from` 的下一位开始，找第一个未缺席的与会者下标（可能绕回 `from`
/// 自己——全场只剩一个人时它就自己连讲）；全员缺席返回 None。
fn next_present(cfg: &MeetingConfig, absent: &HashSet<String>, from: usize) -> Option<usize> {
    let n = cfg.participants.len();
    for step in 1..=n {
        let idx = (from + step) % n;
        if !absent.contains(&cfg.participants[idx].0) {
            return Some(idx);
        }
    }
    None
}

/// 放行一个与会者去发言：补发它按 `seen` 进度错过的全部发言（不含它自己
/// 说过的），并把它的进度推到最新。
fn your_turn_payload(
    cfg: &MeetingConfig,
    seen: &mut HashMap<String, usize>,
    agent_id: &str,
    transcript: &[(String, String, String)],
    wrap_up_requested: bool,
) -> serde_json::Value {
    let idx = seen.get(agent_id).copied().unwrap_or(0);
    let new_speeches: Vec<_> = transcript[idx..]
        .iter()
        .filter(|(sid, _, _)| sid != agent_id)
        .map(|(_, name, content)| serde_json::json!({ "speaker": name, "content": content }))
        .collect();
    seen.insert(agent_id.to_string(), transcript.len());
    let wrap_up_hint = if wrap_up_requested {
        "会议已达到发言上限：请在这次发言中总结会议结论，并同时把 end_meeting 设为 true 结束会议。"
    } else {
        ""
    };
    serde_json::json!({
        "status": "your_turn",
        "meetingId": cfg.meeting_id,
        "topic": cfg.topic,
        "newSpeeches": new_speeches,
        "speechCount": transcript.len(),
        "instruction": format!(
            "轮到你发言了。请立即再次调用 workspace_meeting_checkin（meeting_id 填 \"{}\"），把你对议题的发言写进 content 参数。{}",
            cfg.meeting_id, wrap_up_hint
        ),
    })
}

pub async fn run_coordinator(app_handle: AppHandle, cfg: MeetingConfig, mut rx: mpsc::Receiver<MeetingCheckIn>) {
    // 已签到、正挂起等待的与会者（含刚交完发言的人）。
    let mut waiting: HashMap<String, Waiter> = HashMap::new();
    let mut absent: HashSet<String> = HashSet::new();
    // (speaker_id, speaker_name, 发言内容)
    let mut transcript: Vec<(String, String, String)> = Vec::new();
    // 每个与会者已收到的发言进度（transcript 下标）；点名/散会时靠它补发。
    let mut seen: HashMap<String, usize> = cfg.participants.iter().map(|(id, _)| (id.clone(), 0)).collect();
    // 当前被点名发言的人（participants 下标）；第一轮是发起人的开场发言。
    let mut speaker_pos: usize = 0;
    let mut wrap_up_requested = false;
    let mut end_reason: Option<String> = None;

    while end_reason.is_none() {
        let (speaker_id, speaker_name) = cfg.participants[speaker_pos].clone();
        let mut released_this_turn = false;
        let mut speech: Option<String> = None;

        // ---- 取得发言人的发言（或判定它缺席/弃权）----
        let deadline = tokio::time::Instant::now() + Duration::from_secs(TURN_TIMEOUT_SECS);
        loop {
            // 挂起中的发言人：有现成发言就直接采纳；没有就放行它、请它发言。
            let mut needs_release = false;
            if let Some(w) = waiting.get_mut(&speaker_id) {
                match w.content.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                    Some(text) => {
                        speech = Some(text);
                    }
                    None => needs_release = true,
                }
            }
            if speech.is_some() {
                break;
            }
            if needs_release {
                if released_this_turn {
                    // 放行过一次却交回了空发言：视为弃权。不再反复放行——那只会
                    // 让一个不肯发言的模型无限空转烧 API 调用，发言权顺延即可。
                    insert_workspace_log(&app_handle, &cfg.workspace_id, Some(speaker_id.clone()), "meeting",
                        format!("「{}」放弃本轮发言，发言权顺延", speaker_name)).await;
                    break;
                }
                released_this_turn = true;
                if let Some(w) = waiting.remove(&speaker_id) {
                    let payload = your_turn_payload(&cfg, &mut seen, &speaker_id, &transcript, wrap_up_requested);
                    let _ = w.reply.send(payload);
                }
                continue;
            }
            // 发言人不在场（还没签到 / 被放行后尚未交回发言）：等它，
            // 等待期间照常接纳其他人的签到挂起。
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    absent.insert(speaker_id.clone());
                    insert_workspace_log(&app_handle, &cfg.workspace_id, Some(speaker_id.clone()), "meeting",
                        format!("「{}」未在时限内签到发言，本场会议标记缺席，发言权顺延", speaker_name)).await;
                    break;
                }
                msg = rx.recv() => match msg {
                    Some(ci) => {
                        if !cfg.participants.iter().any(|(id, _)| *id == ci.agent_id) {
                            let _ = ci.reply.send(serde_json::json!({ "error": "你不是本次会议的与会者" }));
                            continue;
                        }
                        absent.remove(&ci.agent_id);
                        waiting.insert(ci.agent_id.clone(), Waiter { reply: ci.reply, content: ci.content, end_meeting: ci.end_meeting });
                    }
                    None => {
                        end_reason = Some("会议通道已关闭（工作组可能已被删除）".to_string());
                        break;
                    }
                }
            }
        }
        if end_reason.is_some() {
            break;
        }

        // ---- 记录本轮发言 + 静默落库 ----
        if let Some(text) = &speech {
            transcript.push((speaker_id.clone(), speaker_name.clone(), text.clone()));
            send_workspace_message_silent(&app_handle, &cfg.workspace_id, &speaker_id, "all",
                &format!("【会议发言 · {}】{}", cfg.topic, text)).await;
        }

        // ---- 结束条件 ----
        let chair_wants_end = waiting.get(&cfg.initiator_id).map(|w| w.end_meeting).unwrap_or(false);
        // 主持人不在发言轮却带着结束陈词来散会（比如缺席后补签到）：陈词也记入纪要。
        if chair_wants_end && speaker_id != cfg.initiator_id {
            let closing = waiting
                .get_mut(&cfg.initiator_id)
                .and_then(|w| w.content.take())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if let Some(text) = closing {
                let chair_name = cfg
                    .participants
                    .iter()
                    .find(|(id, _)| *id == cfg.initiator_id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_default();
                transcript.push((cfg.initiator_id.clone(), chair_name, text.clone()));
                send_workspace_message_silent(&app_handle, &cfg.workspace_id, &cfg.initiator_id, "all",
                    &format!("【会议发言 · {}】{}", cfg.topic, text)).await;
            }
        }

        let cap_reached = transcript.len() as u32 >= cfg.max_speeches;
        let chair_absent = absent.contains(&cfg.initiator_id);
        if chair_wants_end {
            end_reason = Some("主持人宣布会议结束".to_string());
            break;
        }
        if wrap_up_requested && speaker_id == cfg.initiator_id {
            end_reason = Some(if speech.is_some() {
                "会议达到发言上限，主持人总结后结束".to_string()
            } else {
                "会议达到发言上限（主持人未作总结，自动结束）".to_string()
            });
            break;
        }
        if cap_reached && chair_absent {
            end_reason = Some("会议达到发言上限（主持人缺席，自动结束）".to_string());
            break;
        }

        // ---- 指定下一位发言人 ----
        if cap_reached {
            // 触到上限：不再轮转，把发言权交给主持人做总结收场。
            wrap_up_requested = true;
            speaker_pos = cfg.participants.iter().position(|(id, _)| *id == cfg.initiator_id).unwrap_or(0);
        } else {
            match next_present(&cfg, &absent, speaker_pos) {
                Some(next) => speaker_pos = next,
                None => {
                    end_reason = Some("所有与会者均缺席".to_string());
                    break;
                }
            }
        }
    }

    // ---- 收尾：给仍在等待的与会者发会议结束结果，注销会议，还原状态 ----
    let reason = end_reason.unwrap_or_else(|| "会议结束".to_string());
    let transcript_json: Vec<_> = transcript
        .iter()
        .map(|(_, name, content)| serde_json::json!({ "speaker": name, "content": content }))
        .collect();
    let ended_payload = |aid: &str| {
        serde_json::json!({
            "status": "meeting_ended",
            "topic": cfg.topic,
            "reason": reason,
            "transcript": transcript_json,
            "instruction": if aid == cfg.initiator_id {
                "会议已结束。请用普通文字向用户简要汇报会议结论和后续安排。"
            } else {
                "会议已结束，发言纪要已存档。请用一两句普通文字向用户说明你从会议中得到的结论或接下来打算做的事。"
            },
        })
    };
    for (aid, w) in waiting.drain() {
        let _ = w.reply.send(ended_payload(&aid));
    }
    // 清掉可能还排队在通道里的迟到签到，别让它们干等到超时。
    while let Ok(ci) = rx.try_recv() {
        let _ = ci.reply.send(ended_payload(&ci.agent_id));
    }

    {
        let meetings = app_handle.state::<MeetingsState>();
        let mut map = meetings.0.lock().unwrap();
        if map.get(&cfg.workspace_id).map(|h| h.meeting_id == cfg.meeting_id).unwrap_or(false) {
            map.remove(&cfg.workspace_id);
        }
    }
    for (id, _) in &cfg.participants {
        if matches!(load_agent(&app_handle, id).await, Ok(Some(a)) if a.status == AgentStatus::Meeting) {
            let restore = if cfg.sleeping_before.contains(id) { AgentStatus::Sleeping } else { AgentStatus::Idle };
            set_agent_status(&app_handle, id, restore).await;
        }
    }
    insert_workspace_log(&app_handle, &cfg.workspace_id, Some(cfg.initiator_id.clone()), "meeting",
        format!("会议「{}」结束（{}），共 {} 条发言", cfg.topic, reason, transcript.len())).await;
}
