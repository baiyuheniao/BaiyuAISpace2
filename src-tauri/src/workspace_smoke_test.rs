// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Agent Team Mode 的 Workspace 模块所用的临时冒烟测试脚手架。
//! 只有设置了 `BAIYU_WORKSPACE_SMOKE_TEST` 环境变量时才会运行（见 `main.rs`）——
//! 不属于正式发布的功能。通过真实的 Tauri 命令端到端驱动两个本地 Ollama 模型，
//! 这样无需前端界面、也无需任何云端 API Key 就能验证多 Agent 之间的工具调用。
//! 待 Phase 1 验证完毕后，可以安全删除这个文件以及 `main.rs` 里挂它的那个钩子。

use crate::db::DbState;
use crate::workspace::commands::{PendingQuestions, PendingSleepRequests, WorkspaceState};
use crate::workspace::types::{AgentRole, AgentStatus, CreateAgentRequest, CreateWorkspaceRequest};
use std::time::Duration;
use tauri::{AppHandle, Listener, Manager};

pub async fn run(app_handle: AppHandle) {
    log::info!("[smoke_test] 开始 Workspace 冒烟测试");

    let db_state = app_handle.state::<DbState>();
    let workspace = match crate::workspace::commands::workspace_create(
        CreateWorkspaceRequest {
            name: "冒烟测试工作组".to_string(),
            description: "本地 Ollama 模型端到端验证".to_string(),
            max_agents: Some(5),
        },
        db_state.clone(),
    )
    .await
    {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("[smoke_test] 创建工作组失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] 工作组已创建: {}", workspace.id);

    let workspace_state = app_handle.state::<WorkspaceState>();

    let main_agent = match crate::workspace::commands::workspace_create_agent_manual(
        CreateAgentRequest {
            workspace_id: workspace.id.clone(),
            name: "主管小Q".to_string(),
            role: AgentRole::Main,
            provider: "local".to_string(),
            model: "qwen3.5:4b".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_config_id: String::new(),
            system_prompt: "你是工作组的主管 Agent。收到任务后，第一步必须调用 workspace_agent_list \
                工具查看组里有哪些其他 Agent；查到后，第二步必须调用 workspace_message 工具，把目标 \
                Agent 的 id 填到 to_agent_id，对它说一句简短的打招呼内容。完成这两步之后，再用普通文字 \
                简单总结一下你做了什么。"
                .to_string(),
            mcp_server_ids: vec![],
            knowledge_base_ids: vec![],
            active_skill_ids: vec![],
            rag_top_k: 5,
            rag_retrieval_mode: "hybrid".to_string(),
            rag_reranker_config_id: None,
            rag_reranker_base_url: None,
            rag_reranker_model: None,
            rag_rerank_top_n: None,
            require_tool_approval: true,
            enable_thinking: false,
            max_tool_rounds: 20,
            history_limit: 40,
            max_tokens: None,
            tool_whitelist: vec![],
        },
        app_handle.clone(),
        workspace_state.clone(),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            log::error!("[smoke_test] 创建主 Agent 失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] 主 Agent 已创建: {} ({})", main_agent.name, main_agent.id);

    let sub_agent = match crate::workspace::commands::workspace_create_agent_manual(
        CreateAgentRequest {
            workspace_id: workspace.id.clone(),
            name: "助理小G".to_string(),
            role: AgentRole::Sub,
            provider: "local".to_string(),
            model: "gemma4:e4b".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_config_id: String::new(),
            system_prompt: "你是工作组里的助理 Agent。收到别人的消息后，用一两句话简短礼貌地回应即可，不需要调用任何工具。"
                .to_string(),
            mcp_server_ids: vec![],
            knowledge_base_ids: vec![],
            active_skill_ids: vec![],
            rag_top_k: 5,
            rag_retrieval_mode: "hybrid".to_string(),
            rag_reranker_config_id: None,
            rag_reranker_base_url: None,
            rag_reranker_model: None,
            rag_rerank_top_n: None,
            require_tool_approval: true,
            enable_thinking: false,
            max_tool_rounds: 20,
            history_limit: 40,
            max_tokens: None,
            tool_whitelist: vec![],
        },
        app_handle.clone(),
        workspace_state.clone(),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            log::error!("[smoke_test] 创建子 Agent 失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] 子 Agent 已创建: {} ({})", sub_agent.name, sub_agent.id);

    if let Err(e) = crate::workspace::commands::workspace_send_user_message(
        workspace.id.clone(),
        main_agent.id.clone(),
        format!("请确认一下当前工作组里有哪些 Agent，然后跟 {} 打个招呼。", sub_agent.name),
        app_handle.clone(),
    )
    .await
    {
        log::error!("[smoke_test] 发送初始消息失败: {}", e);
        return;
    }
    log::info!("[smoke_test] 已向主 Agent 发送初始消息，等待处理...");

    let mut saw_running = false;
    for i in 0..40 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let agents = match crate::workspace::commands::workspace_list_agents(workspace.id.clone(), db_state.clone()).await {
            Ok(a) => a,
            Err(e) => {
                log::error!("[smoke_test] 查询 Agent 列表失败: {}", e);
                continue;
            }
        };
        if agents.iter().any(|a| a.status == AgentStatus::Running) {
            saw_running = true;
        }
        let all_idle = agents.iter().all(|a| a.status == AgentStatus::Idle);
        log::info!(
            "[smoke_test] 第 {} 次检查（{}s）：{}",
            i + 1,
            (i + 1) * 3,
            agents.iter().map(|a| format!("{}={:?}", a.name, a.status)).collect::<Vec<_>>().join(", ")
        );
        if saw_running && all_idle {
            break;
        }
    }

    match crate::workspace::commands::workspace_list_messages(workspace.id.clone(), None, db_state.clone()).await {
        Ok(messages) => {
            log::info!("[smoke_test] ===== 消息记录 ({} 条) =====", messages.len());
            for m in &messages {
                log::info!("[smoke_test]   [{} -> {}] {}", m.from_agent_id, m.to_agent_id, m.content);
            }
        }
        Err(e) => log::error!("[smoke_test] 查询消息失败: {}", e),
    }

    match crate::workspace::commands::workspace_list_logs(workspace.id.clone(), None, db_state.clone()).await {
        Ok(logs) => {
            log::info!("[smoke_test] ===== 日志记录 ({} 条) =====", logs.len());
            for l in &logs {
                log::info!("[smoke_test]   [{}] {}", l.kind, l.content);
            }
        }
        Err(e) => log::error!("[smoke_test] 查询日志失败: {}", e),
    }

    log::info!("[smoke_test] ===== Phase 1 验证结束，开始 Phase 2 验证（提问 / 休眠审批）=====");

    // workspace_asks 一触发就自动代答，模拟前端的提问卡片交互——
    // 用来验证那条永远要经由用户处理的 `ask` 流程。
    let answer_handle = app_handle.clone();
    app_handle.once_any("workspace://question", move |event| {
        let handle = answer_handle.clone();
        let payload = event.payload().to_string();
        tauri::async_runtime::spawn(async move {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else { return };
            let Some(question_id) = value.get("questionId").and_then(|v| v.as_str()) else { return };
            log::info!("[smoke_test] 监听到提问事件，自动代用户回答: {}", question_id);
            let pending = handle.state::<PendingQuestions>();
            if let Err(e) = crate::workspace::commands::workspace_resolve_question(
                question_id.to_string(),
                "可以，辛苦了，批准休息。".to_string(),
                pending,
                handle.clone(),
            )
            .await
            {
                log::error!("[smoke_test] 自动回答问题失败: {}", e);
            }
        });
    });

    // 安全兜底：如果主 Agent 30 秒内没有自行批准这个休眠申请，就走用户代为覆盖
    // 的路径，避免冒烟测试卡到完整的 10 分钟超时。如果主 Agent 抢先处理了，
    // 下面这行 `workspace_resolve_sleep_request` 只会因为"找不到"而失败，
    // 这本身就是预期中的正常结果。
    let sleep_handle = app_handle.clone();
    app_handle.once_any("workspace://sleep-request", move |event| {
        let handle = sleep_handle.clone();
        let payload = event.payload().to_string();
        tauri::async_runtime::spawn(async move {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else { return };
            let Some(request_id) = value.get("requestId").and_then(|v| v.as_str()).map(str::to_string) else { return };
            log::info!("[smoke_test] 监听到休眠申请事件: {}，给主 Agent 30s 自行批准的机会", request_id);
            tokio::time::sleep(Duration::from_secs(30)).await;
            let pending = handle.state::<PendingSleepRequests>();
            match crate::workspace::commands::workspace_resolve_sleep_request(request_id.clone(), true, pending, handle.clone()).await {
                Ok(()) => log::info!("[smoke_test] 主 Agent 30s 内没批准，已通过用户代为批准覆盖: {}", request_id),
                Err(_) => log::info!("[smoke_test] 主 Agent 已经自己处理过这个休眠申请: {}", request_id),
            }
        });
    });

    if let Err(e) = crate::workspace::commands::workspace_send_user_message(
        workspace.id.clone(),
        sub_agent.id.clone(),
        "现在请你调用 workspace_asks 工具，问用户：'我已经完成手头的工作，能否休息一下？'。等用户回答之后，\
         再调用 workspace_sleep 工具申请进入休眠状态，reason 填'已完成打招呼任务'。"
            .to_string(),
        app_handle.clone(),
    )
    .await
    {
        log::error!("[smoke_test] 发送 Phase 2 触发消息失败: {}", e);
        return;
    }
    log::info!("[smoke_test] 已向子 Agent 发送 Phase 2 触发消息，等待处理...");

    for i in 0..40 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let agents = match crate::workspace::commands::workspace_list_agents(workspace.id.clone(), db_state.clone()).await {
            Ok(a) => a,
            Err(e) => {
                log::error!("[smoke_test] 查询 Agent 列表失败: {}", e);
                continue;
            }
        };
        log::info!(
            "[smoke_test] Phase 2 第 {} 次检查（{}s）：{}",
            i + 1,
            (i + 1) * 3,
            agents.iter().map(|a| format!("{}={:?}", a.name, a.status)).collect::<Vec<_>>().join(", ")
        );
        if agents.iter().any(|a| a.role == AgentRole::Sub && a.status == AgentStatus::Sleeping) {
            break;
        }
    }

    // 主 Agent 的验收唤醒（在上面所有子 Agent 都进入 Sleeping 状态后触发）
    // 是在它自己的循环周期里发生的——多给它一点时间真正跑起来并回复，
    // 再去查看结果。
    log::info!("[smoke_test] 等待主 Agent 完成验收回复...");
    for i in 0..15 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let agents = match crate::workspace::commands::workspace_list_agents(workspace.id.clone(), db_state.clone()).await {
            Ok(a) => a,
            Err(_) => continue,
        };
        log::info!(
            "[smoke_test] 验收等待第 {} 次检查（{}s）：{}",
            i + 1,
            (i + 1) * 3,
            agents.iter().map(|a| format!("{}={:?}", a.name, a.status)).collect::<Vec<_>>().join(", ")
        );
        if agents.iter().filter(|a| a.role == AgentRole::Main).all(|a| a.status == AgentStatus::Idle) {
            break;
        }
    }

    match crate::workspace::commands::workspace_list_messages(workspace.id.clone(), None, db_state.clone()).await {
        Ok(messages) => {
            log::info!("[smoke_test] ===== Phase 2 后完整消息记录 ({} 条) =====", messages.len());
            for m in &messages {
                log::info!("[smoke_test]   [{} -> {}] {}", m.from_agent_id, m.to_agent_id, m.content);
            }
        }
        Err(e) => log::error!("[smoke_test] 查询消息失败: {}", e),
    }

    match crate::workspace::commands::workspace_list_logs(workspace.id.clone(), None, db_state.clone()).await {
        Ok(logs) => {
            log::info!("[smoke_test] ===== Phase 2 后完整日志记录 ({} 条) =====", logs.len());
            for l in &logs {
                log::info!("[smoke_test]   [{}] {}", l.kind, l.content);
            }
        }
        Err(e) => log::error!("[smoke_test] 查询日志失败: {}", e),
    }

    log::info!("[smoke_test] ===== Phase 2 验证结束，开始 Phase 3 验证（会议机制）=====");

    // Phase 3 使用一个全新的工作组，避免受到 Phase 1/2 残留状态的污染。
    let meeting_ws = match crate::workspace::commands::workspace_create(
        CreateWorkspaceRequest {
            name: "Phase3 会议测试工作组".to_string(),
            description: "测试 workspace_meeting 轮流发言".to_string(),
            max_agents: Some(5),
        },
        db_state.clone(),
    )
    .await
    {
        Ok(ws) => ws,
        Err(e) => {
            log::error!("[smoke_test] Phase 3 创建工作组失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] Phase 3 工作组已创建: {}", meeting_ws.id);

    let p3_main = match crate::workspace::commands::workspace_create_agent_manual(
        CreateAgentRequest {
            workspace_id: meeting_ws.id.clone(),
            name: "主持人小Q".to_string(),
            role: AgentRole::Main,
            provider: "local".to_string(),
            model: "qwen3.5:4b".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_config_id: String::new(),
            system_prompt: "你是工作组的主持人。收到开会指令后，第一步必须立即调用 workspace_meeting \
                工具，tool 的 topic 参数填用户提到的议题名称。工具调用完成后，用一两句话做总结即可。"
                .to_string(),
            mcp_server_ids: vec![],
            knowledge_base_ids: vec![],
            active_skill_ids: vec![],
            rag_top_k: 5,
            rag_retrieval_mode: "hybrid".to_string(),
            rag_reranker_config_id: None,
            rag_reranker_base_url: None,
            rag_reranker_model: None,
            rag_rerank_top_n: None,
            require_tool_approval: true,
            enable_thinking: false,
            max_tool_rounds: 20,
            history_limit: 40,
            max_tokens: None,
            tool_whitelist: vec![],
        },
        app_handle.clone(),
        workspace_state.clone(),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            log::error!("[smoke_test] Phase 3 创建主 Agent 失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] Phase 3 主 Agent 已创建: {}", p3_main.name);

    let p3_sub = match crate::workspace::commands::workspace_create_agent_manual(
        CreateAgentRequest {
            workspace_id: meeting_ws.id.clone(),
            name: "参会小G".to_string(),
            role: AgentRole::Sub,
            provider: "local".to_string(),
            model: "gemma4:e4b".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_config_id: String::new(),
            system_prompt: "你是工作组的助理。收到【会议通知】消息时，就议题简短发表 2-3 句话的看法，\
                用普通文字回复即可，不需要调用任何工具。"
                .to_string(),
            mcp_server_ids: vec![],
            knowledge_base_ids: vec![],
            active_skill_ids: vec![],
            rag_top_k: 5,
            rag_retrieval_mode: "hybrid".to_string(),
            rag_reranker_config_id: None,
            rag_reranker_base_url: None,
            rag_reranker_model: None,
            rag_rerank_top_n: None,
            require_tool_approval: true,
            enable_thinking: false,
            max_tool_rounds: 20,
            history_limit: 40,
            max_tokens: None,
            tool_whitelist: vec![],
        },
        app_handle.clone(),
        workspace_state.clone(),
    )
    .await
    {
        Ok(a) => a,
        Err(e) => {
            log::error!("[smoke_test] Phase 3 创建子 Agent 失败: {}", e);
            return;
        }
    };
    log::info!("[smoke_test] Phase 3 子 Agent 已创建: {}", p3_sub.name);

    if let Err(e) = crate::workspace::commands::workspace_send_user_message(
        meeting_ws.id.clone(),
        p3_main.id.clone(),
        "请就「今天的工作计划」议题召开一次工作组会议".to_string(),
        app_handle.clone(),
    )
    .await
    {
        log::error!("[smoke_test] Phase 3 发送触发消息失败: {}", e);
        return;
    }
    log::info!("[smoke_test] Phase 3 已向主 Agent 发送开会指令，等待会议流程...");

    let mut saw_activity = false;
    let mut saw_meeting = false;
    for i in 0..60 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let agents = match crate::workspace::commands::workspace_list_agents(meeting_ws.id.clone(), db_state.clone()).await {
            Ok(a) => a,
            Err(e) => {
                log::error!("[smoke_test] Phase 3 查询 Agent 列表失败: {}", e);
                continue;
            }
        };
        if agents.iter().any(|a| a.status != AgentStatus::Idle) {
            saw_activity = true;
        }
        if agents.iter().any(|a| a.status == AgentStatus::Meeting) {
            saw_meeting = true;
        }
        let all_idle = agents.iter().all(|a| a.status == AgentStatus::Idle);
        log::info!(
            "[smoke_test] Phase 3 第 {} 次检查（{}s）：{}",
            i + 1,
            (i + 1) * 3,
            agents.iter().map(|a| format!("{}={:?}", a.name, a.status)).collect::<Vec<_>>().join(", ")
        );
        if saw_activity && all_idle {
            log::info!("[smoke_test] Phase 3 会议流程已结束，所有 Agent 已回到 Idle 状态{}",
                if saw_meeting { "（期间观察到 Meeting 状态）" } else { "（会议太快，轮询未能捕捉到 Meeting 状态）" });
            break;
        }
    }

    match crate::workspace::commands::workspace_list_messages(meeting_ws.id.clone(), None, db_state.clone()).await {
        Ok(messages) => {
            log::info!("[smoke_test] ===== Phase 3 消息记录 ({} 条) =====", messages.len());
            for m in &messages {
                log::info!("[smoke_test]   [{} -> {}] {}", m.from_agent_id, m.to_agent_id, m.content);
            }
        }
        Err(e) => log::error!("[smoke_test] Phase 3 查询消息失败: {}", e),
    }

    match crate::workspace::commands::workspace_list_logs(meeting_ws.id.clone(), None, db_state.clone()).await {
        Ok(logs) => {
            log::info!("[smoke_test] ===== Phase 3 日志记录 ({} 条) =====", logs.len());
            for l in &logs {
                log::info!("[smoke_test]   [{}] {}", l.kind, l.content);
            }
        }
        Err(e) => log::error!("[smoke_test] Phase 3 查询日志失败: {}", e),
    }

    log::info!("[smoke_test] Workspace 冒烟测试结束");
}
