//! Windows 服务管理：更新前卸载用户自定义服务（卸载后不恢复、不重启）。
//!
//! 通过系统自带 sc.exe 交互，零额外依赖：
//! - 未安装的服务跳过
//! - 运行中的服务先 `sc stop` 并等待停止完成（15s 超时，失败也继续卸载）
//! - `sc delete` 卸载；卸载失败返回错误由编排层决定（原 Tauri 版曾：安装前终止，
//!   避免「服务仍注册旧路径、新版本却已装完」的半更新状态）
//!
//! 失败信息必须可现场定位：release 无控制台，UI 上那行详情就是唯一线索。
//! 因此 [`ScOutput`] 会保留退出码与 sc 原始输出，`sc delete` 的失败按退出码分流：
//! - 1060（服务不存在）→ 视为已卸载
//! - 1072（标记删除中）→ 短重试几次，仍失败提示重启
//! - 5（拒绝访问）→ 提示需管理员权限
//! - 其余 → 原样带上退出码与 sc 输出
//!
//! sc.exe 调用经 [`ScRunner`] 抽象：生产用 [`SystemScRunner`]，测试用内存 mock，
//! 避免单元测试触碰真实系统服务。真实冒烟测试 gate 在 `integration` feature 下。

use std::process::Command;
use std::time::{Duration, Instant};

/// 停止等待超时（秒）：超时后仍继续尝试卸载
pub const STOP_TIMEOUT_SECS: u64 = 15;

/// 轮询间隔（毫秒）
pub const POLL_INTERVAL_MS: u64 = 300;

/// sc.exe 退出码：服务不存在（ERROR_SERVICE_DOES_NOT_EXIST）
const SC_SERVICE_DOES_NOT_EXIST: i32 = 1060;

/// sc.exe 退出码：服务已被标记为删除（ERROR_SERVICE_MARKED_FOR_DELETE）
///
/// 典型成因：上一次卸载已把服务标记删除，但仍有进程持有服务句柄
/// （服务进程未完全退出），句柄全部释放前服务处于「僵尸」状态。
const SC_SERVICE_MARKED_FOR_DELETE: i32 = 1072;

/// sc.exe 退出码：拒绝访问（ERROR_ACCESS_DENIED），需管理员权限
const SC_ACCESS_DENIED: i32 = 5;

/// `sc delete` 最大尝试次数（仅对「标记删除中」这类瞬时失败重试）
const DELETE_RETRY_ATTEMPTS: u32 = 4;

/// `sc delete` 重试间隔（毫秒）
const DELETE_RETRY_INTERVAL_MS: u64 = 500;

/// 服务状态（sc query STATE 行数字码：1=STOPPED 4=RUNNING，其余为过渡/暂停态）
#[derive(Debug, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Running,
    Other,
    NotInstalled,
}

/// 一条 sc.exe 命令的执行结果
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScOutput {
    /// 命令是否成功执行（退出码为 0）
    pub success: bool,
    /// 进程退出码（被信号终止等异常场景为 None）
    pub code: Option<i32>,
    /// 标准输出文本（sc.exe 的失败提示也走 stdout）
    pub stdout: String,
    /// 标准错误文本
    pub stderr: String,
}

impl ScOutput {
    /// 构造一条成功响应
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            success: true,
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// 构造一条失败响应（非零退出码，用于 mock）
    pub fn failed() -> Self {
        Self {
            success: false,
            code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// 用指定退出码构造一条失败响应（mock 专用：可模拟 1060/1072/5 等真实码）
    pub fn failed_with_code(code: i32) -> Self {
        Self {
            success: false,
            code: Some(code),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// 失败详情：优先 sc 自己的输出（stdout 通常带 `[SC] DeleteService FAILED 1072:`），
    /// 都没有时退回退出码。用于拼进 UI 错误文案，避免只显示一个空泛的「非零」。
    pub fn detail(&self) -> String {
        let message = self
            .stderr
            .lines()
            .chain(self.stdout.lines())
            .map(str::trim)
            .find(|line| !line.is_empty());
        match (self.code, message) {
            (Some(code), Some(msg)) => format!("退出码 {code}: {msg}"),
            (Some(code), None) => format!("退出码 {code}"),
            (None, Some(msg)) => msg.to_string(),
            (None, None) => "无输出".to_string(),
        }
    }

    /// 用给定 STATE 数字码构造一条 sc query 成功响应
    pub fn query_state(code: u32) -> Self {
        Self::ok(format!(
            "SERVICE_NAME: demo\n        TYPE               : 10  WIN32_OWN_PROCESS \n        STATE              : {code}  RUNNING\n"
        ))
    }
}

/// sc.exe 命令执行抽象：便于在测试中注入假响应，避免触碰真实系统服务
pub trait ScRunner: Send + Sync {
    /// 执行一条 sc 命令（如 `["query", "Spooler"]`），返回输出或 IO 错误
    fn run(&self, args: &[&str]) -> std::io::Result<ScOutput>;
}

/// 真实实现：调用系统 sc.exe
///
/// 用绝对路径而不是裸 `sc`：避免 PATH 里同名程序被优先执行（提权进程尤其敏感）。
pub struct SystemScRunner;

/// sc.exe 绝对路径；取不到 SystemRoot 时退回固定路径
fn sc_exe() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    format!("{root}\\System32\\sc.exe")
}

impl ScRunner for SystemScRunner {
    fn run(&self, args: &[&str]) -> std::io::Result<ScOutput> {
        let out = Command::new(sc_exe()).args(args).output()?;
        Ok(ScOutput {
            success: out.status.success(),
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// 查询服务状态。sc.exe 的 STATE/RUNNING 标签不随系统语言本地化，按数字码解析。
fn query_state(runner: &dyn ScRunner, name: &str) -> ServiceState {
    let Ok(out) = runner.run(&["query", name]) else {
        return ServiceState::NotInstalled;
    };
    if !out.success {
        // 典型退出码 1060：指定的服务未安装
        return ServiceState::NotInstalled;
    }
    let text = &out.stdout;
    for line in text.lines() {
        if line.contains("STATE") {
            if let Some(code) = line
                .split(':')
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|token| token.parse::<u32>().ok())
            {
                return match code {
                    1 => ServiceState::Stopped,
                    4 => ServiceState::Running,
                    _ => ServiceState::Other,
                };
            }
        }
    }
    ServiceState::Other
}

/// 停止服务并等待停止完成。
///
/// - `timeout`: 最长等待时长；传 [`Duration::ZERO`] 时只查询一次（测试用，零等待）
///
/// 返回 true = 已停止（或停止过程中被移除）；false = 等待超时。
fn stop_and_wait(
    runner: &dyn ScRunner,
    name: &str,
    on_event: &dyn Fn(&str),
    timeout: Duration,
) -> bool {
    on_event(&format!("正在停止服务 {name}..."));
    if let Err(e) = runner.run(&["stop", name]) {
        eprintln!("[服务] {name} sc stop 执行失败: {e}（继续等待状态确认）");
    }
    let deadline = Instant::now() + timeout;
    loop {
        match query_state(runner, name) {
            ServiceState::Stopped | ServiceState::NotInstalled => return true,
            _ => {}
        }
        if Instant::now() >= deadline {
            eprintln!("[服务] {name} 停止等待超时（{}s）", timeout.as_secs());
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

/// 停止服务并等待停止完成（默认 [`STOP_TIMEOUT_SECS`] 超时）
fn stop_service(runner: &dyn ScRunner, name: &str, on_event: &dyn Fn(&str)) -> bool {
    stop_and_wait(
        runner,
        name,
        on_event,
        Duration::from_secs(STOP_TIMEOUT_SECS),
    )
}

/// 卸载单个服务：运行中先停止（失败也继续），然后 `sc delete`。未安装跳过。
///
/// - `stop_timeout`: 停止等待时长；测试可传 [`Duration::ZERO`] 实现零等待
///
/// 返回：Ok = 已卸载或本就未安装；Err = 卸载失败详情（交由编排层处置）
fn uninstall_one(
    runner: &dyn ScRunner,
    name: &str,
    on_event: &dyn Fn(&str),
    stop_timeout: Duration,
) -> Result<(), String> {
    match query_state(runner, name) {
        ServiceState::NotInstalled => {
            eprintln!("[服务] {name} 未安装，跳过");
            return Ok(());
        }
        ServiceState::Stopped => {}
        ServiceState::Running | ServiceState::Other => {
            let _ = stop_and_wait(runner, name, on_event, stop_timeout);
        }
    }
    on_event(&format!("正在卸载服务 {name}..."));
    delete_with_retry(
        runner,
        name,
        DELETE_RETRY_ATTEMPTS,
        Duration::from_millis(DELETE_RETRY_INTERVAL_MS),
    )?;
    eprintln!("[服务] {name} 已卸载");
    Ok(())
}

/// 从失败输出里判定 sc.exe 的真实错误码。
///
/// sc.exe 多数情况把 Win32 错误码用作自身退出码，但部分版本/本地化下只退 1，
/// 真实码仅出现在输出文本里（如 `[SC] DeleteService FAILED 1072:`）——两者都要认。
fn failure_code(out: &ScOutput) -> Option<i32> {
    if let Some(code) = out.code {
        if code != 0 && code != 1 {
            return Some(code);
        }
    }
    let text = format!("{}\n{}", out.stdout, out.stderr);
    [SC_SERVICE_DOES_NOT_EXIST, SC_SERVICE_MARKED_FOR_DELETE]
        .into_iter()
        .find(|code| text.contains(&code.to_string()))
}

/// 执行 `sc delete`，按退出码把失败翻译成可现场处置的提示。
///
/// - 成功 / 1060（已不存在）→ Ok
/// - 1072（标记删除中）→ 重试 `attempts` 次（等服务进程释放句柄），仍失败提示重启
/// - 5（拒绝访问）→ 明确提示需要管理员权限
/// - 其余 → 带上退出码与 sc 原始输出
fn delete_with_retry(
    runner: &dyn ScRunner,
    name: &str,
    attempts: u32,
    interval: Duration,
) -> Result<(), String> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match runner.run(&["delete", name]) {
            Ok(out) if out.success => return Ok(()),
            // 服务已不存在：目标达成（并发/上一次卸载已完成）
            Ok(out) if failure_code(&out) == Some(SC_SERVICE_DOES_NOT_EXIST) => return Ok(()),
            Ok(out) if failure_code(&out) == Some(SC_ACCESS_DENIED) => {
                return Err(format!(
                    "服务 {name} 卸载失败：权限不足，更新程序需以管理员身份运行（{}）",
                    out.detail()
                ))
            }
            Ok(out) if failure_code(&out) == Some(SC_SERVICE_MARKED_FOR_DELETE) => {
                if attempt < attempts {
                    eprintln!("[服务] {name} 已被标记删除，第 {attempt} 次重试等待句柄释放");
                    std::thread::sleep(interval);
                    continue;
                }
                return Err(format!(
                    "服务 {name} 卸载失败：服务已被标记删除但仍有进程占用（{}），请重启系统后重试",
                    out.detail()
                ));
            }
            Ok(out) => return Err(format!("服务 {name} 卸载失败（{}）", out.detail())),
            Err(e) => return Err(format!("服务 {name} 卸载执行失败: {e}")),
        }
    }
}

/// 卸载配置的服务列表（安装前调用，阻塞，编排层用 spawn_blocking 包裹）。
///
/// 全部服务都会尝试卸载（单个失败不中断剩余项），最终聚合返回：
/// Ok = 全部成功或未安装；Err = 失败详情列表（非空）。
///
/// - `runner`: sc.exe 执行器（生产传 [`SystemScRunner`]，测试可传 mock）
/// - `on_event`: 用于 UI 阶段文案；卸载后不恢复、不重启。
pub fn uninstall_configured(
    runner: &dyn ScRunner,
    services: &[String],
    on_event: &dyn Fn(&str),
) -> Result<(), Vec<String>> {
    let stop_timeout = Duration::from_secs(STOP_TIMEOUT_SECS);
    let mut failures = Vec::new();
    for name in services {
        if let Err(e) = uninstall_one(runner, name, on_event, stop_timeout) {
            failures.push(e);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// 内存版 sc.exe mock：按命令类型路由响应，并记录全部调用
    ///
    /// - `query`：按序消费 `queries` 队列，耗尽后返回失败（等价于服务未安装）
    /// - `stop` / `delete`：按序消费各自的队列，耗尽后返回成功
    ///   （需要模拟失败时往队列里 push 带退出码的 [`ScOutput`]）
    /// - `io_error`：为 true 时所有调用返回 IO 错误
    #[derive(Default)]
    struct FakeSc {
        queries: Mutex<VecDeque<ScOutput>>,
        stops: Mutex<VecDeque<ScOutput>>,
        deletes: Mutex<VecDeque<ScOutput>>,
        io_error: Mutex<bool>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeSc {
        /// 用给定的 query 响应序列构造 mock（stop/delete 默认成功）
        fn new(queries: Vec<ScOutput>) -> Self {
            Self {
                queries: Mutex::new(queries.into_iter().collect()),
                ..Self::default()
            }
        }

        /// 追加一条 `sc stop` 响应
        fn push_stop(&self, out: ScOutput) {
            self.stops
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(out);
        }

        /// 追加一条 `sc delete` 响应（队列耗尽后默认成功）
        fn push_delete(&self, out: ScOutput) {
            self.deletes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(out);
        }

        /// 所有调用均返回 IO 错误（模拟 sc.exe 无法执行）
        fn with_io_error() -> Self {
            Self {
                io_error: Mutex::new(true),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        /// 某类命令被调用了多少次
        fn count(&self, cmd: &str) -> usize {
            self.calls()
                .iter()
                .filter(|c| c.first().is_some_and(|a| a == cmd))
                .count()
        }
    }

    impl ScRunner for FakeSc {
        fn run(&self, args: &[&str]) -> std::io::Result<ScOutput> {
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(args.iter().map(|s| s.to_string()).collect());

            if *self.io_error.lock().unwrap_or_else(|e| e.into_inner()) {
                return Err(std::io::Error::other("mock io error"));
            }

            let (queue, empty_fallback_ok) = match args[0] {
                "query" => (&self.queries, false),
                "stop" => (&self.stops, true),
                "delete" => (&self.deletes, true),
                _ => return Ok(ScOutput::ok("")),
            };
            let popped = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            Ok(match popped {
                Some(out) => out,
                // query 队列耗尽 → 默认失败 = 服务未安装
                None if !empty_fallback_ok => ScOutput::failed(),
                // stop/delete 队列耗尽 → 默认成功
                None => ScOutput::ok(""),
            })
        }
    }

    const SVC: &str = "qs_test_svc";

    // ---------- query_state ----------

    #[test]
    fn query_state_running_code_4() {
        let sc = FakeSc::new(vec![ScOutput::query_state(4)]);
        assert_eq!(query_state(&sc, SVC), ServiceState::Running);
        assert_eq!(sc.calls(), vec![vec!["query", SVC]]);
    }

    #[test]
    fn query_state_stopped_code_1() {
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        assert_eq!(query_state(&sc, SVC), ServiceState::Stopped);
    }

    #[test]
    fn query_state_transitional_code_3_is_other() {
        let sc = FakeSc::new(vec![ScOutput::query_state(3)]);
        assert_eq!(query_state(&sc, SVC), ServiceState::Other);
    }

    #[test]
    fn query_state_paused_code_7_is_other() {
        let sc = FakeSc::new(vec![ScOutput::query_state(7)]);
        assert_eq!(query_state(&sc, SVC), ServiceState::Other);
    }

    #[test]
    fn query_state_uninstalled_on_failed_command() {
        // 退出码 1060（服务未安装）：success=false
        let sc = FakeSc::new(vec![ScOutput::failed()]);
        assert_eq!(query_state(&sc, SVC), ServiceState::NotInstalled);
    }

    #[test]
    fn query_state_uninstalled_when_queries_exhausted() {
        // 响应耗尽 → 默认失败 → 视为未安装
        let sc = FakeSc::new(vec![]);
        assert_eq!(query_state(&sc, SVC), ServiceState::NotInstalled);
    }

    #[test]
    fn query_state_uninstalled_on_io_error() {
        let sc = FakeSc::with_io_error();
        assert_eq!(query_state(&sc, SVC), ServiceState::NotInstalled);
        assert_eq!(sc.calls(), vec![vec!["query", SVC]]);
    }

    #[test]
    fn query_state_other_when_no_state_line() {
        let sc = FakeSc::new(vec![ScOutput::ok("SERVICE_NAME: demo\n")]);
        assert_eq!(query_state(&sc, SVC), ServiceState::Other);
    }

    // ---------- stop_and_wait ----------

    #[test]
    fn stop_returns_true_when_already_stopped() {
        // 首次查询即停止 → 零 sleep 直接返回
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        let events = std::cell::RefCell::new(Vec::new());
        let ok = stop_and_wait(
            &sc,
            SVC,
            &|e| events.borrow_mut().push(e.to_string()),
            Duration::ZERO,
        );
        assert!(ok);
        assert_eq!(sc.calls(), vec![vec!["stop", SVC], vec!["query", SVC]]);
        assert_eq!(
            events.into_inner(),
            vec!["正在停止服务 qs_test_svc...".to_string()]
        );
    }

    #[test]
    fn stop_returns_true_when_service_removed_during_stop() {
        // 停止过程中服务被移除 → 视为已停止
        let sc = FakeSc::new(vec![ScOutput::failed()]);
        let ok = stop_and_wait(&sc, SVC, &|_| {}, Duration::ZERO);
        assert!(ok);
    }

    #[test]
    fn stop_times_out_when_still_running() {
        // timeout=ZERO：只查询一次即超时，零等待
        let sc = FakeSc::new(vec![ScOutput::query_state(4)]);
        let ok = stop_and_wait(&sc, SVC, &|_| {}, Duration::ZERO);
        assert!(!ok);
        assert_eq!(sc.calls(), vec![vec!["stop", SVC], vec!["query", SVC]]);
    }

    #[test]
    fn stop_waits_until_stopped() {
        // 运行 → 轮询一次后停止（全测试套件中唯一产生 300ms 等待的用例）
        let sc = FakeSc::new(vec![ScOutput::query_state(4), ScOutput::query_state(1)]);
        let ok = stop_and_wait(&sc, SVC, &|_| {}, Duration::from_secs(5));
        assert!(ok);
        assert_eq!(sc.calls().len(), 3);
    }

    #[test]
    fn stop_tolerates_sc_stop_command_failure() {
        // sc stop 返回非零退出码：仅记录，仍继续轮询确认状态
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        sc.push_stop(ScOutput::failed());
        let ok = stop_and_wait(&sc, SVC, &|_| {}, Duration::ZERO);
        assert!(ok);
        assert_eq!(sc.calls(), vec![vec!["stop", SVC], vec!["query", SVC]]);
    }

    #[test]
    fn stop_tolerates_sc_stop_io_error() {
        // sc stop 无法执行（IO 错误）：仅记录，仍继续轮询
        let sc = FakeSc::with_io_error();
        let ok = stop_and_wait(&sc, SVC, &|_| {}, Duration::ZERO);
        // query 也失败 → 判为未安装 → 视为已停止
        assert!(ok);
    }

    // ---------- uninstall_one / uninstall_configured ----------

    #[test]
    fn uninstall_skips_not_installed_service() {
        let sc = FakeSc::new(vec![ScOutput::failed()]);
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_configured(&sc, &[SVC.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        assert_eq!(sc.calls(), vec![vec!["query", SVC]]);
        assert!(events.into_inner().is_empty(), "未安装时不应推送任何事件");
        assert!(result.is_ok(), "服务未安装应视为成功跳过");
    }

    #[test]
    fn uninstall_deletes_stopped_service_directly() {
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_configured(&sc, &[SVC.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        assert_eq!(
            sc.calls(),
            vec![vec!["query", SVC], vec!["delete", SVC]],
            "已停止的服务应直接卸载，不应先 stop"
        );
        assert_eq!(
            events.into_inner(),
            vec!["正在卸载服务 qs_test_svc...".to_string()]
        );
        assert!(result.is_ok());
    }

    #[test]
    fn uninstall_stops_then_deletes_running_service() {
        let sc = FakeSc::new(vec![ScOutput::query_state(4), ScOutput::query_state(1)]);
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_configured(&sc, &[SVC.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        assert_eq!(
            sc.calls(),
            vec![
                vec!["query", SVC],
                vec!["stop", SVC],
                vec!["query", SVC],
                vec!["delete", SVC]
            ]
        );
        let events = events.into_inner();
        assert_eq!(events[0], "正在停止服务 qs_test_svc...");
        assert_eq!(events[1], "正在卸载服务 qs_test_svc...");
        assert!(result.is_ok());
    }

    #[test]
    fn uninstall_continues_after_stop_timeout() {
        // 停止超时后仍继续卸载：query(运行) → stop → query(仍运行) → 超时 → delete
        let sc = FakeSc::new(vec![ScOutput::query_state(4), ScOutput::query_state(4)]);
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_one(
            &sc,
            SVC,
            &|e| events.borrow_mut().push(e.to_string()),
            Duration::ZERO,
        );
        assert_eq!(
            sc.calls(),
            vec![
                vec!["query", SVC],
                vec!["stop", SVC],
                vec!["query", SVC],
                vec!["delete", SVC]
            ],
            "超时后必须仍执行 delete"
        );
        let events = events.into_inner();
        assert!(
            events.iter().any(|e| e.contains("正在停止服务")),
            "应推送停止事件"
        );
        assert!(
            events.iter().any(|e| e.contains("正在卸载服务")),
            "应推送卸载事件"
        );
        assert!(result.is_ok(), "delete 成功则整体成功");
    }

    #[test]
    fn uninstall_delete_failure_returns_err() {
        // sc delete 返回非零退出码：不再静默「继续更新」，返回错误交由编排层终止
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        sc.push_delete(ScOutput::failed());
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_configured(&sc, &[SVC.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        assert_eq!(sc.calls(), vec![vec!["query", SVC], vec!["delete", SVC]]);
        assert_eq!(
            events.into_inner(),
            vec!["正在卸载服务 qs_test_svc...".to_string()]
        );
        let failures = match result {
            Err(f) => f,
            Ok(()) => panic!("卸载失败必须返回 Err"),
        };
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].contains(SVC),
            "错误详情应含服务名: {}",
            failures[0]
        );
        assert!(
            failures[0].contains("退出码 1"),
            "错误详情应含退出码: {}",
            failures[0]
        );
    }

    #[test]
    fn uninstall_continues_after_delete_failure_and_aggregates() {
        // 一个服务卸载失败不中断剩余项：两个服务都应执行 delete，
        // 最终 Err 聚合全部失败（此处 2 条）
        let sc = FakeSc::new(vec![ScOutput::query_state(1), ScOutput::query_state(1)]);
        sc.push_delete(ScOutput::failed());
        sc.push_delete(ScOutput::failed());
        let names = vec!["svc-a".to_string(), "svc-b".to_string()];
        let result = uninstall_configured(&sc, &names, &|_| {});
        assert_eq!(
            sc.calls(),
            vec![
                vec!["query", "svc-a"],
                vec!["delete", "svc-a"],
                vec!["query", "svc-b"],
                vec!["delete", "svc-b"],
            ],
            "单个失败后仍必须卸载剩余服务"
        );
        let failures = match result {
            Err(f) => f,
            Ok(()) => panic!("两个都失败应返回 Err"),
        };
        assert_eq!(failures.len(), 2);
        assert!(failures.iter().any(|f| f.contains("svc-a")));
        assert!(failures.iter().any(|f| f.contains("svc-b")));
    }

    #[test]
    fn uninstall_configured_handles_multiple_services() {
        let sc = FakeSc::new(vec![
            ScOutput::query_state(4), // svc-a 运行中
            ScOutput::query_state(1), // svc-a 已停止
            ScOutput::query_state(1), // svc-b 已停止
        ]);
        let names = vec!["svc-a".to_string(), "svc-b".to_string()];
        let result = uninstall_configured(&sc, &names, &|_| {});
        assert_eq!(
            sc.calls(),
            vec![
                vec!["query", "svc-a"],
                vec!["stop", "svc-a"],
                vec!["query", "svc-a"],
                vec!["delete", "svc-a"],
                vec!["query", "svc-b"],
                vec!["delete", "svc-b"],
            ]
        );
        assert!(result.is_ok());
    }

    #[test]
    fn uninstall_configured_empty_list_makes_no_calls() {
        let sc = FakeSc::new(vec![]);
        let result = uninstall_configured(&sc, &[], &|_| {});
        assert!(sc.calls().is_empty());
        assert!(result.is_ok());
    }

    #[test]
    fn sc_output_query_state_helper_shape() {
        let out = ScOutput::query_state(4);
        assert!(out.success);
        assert!(out.stdout.contains("STATE"));
        assert_eq!(
            ScOutput::failed(),
            ScOutput {
                success: false,
                code: Some(1),
                stdout: String::new(),
                stderr: String::new(),
            }
        );
    }

    // ---------- delete 失败分流（现场定位用） ----------

    /// 走真实 `uninstall_one` 路径，验证 delete 失败文案
    fn run_delete_case(deletes: Vec<ScOutput>) -> Result<(), String> {
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        for out in deletes {
            sc.push_delete(out);
        }
        uninstall_one(&sc, SVC, &|_| {}, Duration::ZERO)
    }

    #[test]
    fn delete_treats_service_not_exist_as_success() {
        // 1060：并发/上一次已删掉，目标已达成，不应阻断更新
        let result = run_delete_case(vec![ScOutput::failed_with_code(SC_SERVICE_DOES_NOT_EXIST)]);
        assert!(result.is_ok(), "1060 应视为已卸载: {result:?}");
    }

    #[test]
    fn delete_access_denied_mentions_admin() {
        // 5：更新程序未提权 → 提示必须管理员运行
        let err = run_delete_case(vec![ScOutput::failed_with_code(SC_ACCESS_DENIED)])
            .expect_err("拒绝访问必须报错");
        assert!(err.contains("管理员"), "应提示提权: {err}");
        assert!(err.contains("退出码 5"), "应带退出码: {err}");
    }

    #[test]
    fn delete_retries_marked_for_delete_then_succeeds() {
        // 1072 一次后成功：重试应吃掉这个瞬时失败
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        sc.push_delete(ScOutput::failed_with_code(SC_SERVICE_MARKED_FOR_DELETE));
        let result = delete_with_retry(&sc, SVC, 3, Duration::ZERO);
        assert!(result.is_ok(), "重试后应成功: {result:?}");
        assert_eq!(sc.count("delete"), 2);
    }

    #[test]
    fn delete_marked_for_delete_exhausted_suggests_reboot() {
        // 1072 持续失败：有限次重试后提示重启，不再无限等待
        let sc = FakeSc::new(vec![]);
        for _ in 0..10 {
            sc.push_delete(ScOutput::failed_with_code(SC_SERVICE_MARKED_FOR_DELETE));
        }
        let err = delete_with_retry(&sc, SVC, 3, Duration::ZERO).expect_err("持续 1072 必须报错");
        assert!(err.contains("重启"), "应提示重启: {err}");
        assert_eq!(sc.count("delete"), 3, "重试次数应受 attempts 约束");
    }

    #[test]
    fn delete_falls_back_to_code_in_output_when_exit_code_is_one() {
        // 部分 sc.exe 版本只退 1，真实码在输出文本里
        let out = ScOutput {
            success: false,
            code: Some(1),
            stdout: "[SC] DeleteService FAILED 1072:\n".to_string(),
            stderr: String::new(),
        };
        assert_eq!(failure_code(&out), Some(SC_SERVICE_MARKED_FOR_DELETE));
    }

    #[test]
    fn delete_generic_failure_keeps_sc_message() {
        let out = ScOutput {
            success: false,
            code: Some(1),
            stdout: "[SC] DeleteService FAILED 1234: 某个未知错误\n".to_string(),
            stderr: String::new(),
        };
        assert_eq!(
            out.detail(),
            "退出码 1: [SC] DeleteService FAILED 1234: 某个未知错误"
        );
    }
}

/// 真实 sc.exe 冒烟测试：触碰真实系统服务，需管理员权限。
///
/// 默认 `cargo test` **不会**执行；显式开启：
/// `cargo test --features integration`
#[cfg(all(test, feature = "integration"))]
mod integration_tests {
    use super::*;

    /// 测试辅助：sc start 启动服务（用于系统服务测后恢复）
    fn start_service(name: &str) {
        let _ = SystemScRunner.run(&["start", name]);
    }

    /// 真实 sc.exe 冒烟：停止运行中服务的等待逻辑（系统服务 Spooler，测完恢复）。
    #[test]
    fn stop_running_service_waits_for_stopped() {
        const NAME: &str = "Spooler";
        let runner = SystemScRunner;
        assert_eq!(
            query_state(&runner, NAME),
            ServiceState::Running,
            "前置: Spooler 应在运行"
        );

        assert!(stop_service(&runner, NAME, &|_| {}), "停止应成功");
        assert_eq!(query_state(&runner, NAME), ServiceState::Stopped);

        // 恢复系统服务 → 轮询等待回到运行态（sc start 返回时可能仍在 START_PENDING）
        start_service(NAME);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert_ne!(
                query_state(&runner, NAME),
                ServiceState::NotInstalled,
                "服务不应消失"
            );
            if query_state(&runner, NAME) == ServiceState::Running {
                break;
            }
            assert!(Instant::now() < deadline, "10s 内未恢复运行");
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    /// 真实 sc.exe 冒烟：临时创建测试服务 → 卸载 → 确认系统无残留。
    #[test]
    fn uninstall_temporary_service() {
        const NAME: &str = "qs_updtest_svc";
        let runner = SystemScRunner;
        // 清理可能的旧残留
        let _ = runner.run(&["delete", NAME]);

        // 创建临时服务（demand-start，无需真正运行；binPath 用测试进程自身 exe，无害）。
        // sc.exe 的参数解析要求 "binPath=" 与值分开传（同参数合一会报 Invalid field）。
        let bin = std::env::current_exe().unwrap_or_default();
        let out = runner
            .run(&[
                "create",
                NAME,
                "binPath=",
                &format!("\"{}\"", bin.display()),
                "start=",
                "demand",
            ])
            .unwrap_or_default();
        assert!(out.success, "sc create 应成功");
        assert_eq!(
            query_state(&runner, NAME),
            ServiceState::Stopped,
            "新建服务应为停止态"
        );

        // 卸载（事件文案被推送）→ 服务不存在
        let events = std::cell::RefCell::new(Vec::new());
        let result = uninstall_configured(&runner, &[NAME.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        let events = events.into_inner();
        assert_eq!(
            query_state(&runner, NAME),
            ServiceState::NotInstalled,
            "卸载后服务应不存在"
        );
        assert!(
            events.iter().any(|e| e.contains("正在卸载服务")),
            "应推送卸载事件文案"
        );
        assert!(result.is_ok(), "真实卸载应成功");

        // 对已卸载的服务再次执行 → 未安装跳过，不报错
        let again = uninstall_configured(&runner, &[NAME.to_string()], &|_| {});
        assert!(again.is_ok(), "已卸载的服务应跳过而非报错");
        assert_eq!(query_state(&runner, NAME), ServiceState::NotInstalled);
    }
}
