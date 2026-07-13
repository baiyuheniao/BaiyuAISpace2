// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent Team Mode 的屏障式（barrier-style）会议协调。
//!
//! 会议采用"集合点"同步协议：所有与会 Agent 都挂起在
//! `workspace_meeting_checkin` 工具调用上，协调器等人到齐后统一放行，把新
//! 发言作为工具结果同时发给所有人——发言因此直接进入每个与会者当前唤醒的
//! 模型上下文，人人都能听到彼此在说什么。放行时同时指定下一位发言人。
//!
//! 发言另以静默广播消息落库（不唤醒任何 Agent，见
//! `send_workspace_message_silent`），既让前端时间线实时可见，也让散会后的
//! 历史重建保留会议内容，避免"会上说过的话散会就忘"。
//!
//! 结束条件：主持人（发起人）在任意一次签到时传 `end_meeting=true`；或发言
//! 数达到上限（默认 30，发起时可自定义）后，协调器把发言权交给主持人做总结
//! 收场。每轮集合有超时，超时未签到的与会者标记缺席、不再等待；缺席者之后
//! 补签到会自动重新入会，并补收缺席期间错过的发言。

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
/// 每轮集合（等所有未缺席与会者签到）的时限，超时者标记缺席。
const CYCLE_TIMEOUT_SECS: u64 = 180;

/// 一次签到：与会 Agent 调用 `workspace_meeting_checkin`（发起人则是
/// `workspace_meeting` 本身）后，其工具调用挂起在 `reply` 上等协调器放行。
pub struct MeetingCheckIn {
    pub agent_id: String,
    /// 轮到该 Agent 发言时的发言内容；未轮到发言的签到为 None。
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
}

struct Waiter {
    reply: oneshot::Sender<serde_json::Value>,
    content: Option<String>,
    end_meeting: bool,
}

pub async fn run_coordinator(app_handle: AppHandle, cfg: MeetingConfig, mut rx: mpsc::Receiver<MeetingCheckIn>) {
    let mut waiting: HashMap<String, Waiter> = HashMap::new();
    let mut absent: HashSet<String> = HashSet::new();
    // (speaker_id, speaker_name, 发言内容)
    let mut transcript: Vec<(String, String, String)> = Vec::new();
    // 每个与会者已收到的发言进度（transcript 下标）；缺席补签的人靠它补收错过的发言。
    let mut seen: HashMap<String, usize> = cfg.participants.iter().map(|(id, _)| (id.clone(), 0)).collect();
    // 本轮被指定发言的人（participants 下标）；第一轮是发起人的开场发言。
    let mut speaker_pos: usize = 0;
    let mut wrap_up_requested = false;
    let mut end_reason: Option<String> = None;

    while end_reason.is_none() {
        // ---- 集合阶段：等所有未缺席的与会者签到 ----
        let deadline = tokio::time::Instant::now() + Duration::from_secs(CYCLE_TIMEOUT_SECS);
        loop {
            let all_present = cfg.participants.iter().all(|(id, _)| absent.contains(id) || waiting.contains_key(id));
            if all_present && !waiting.is_empty() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    for (id, name) in &cfg.participants {
                        if !waiting.contains_key(id) && !absent.contains(id) {
                            absent.insert(id.clone());
                            insert_workspace_log(&app_handle, &cfg.workspace_id, Some(id.clone()), "meeting",
                                format!("「{}」未在时限内签到，本场会议标记缺席", name)).await;
                        }
                    }
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
        if waiting.is_empty() {
            end_reason = Some("所有与会者均缺席".to_string());
            break;
        }

        // ---- 记录本轮发言 ----
        let (speaker_id, speaker_name) = cfg.participants[speaker_pos].clone();
        let chair_wants_end = waiting.get(&cfg.initiator_id).map(|w| w.end_meeting).unwrap_or(false);

        let speech = waiting
            .get(&speaker_id)
            .and_then(|w| w.content.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(text) = &speech {
            transcript.push((speaker_id.clone(), speaker_name.clone(), text.clone()));
            send_workspace_message_silent(&app_handle, &cfg.workspace_id, &speaker_id, "all",
                &format!("【会议发言 · {}】{}", cfg.topic, text)).await;
        }
        // 主持人不在发言轮却带着结束陈词来散会：陈词也记入纪要。
        if chair_wants_end && speaker_id != cfg.initiator_id {
            let closing = waiting
                .get(&cfg.initiator_id)
                .and_then(|w| w.content.clone())
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
            end_reason = Some("会议达到发言上限，主持人总结后结束".to_string());
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
            let n = cfg.participants.len();
            let mut next = (speaker_pos + 1) % n;
            for _ in 0..n {
                if !absent.contains(&cfg.participants[next].0) {
                    break;
                }
                next = (next + 1) % n;
            }
            speaker_pos = next;
        }
        let (next_id, next_name) = cfg.participants[speaker_pos].clone();

        // ---- 放行：把新发言作为工具结果发给每个等待中的与会者 ----
        for (aid, w) in waiting.drain() {
            let idx = seen.get(&aid).copied().unwrap_or(0);
            let new_speeches: Vec<_> = transcript[idx..]
                .iter()
                .filter(|(sid, _, _)| *sid != aid)
                .map(|(_, name, content)| serde_json::json!({ "speaker": name, "content": content }))
                .collect();
            seen.insert(aid.clone(), transcript.len());
            let payload = if aid == next_id {
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
            } else {
                serde_json::json!({
                    "status": "listening",
                    "meetingId": cfg.meeting_id,
                    "topic": cfg.topic,
                    "newSpeeches": new_speeches,
                    "speechCount": transcript.len(),
                    "nextSpeaker": next_name,
                    "instruction": format!(
                        "请听取以上发言。下一位发言人是「{}」。请立即再次调用 workspace_meeting_checkin（meeting_id 填 \"{}\"，content 留空）签到等待。",
                        next_name, cfg.meeting_id
                    ),
                })
            };
            let _ = w.reply.send(payload);
        }
    }

    // ---- 收尾：给仍在等待的与会者发会议结束结果，注销会议，复位状态 ----
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
            set_agent_status(&app_handle, id, AgentStatus::Idle).await;
        }
    }
    insert_workspace_log(&app_handle, &cfg.workspace_id, Some(cfg.initiator_id.clone()), "meeting",
        format!("会议「{}」结束（{}），共 {} 条发言", cfg.topic, reason, transcript.len())).await;
}
