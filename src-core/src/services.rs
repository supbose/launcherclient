//! Windows 服务管理：更新前卸载用户自定义服务（卸载后不恢复、不重启）。
//!
//! 通过系统自带 sc.exe 交互，零额外依赖：
//! - 未安装的服务跳过
//! - 运行中的服务先 `sc stop` 并等待停止完成（15s 超时，失败也继续卸载）
//! - `sc delete` 卸载；卸载失败仅记录，不阻塞更新

use std::process::Command;
use std::time::{Duration, Instant};

/// 停止等待超时（秒）：超时后仍继续尝试卸载
const STOP_TIMEOUT_SECS: u64 = 15;

/// 轮询间隔（毫秒）
const POLL_INTERVAL_MS: u64 = 300;

/// 服务状态（sc query STATE 行数字码：1=STOPPED 4=RUNNING，其余为过渡/暂停态）
#[derive(Debug, PartialEq, Eq)]
enum ServiceState {
    Stopped,
    Running,
    Other,
    NotInstalled,
}

/// 查询服务状态。sc.exe 的 STATE/RUNNING 标签不随系统语言本地化，按数字码解析。
fn query_state(name: &str) -> ServiceState {
    let Ok(out) = Command::new("sc").arg("query").arg(name).output() else {
        return ServiceState::NotInstalled;
    };
    if !out.status.success() {
        // 典型退出码 1060：指定的服务未安装
        return ServiceState::NotInstalled;
    }
    let text = String::from_utf8_lossy(&out.stdout);
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
/// 返回 true = 已停止（或停止过程中被移除）；false = 等待超时。
fn stop_and_wait(name: &str, on_event: &dyn Fn(&str)) -> bool {
    on_event(&format!("正在停止服务 {name}..."));
    if let Err(e) = Command::new("sc").args(["stop", name]).output() {
        eprintln!("[服务] {name} sc stop 执行失败: {e}（继续等待状态确认）");
    }
    let deadline = Instant::now() + Duration::from_secs(STOP_TIMEOUT_SECS);
    loop {
        match query_state(name) {
            ServiceState::Stopped | ServiceState::NotInstalled => return true,
            _ => {}
        }
        if Instant::now() >= deadline {
            eprintln!("[服务] {name} 停止等待超时（{STOP_TIMEOUT_SECS}s）");
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

/// 卸载单个服务：运行中先停止（失败也继续），然后 `sc delete`。未安装跳过。
fn uninstall_one(name: &str, on_event: &dyn Fn(&str)) {
    match query_state(name) {
        ServiceState::NotInstalled => {
            eprintln!("[服务] {name} 未安装，跳过");
            return;
        }
        ServiceState::Stopped => {}
        ServiceState::Running | ServiceState::Other => {
            let _ = stop_and_wait(name, on_event);
        }
    }
    on_event(&format!("正在卸载服务 {name}..."));
    match Command::new("sc").args(["delete", name]).output() {
        Ok(o) if o.status.success() => eprintln!("[服务] {name} 已卸载"),
        Ok(o) => eprintln!("[服务] {name} 卸载失败（exit {:?}），继续更新", o.status.code()),
        Err(e) => eprintln!("[服务] {name} 卸载执行失败: {e}，继续更新"),
    }
}

/// 卸载配置的服务列表（安装前调用，阻塞，编排层用 spawn_blocking 包裹）。
///
/// `on_event` 用于 UI 阶段文案；卸载后不恢复、不重启。
pub fn uninstall_configured(services: &[String], on_event: &dyn Fn(&str)) {
    for name in services {
        uninstall_one(name, on_event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：sc start 启动服务（用于系统服务测后恢复）
    fn start_service(name: &str) {
        let _ = Command::new("sc").args(["start", name]).output();
    }

    /// 真实 sc.exe 冒烟：停止运行中服务的等待逻辑（系统服务 Spooler，测完恢复）。
    #[test]
    fn stop_running_service_waits_for_stopped() {
        const NAME: &str = "Spooler";
        assert_eq!(query_state(NAME), ServiceState::Running, "前置: Spooler 应在运行");

        assert!(stop_and_wait(NAME, &|_| {}), "停止应成功");
        assert_eq!(query_state(NAME), ServiceState::Stopped);

        // 恢复系统服务 → 轮询等待回到运行态（sc start 返回时可能仍在 START_PENDING）
        start_service(NAME);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert_ne!(query_state(NAME), ServiceState::NotInstalled, "服务不应消失");
            if query_state(NAME) == ServiceState::Running {
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
        // 清理可能的旧残留
        let _ = Command::new("sc").args(["delete", NAME]).output();

        // 创建临时服务（demand-start，无需真正运行；binPath 用测试进程自身 exe，无害）。
        // sc.exe 的参数解析要求 "binPath=" 与值分开传（同参数合一会报 Invalid field）。
        let bin = std::env::current_exe().unwrap();
        let out = Command::new("sc")
            .args([
                "create",
                NAME,
                "binPath=",
                &format!("\"{}\"", bin.display()),
                "start=",
                "demand",
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "sc create 应成功");
        assert_eq!(query_state(NAME), ServiceState::Stopped, "新建服务应为停止态");

        // 卸载（事件文案被推送）→ 服务不存在
        let events = std::cell::RefCell::new(Vec::new());
        uninstall_configured(&[NAME.to_string()], &|e| events.borrow_mut().push(e.to_string()));
        let events = events.into_inner();
        assert_eq!(query_state(NAME), ServiceState::NotInstalled, "卸载后服务应不存在");
        assert!(events.iter().any(|e| e.contains("正在卸载服务")), "应推送卸载事件文案");

        // 对已卸载的服务再次执行 → 未安装跳过，不报错
        uninstall_configured(&[NAME.to_string()], &|_| {});
        assert_eq!(query_state(NAME), ServiceState::NotInstalled);
    }
}
