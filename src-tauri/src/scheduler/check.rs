// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! 条件任务的检测脚本运行器与触发判断。
//!
//! 约定：脚本**退出码为 0 表示条件成立**，非 0 表示不成立；脚本输出的文字（截断后）
//! 会附在触发消息后面。脚本的启动、超时、输出截断都收在 [`run_check_script`] 里，
//! 以后要加审批、白名单或沙箱，只需要改这一处。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// 单次检测脚本的最长运行时间，超时会被强制结束并按"出错"处理。
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// 检测输出最多保留的字节数，超出部分截断。
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024;

/// 管道里最多读进内存的字节数（比输出上限多一点，让 `truncate_output` 能判断是否被截断）。
const READ_CAP: usize = MAX_OUTPUT_BYTES + 1024;

/// 脚本结束后等管道读完的最长时间。
const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(2);

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

    // 脚本自己放进独立进程组，超时/结束后可以连同它启动的后台子进程一起清理
    #[cfg(unix)]
    command.process_group(0);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return error_outcome(format!("无法启动 {program}：{e}"), started),
    };
    let pid = child.id();
    // Windows：脚本进程放进 Job Object，guard 活到本函数结束，丢弃时杀掉整个进程树
    #[cfg(windows)]
    let _job = JobGuard::attach(&child);

    // 边读边丢：只保留前 READ_CAP 字节，其余读出来直接扔掉，既不撑内存也不让脚本因管道写满而卡住
    let stdout_buf = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf = Arc::new(Mutex::new(Vec::new()));
    let stdout_task = child.stdout.take().map(|pipe| tokio::spawn(drain_bounded(pipe, stdout_buf.clone())));
    let stderr_task = child.stderr.take().map(|pipe| tokio::spawn(drain_bounded(pipe, stderr_buf.clone())));

    let waited = tokio::time::timeout(timeout, child.wait()).await;
    // 不管正常结束还是超时，都把进程树清掉：脚本里 `sleep 600 &` 之类的后台进程不能留下来累积
    kill_process_tree(pid);

    if waited.is_err() {
        let _ = child.kill().await;
        for task in [&stdout_task, &stderr_task].into_iter().flatten() {
            task.abort();
        }
        return error_outcome(format!("检测脚本运行超过 {} 秒，已被强制结束", timeout.as_secs()), started);
    }
    // 进程树已清理，管道很快就会 EOF；仍然给个上限，防止个别后台进程把管道攥住
    for task in [stdout_task, stderr_task].into_iter().flatten() {
        if tokio::time::timeout(PIPE_DRAIN_GRACE, task).await.is_err() {
            // 超时的任务在 timeout 里已被丢弃（detach），缓冲区里已有的内容照样可用
        }
    }

    match waited.unwrap() {
        Err(e) => error_outcome(format!("等待检测脚本结束时出错：{e}"), started),
        Ok(status) => {
            let stdout = String::from_utf8_lossy(&stdout_buf.lock().unwrap()).into_owned();
            let stderr = String::from_utf8_lossy(&stderr_buf.lock().unwrap()).into_owned();
            let text = if stdout.trim().is_empty() { stderr } else { stdout };
            CheckOutcome {
                status: if status.success() { CheckStatus::Met } else { CheckStatus::NotMet },
                output: truncate_output(&text),
                exit_code: status.code(),
                duration_ms: started.elapsed().as_millis() as u64,
            }
        }
    }
}

/// 读到 EOF 为止，缓冲区最多保留 `READ_CAP` 字节，多出来的读掉丢弃。
async fn drain_bounded<R: tokio::io::AsyncRead + Unpin>(mut pipe: R, buf: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0u8; 4096];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut buf = buf.lock().unwrap();
                let room = READ_CAP.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..n.min(room)]);
            }
        }
    }
}

/// 结束脚本进程及其所有后代（Unix：杀整个进程组）。进程已经退出时是空操作。
/// Windows 不走这里：根进程退出后 `taskkill /T` 找不到它的后代，改用 [`JobGuard`]。
#[cfg(unix)]
fn kill_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(windows)]
fn kill_process_tree(_pid: Option<u32>) {}

/// Windows 的进程树清理：把脚本进程放进一个 Job Object，设置"句柄关闭时杀掉作业内所有进程"。
/// Guard 被丢弃（函数结束或超时）时，脚本及其所有后代——即使根进程早已退出——一并结束。
/// 注意：进程从创建到被放进作业之间有极短的窗口，窗口内启动的子进程不受控；脚本一启动就
/// fork 后台进程再立刻退出的极端情况不保证清理。
#[cfg(windows)]
struct JobGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl JobGuard {
    fn attach(child: &tokio::process::Child) -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let process = child.raw_handle()?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job as usize == 0 {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if configured == 0 || AssignProcessToJobObject(job, process as _) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(JobGuard(job))
        }
    }
}

#[cfg(windows)]
impl Drop for JobGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for JobGuard {}

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

    #[cfg(unix)]
    #[tokio::test]
    async fn endless_output_is_bounded_and_does_not_hang_the_script() {
        // yes 不停写 stdout；上限内读够就丢弃，脚本被超时结束，不应把内存撑爆或卡死
        let outcome = run_check_script("sh", "yes aaaaaaaaaaaaaaaa", Duration::from_secs(1)).await;
        assert_eq!(outcome.status, CheckStatus::Error);
        let outcome = run_check_script("sh", "yes aaaaaaaaaaaaaaaa | head -c 5000000; exit 0", Duration::from_secs(10)).await;
        assert_eq!(outcome.status, CheckStatus::Met);
        assert!(outcome.output.len() <= MAX_OUTPUT_BYTES + 64, "输出长度 {}", outcome.output.len());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn background_descendants_are_killed_on_timeout() {
        let marker = std::env::temp_dir().join(format!("baiyu-bg-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        // 后台进程 2 秒后写标记文件；超时（1 秒）清理进程树后，它不应该还活着
        let script = format!("(sleep 2; touch {}) & wait", marker.display());
        let outcome = run_check_script("sh", &script, Duration::from_secs(1)).await;
        assert_eq!(outcome.status, CheckStatus::Error);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "后台子进程在超时后仍在运行");
        let _ = std::fs::remove_file(&marker);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn background_descendants_are_killed_after_normal_exit_too() {
        let marker = std::env::temp_dir().join(format!("baiyu-bg2-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("(sleep 2; touch {}) >/dev/null 2>&1 & exit 0", marker.display());
        let outcome = run_check_script("sh", &script, Duration::from_secs(10)).await;
        assert_eq!(outcome.status, CheckStatus::Met);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "脚本正常退出后遗留的后台进程没有被清理");
        let _ = std::fs::remove_file(&marker);
    }
}
