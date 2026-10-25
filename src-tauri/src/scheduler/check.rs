// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 条件任务的检测脚本运行器与触发判断。
//!
//! 约定：脚本**退出码为 0 表示条件成立**，非 0 表示不成立；脚本输出的文字（截断后）
//! 会附在触发消息后面。脚本的启动、超时、输出截断都收在 [`run_check_script`] 里，
//! 以后要加审批、白名单或沙箱，只需要改这一处。

use std::time::{Duration, Instant};
use tokio::process::Command;

/// 单次检测脚本的最长运行时间，超时会被强制结束并按"出错"处理。
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// 检测输出最多保留的字节数，超出部分截断。
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    /// 退出码为 0：条件成立。
    Met,
    /// 退出码非 0：条件不成立。
    NotMet,
    /// 脚本没能正常跑完（找不到解释器、启动失败、超时）。
    Error,
}

impl CheckStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckStatus::Met => "met",
            CheckStatus::NotMet => "not_met",
            CheckStatus::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub status: CheckStatus,
    /// 脚本输出（stdout，没有时用 stderr），已截断；`Error` 时是错误原因。
    pub output: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

/// 解释器对应的可执行文件与参数。解释器名必须在 `CHECK_SHELLS` 里。
fn shell_command(shell: &str, script: &str) -> Result<(String, Vec<String>), String> {
    match shell {
        "powershell" => {
            let program = if cfg!(windows) { "powershell" } else { "pwsh" };
            Ok((
                program.to_string(),
                vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script.to_string()],
            ))
        }
        "cmd" => {
            if cfg!(windows) {
                Ok(("cmd".to_string(), vec!["/C".into(), script.to_string()]))
            } else {
                Err("cmd 只能在 Windows 上使用".to_string())
            }
        }
        "sh" => Ok(("sh".to_string(), vec!["-c".into(), script.to_string()])),
        "python" => {
            let program = if cfg!(windows) { "python" } else { "python3" };
            Ok((program.to_string(), vec!["-c".into(), script.to_string()]))
        }
        other => Err(format!("不支持的脚本解释器：{other}")),
    }
}

/// 把输出截断到 `MAX_OUTPUT_BYTES`，且不拆开多字节字符。
pub fn truncate_output(text: &str) -> String {
    let text = text.trim();
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…（已截断）", &text[..end])
}

fn error_outcome(message: String, started: Instant) -> CheckOutcome {
    CheckOutcome {
        status: CheckStatus::Error,
        output: message,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

/// 运行一次检测脚本。永远不会 panic 或返回 Err：所有失败都折叠成 `CheckStatus::Error`。
pub async fn run_check_script(shell: &str, script: &str, timeout: Duration) -> CheckOutcome {
    let started = Instant::now();
    let (program, args) = match shell_command(shell, script) {
        Ok(command) => command,
        Err(message) => return error_outcome(message, started),
    };

    let mut command = Command::new(&program);
    command
        .args(&args)
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // 超时后丢弃 future 时同时结束子进程
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // 不弹出控制台窗口
        command.creation_flags(0x0800_0000);
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return error_outcome(format!("无法启动 {program}：{e}"), started),
    };

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => error_outcome(format!("检测脚本运行超过 {} 秒，已被强制结束", timeout.as_secs()), started),
        Ok(Err(e)) => error_outcome(format!("等待检测脚本结束时出错：{e}"), started),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let text = if stdout.trim().is_empty() { stderr } else { stdout };
            CheckOutcome {
                status: if output.status.success() { CheckStatus::Met } else { CheckStatus::NotMet },
                output: truncate_output(&text),
                exit_code: output.status.code(),
                duration_ms: started.elapsed().as_millis() as u64,
            }
        }
    }
}

/// 一次有明确结论的检测之后，这次要不要触发任务。
///
/// - `edge`：条件由不成立变成立时触发一次；一直成立期间不重复触发。
/// - `repeat`：条件成立期间，距离上次触发超过冷却时间就再触发一次。
pub fn should_fire(
    trigger_mode: &str,
    previously_met: bool,
    met: bool,
    last_fired_ms: Option<i64>,
    now_ms: i64,
    cooldown_ms: i64,
) -> bool {
    if !met {
        return false;
    }
    match trigger_mode {
        "repeat" => last_fired_ms.map_or(true, |last| now_ms - last >= cooldown_ms),
        _ => !previously_met,
    }
}

/// 把检测输出附到触发消息后面，让 Agent 看到"检测到了什么"。
pub fn compose_trigger_message(message: &str, output: &str) -> String {
    if output.trim().is_empty() {
        message.to_string()
    } else {
        format!("{message}\n\n检测脚本输出：\n{output}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_mode_fires_only_when_the_condition_turns_true() {
        assert!(should_fire("edge", false, true, None, 1_000, 0), "不成立 → 成立：触发");
        assert!(!should_fire("edge", true, true, Some(0), 1_000, 0), "一直成立：不重复触发");
        assert!(!should_fire("edge", false, false, None, 1_000, 0), "不成立：不触发");
        assert!(!should_fire("edge", true, false, Some(0), 1_000, 0), "成立 → 不成立：不触发");
        // 没有指定模式时按默认的边沿触发处理
        assert!(should_fire("", false, true, None, 1_000, 0));
    }

    #[test]
    fn repeat_mode_fires_again_only_after_the_cooldown() {
        let cooldown = 60_000;
        assert!(should_fire("repeat", true, true, None, 1_000, cooldown), "从没触发过：触发");
        assert!(!should_fire("repeat", true, true, Some(1_000), 30_000, cooldown), "冷却中：不触发");
        assert!(should_fire("repeat", true, true, Some(1_000), 61_000, cooldown), "冷却结束：再次触发");
        assert!(!should_fire("repeat", true, false, Some(1_000), 99_000, cooldown), "条件不成立：不触发");
    }

    #[test]
    fn trigger_message_appends_the_script_output_only_when_there_is_some() {
        assert_eq!(compose_trigger_message("磁盘告警", ""), "磁盘告警");
        assert_eq!(compose_trigger_message("磁盘告警", "  \n"), "磁盘告警");
        assert_eq!(compose_trigger_message("磁盘告警", "C: 95%"), "磁盘告警\n\n检测脚本输出：\nC: 95%");
    }

    #[test]
    fn truncate_output_never_splits_a_multibyte_character() {
        let long = "汉".repeat(MAX_OUTPUT_BYTES); // 每个字 3 字节
        let truncated = truncate_output(&long);
        assert!(truncated.ends_with("…（已截断）"));
        assert!(truncated.len() <= MAX_OUTPUT_BYTES + "…（已截断）".len());
        assert!(!truncated.contains('\u{FFFD}'));
        assert_eq!(truncate_output("  短输出\n"), "短输出");
    }

    #[test]
    fn unsupported_shells_are_rejected_instead_of_run() {
        assert!(shell_command("fish", "echo hi").is_err());
        assert!(shell_command("sh", "echo hi").is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exit_code_zero_means_met_and_stdout_is_captured() {
        let outcome = run_check_script("sh", "echo 磁盘使用率 95%; exit 0", Duration::from_secs(10)).await;
        assert_eq!(outcome.status, CheckStatus::Met);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.output, "磁盘使用率 95%");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn non_zero_exit_means_not_met_and_falls_back_to_stderr_for_output() {
        let outcome = run_check_script("sh", "echo 没有新文件 1>&2; exit 3", Duration::from_secs(10)).await;
        assert_eq!(outcome.status, CheckStatus::NotMet);
        assert_eq!(outcome.exit_code, Some(3));
        assert_eq!(outcome.output, "没有新文件");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_script_that_overruns_the_timeout_is_killed_and_reported_as_an_error() {
        let started = Instant::now();
        let outcome = run_check_script("sh", "sleep 30", Duration::from_millis(300)).await;
        assert_eq!(outcome.status, CheckStatus::Error);
        assert!(outcome.output.contains("超过"), "{}", outcome.output);
        assert!(started.elapsed() < Duration::from_secs(10), "必须在超时后立刻返回，而不是等脚本跑完");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_interpreter_unavailable_on_this_platform_is_an_error_not_a_panic() {
        // cmd 只存在于 Windows：在其他平台选它要得到明确的错误，而不是 panic 或静默通过
        let outcome = run_check_script("cmd", "echo hi", Duration::from_secs(5)).await;
        assert_eq!(outcome.status, CheckStatus::Error);
        assert!(outcome.output.contains("Windows"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn very_long_output_is_truncated() {
        let outcome = run_check_script("sh", "head -c 20000 /dev/zero | tr '\\0' 'a'", Duration::from_secs(10)).await;
        assert_eq!(outcome.status, CheckStatus::Met);
        assert!(outcome.output.ends_with("…（已截断）"));
    }
}
