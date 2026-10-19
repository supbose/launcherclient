//! Windows 服务管理：更新前卸载用户自定义服务（卸载后不恢复、不重启）。
//!
//! 通过系统自带 sc.exe 交互，零额外依赖：
//! - 未安装的服务跳过
//! - 运行中的服务先 `sc stop` 并等待停止完成（15s 超时，失败也继续卸载）
//! - `sc delete` 卸载；卸载失败仅记录，不阻塞更新
//!
//! sc.exe 调用经 [`ScRunner`] 抽象：生产用 [`SystemScRunner`]，测试用内存 mock，
//! 避免单元测试触碰真实系统服务。真实冒烟测试 gate 在 `integration` feature 下。

use std::process::Command;
use std::time::{Duration, Instant};

/// 停止等待超时（秒）：超时后仍继续尝试卸载
pub const STOP_TIMEOUT_SECS: u64 = 15;

/// 轮询间隔（毫秒）
pub const POLL_INTERVAL_MS: u64 = 300;

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
    /// 标准输出文本
    pub stdout: String,
}

impl ScOutput {
    /// 构造一条成功响应
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            success: true,
            stdout: stdout.into(),
        }
    }

    /// 构造一条失败响应（非零退出码）
    pub fn failed() -> Self {
        Self {
            success: false,
            stdout: String::new(),
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
pub struct SystemScRunner;

impl ScRunner for SystemScRunner {
    fn run(&self, args: &[&str]) -> std::io::Result<ScOutput> {
        let out = Command::new("sc").args(args).output()?;
        Ok(ScOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
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
fn stop_and_wait(runner: &dyn ScRunner, name: &str, on_event: &dyn Fn(&str), timeout: Duration) -> bool {
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
    stop_and_wait(runner, name, on_event, Duration::from_secs(STOP_TIMEOUT_SECS))
}

/// 卸载单个服务：运行中先停止（失败也继续），然后 `sc delete`。未安装跳过。
///
/// - `stop_timeout`: 停止等待时长；测试可传 [`Duration::ZERO`] 实现零等待
fn uninstall_one(runner: &dyn ScRunner, name: &str, on_event: &dyn Fn(&str), stop_timeout: Duration) {
    match query_state(runner, name) {
        ServiceState::NotInstalled => {
            eprintln!("[服务] {name} 未安装，跳过");
            return;
        }
        ServiceState::Stopped => {}
        ServiceState::Running | ServiceState::Other => {
            let _ = stop_and_wait(runner, name, on_event, stop_timeout);
        }
    }
    on_event(&format!("正在卸载服务 {name}..."));
    match runner.run(&["delete", name]) {
        Ok(o) if o.success => eprintln!("[服务] {name} 已卸载"),
        Ok(o) => eprintln!("[服务] {name} 卸载失败（exit {:?}），继续更新", o.success),
        Err(e) => eprintln!("[服务] {name} 卸载执行失败: {e}，继续更新"),
    }
}

/// 卸载配置的服务列表（安装前调用，阻塞，编排层用 spawn_blocking 包裹）。
///
/// - `runner`: sc.exe 执行器（生产传 [`SystemScRunner`]，测试可传 mock）
/// - `on_event`: 用于 UI 阶段文案；卸载后不恢复、不重启。
pub fn uninstall_configured(runner: &dyn ScRunner, services: &[String], on_event: &dyn Fn(&str)) {
    let stop_timeout = Duration::from_secs(STOP_TIMEOUT_SECS);
    for name in services {
        uninstall_one(runner, name, on_event, stop_timeout);
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
    /// - `stop` / `delete`：默认成功，可用 `stop_ok` / `delete_ok` 单独置为失败
    /// - `io_error`：为 true 时所有调用返回 IO 错误
    #[derive(Default)]
    struct FakeSc {
        queries: Mutex<VecDeque<ScOutput>>,
        stop_ok: Mutex<bool>,
        delete_ok: Mutex<bool>,
        io_error: Mutex<bool>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeSc {
        /// 用给定的 query 响应序列构造 mock（stop/delete 默认成功）
        fn new(queries: Vec<ScOutput>) -> Self {
            Self {
                queries: Mutex::new(queries.into_iter().collect()),
                stop_ok: Mutex::new(true),
                delete_ok: Mutex::new(true),
                ..Self::default()
            }
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

            let ok = match args[0] {
                "query" => {
                    return Ok(self
                        .queries
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .pop_front()
                        .unwrap_or_default());
                }
                "stop" => *self.stop_ok.lock().unwrap_or_else(|e| e.into_inner()),
                "delete" => *self.delete_ok.lock().unwrap_or_else(|e| e.into_inner()),
                _ => true,
            };
            Ok(if ok {
                ScOutput::ok("")
            } else {
                ScOutput::failed()
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
        let ok = stop_and_wait(&sc, SVC, &|e| events.borrow_mut().push(e.to_string()), Duration::ZERO);
        assert!(ok);
        assert_eq!(sc.calls(), vec![vec!["stop", SVC], vec!["query", SVC]]);
        assert_eq!(events.into_inner(), vec!["正在停止服务 qs_test_svc...".to_string()]);
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
        {
            let mut guard = sc.stop_ok.lock().unwrap_or_else(|e| e.into_inner());
            *guard = false;
        }
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
        uninstall_configured(&sc, &[SVC.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        assert_eq!(sc.calls(), vec![vec!["query", SVC]]);
        assert!(events.into_inner().is_empty(), "未安装时不应推送任何事件");
    }

    #[test]
    fn uninstall_deletes_stopped_service_directly() {
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_configured(&sc, &[SVC.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        assert_eq!(
            sc.calls(),
            vec![vec!["query", SVC], vec!["delete", SVC]],
            "已停止的服务应直接卸载，不应先 stop"
        );
        assert_eq!(events.into_inner(), vec!["正在卸载服务 qs_test_svc...".to_string()]);
    }

    #[test]
    fn uninstall_stops_then_deletes_running_service() {
        let sc = FakeSc::new(vec![ScOutput::query_state(4), ScOutput::query_state(1)]);
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_configured(&sc, &[SVC.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        assert_eq!(
            sc.calls(),
            vec![vec!["query", SVC], vec!["stop", SVC], vec!["query", SVC], vec!["delete", SVC]]
        );
        let events = events.into_inner();
        assert_eq!(events[0], "正在停止服务 qs_test_svc...");
        assert_eq!(events[1], "正在卸载服务 qs_test_svc...");
    }

    #[test]
    fn uninstall_continues_after_stop_timeout() {
        // 停止超时后仍继续卸载：query(运行) → stop → query(仍运行) → 超时 → delete
        let sc = FakeSc::new(vec![ScOutput::query_state(4), ScOutput::query_state(4)]);
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_one(&sc, SVC, &|e| events.borrow_mut().push(e.to_string()), Duration::ZERO);
        assert_eq!(
            sc.calls(),
            vec![vec!["query", SVC], vec!["stop", SVC], vec!["query", SVC], vec!["delete", SVC]],
            "超时后必须仍执行 delete"
        );
        let events = events.into_inner();
        assert!(events.iter().any(|e| e.contains("正在停止服务")), "应推送停止事件");
        assert!(events.iter().any(|e| e.contains("正在卸载服务")), "应推送卸载事件");
    }

    #[test]
    fn uninstall_delete_failure_does_not_panic() {
        // sc delete 返回非零退出码：仅记录，不向上抛错
        let sc = FakeSc::new(vec![ScOutput::query_state(1)]);
        {
            let mut guard = sc.delete_ok.lock().unwrap_or_else(|e| e.into_inner());
            *guard = false;
        }
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_configured(&sc, &[SVC.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        assert_eq!(sc.calls(), vec![vec!["query", SVC], vec!["delete", SVC]]);
        assert_eq!(events.into_inner(), vec!["正在卸载服务 qs_test_svc...".to_string()]);
    }

    #[test]
    fn uninstall_configured_handles_multiple_services() {
        let sc = FakeSc::new(vec![
            ScOutput::query_state(4), // svc-a 运行中
            ScOutput::query_state(1), // svc-a 已停止
            ScOutput::query_state(1), // svc-b 已停止
        ]);
        let names = vec!["svc-a".to_string(), "svc-b".to_string()];
        uninstall_configured(&sc, &names, &|_| {});
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
    }

    #[test]
    fn uninstall_configured_empty_list_makes_no_calls() {
        let sc = FakeSc::new(vec![]);
        uninstall_configured(&sc, &[], &|_| {});
        assert!(sc.calls().is_empty());
    }

    #[test]
    fn sc_output_query_state_helper_shape() {
        let out = ScOutput::query_state(4);
        assert!(out.success);
        assert!(out.stdout.contains("STATE"));
        assert_eq!(ScOutput::failed(), ScOutput { success: false, stdout: String::new() });
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
        assert_eq!(query_state(&runner, NAME), ServiceState::Running, "前置: Spooler 应在运行");

        assert!(stop_service(&runner, NAME, &|_| {}), "停止应成功");
        assert_eq!(query_state(&runner, NAME), ServiceState::Stopped);

        // 恢复系统服务 → 轮询等待回到运行态（sc start 返回时可能仍在 START_PENDING）
        start_service(NAME);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert_ne!(query_state(&runner, NAME), ServiceState::NotInstalled, "服务不应消失");
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
            .run(&["create", NAME, "binPath=", &format!("\"{}\"", bin.display()), "start=", "demand"])
            .unwrap_or_default();
        assert!(out.success, "sc create 应成功");
        assert_eq!(query_state(&runner, NAME), ServiceState::Stopped, "新建服务应为停止态");

        // 卸载（事件文案被推送）→ 服务不存在
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_configured(&runner, &[NAME.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        let events = events.into_inner();
        assert_eq!(query_state(&runner, NAME), ServiceState::NotInstalled, "卸载后服务应不存在");
        assert!(events.iter().any(|e| e.contains("正在卸载服务")), "应推送卸载事件文案");

        // 对已卸载的服务再次执行 → 未安装跳过，不报错
        uninstall_configured(&runner, &[NAME.to_string()], &|_| {});
        assert_eq!(query_state(&runner, NAME), ServiceState::NotInstalled);
    }
}
