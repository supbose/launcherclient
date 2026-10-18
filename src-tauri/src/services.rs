//! Windows 服务管理：更新前停止用户自定义服务，更新完成后恢复。
//!
//! 通过系统自带 sc.exe 交互，零额外依赖：
//! - 未安装 / 已停止的服务跳过；运行中的服务 `sc stop` 后轮询等待（15s 超时）
//! - 停止失败（超时/拒绝访问等）：`sc delete` 卸载兜底，保证更新继续
//! - 更新完成后对成功停止且未卸载的服务 `sc start` 恢复

use std::process::Command;
use std::time::{Duration, Instant};

/// 停止等待超时（秒）：超时视为停止失败，走卸载兜底
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

/// 停止单个服务。
///
/// 返回 true 表示服务已被本函数停止、更新完成后需要 `sc start` 恢复；
/// 未安装/原本已停止/停止失败走卸载兜底/卸载成功，均返回 false（无需恢复）。
fn stop_one(name: &str, on_event: &dyn Fn(&str)) -> bool {
    match query_state(name) {
        ServiceState::NotInstalled => {
            eprintln!("[服务] {name} 未安装，跳过");
            false
        }
        ServiceState::Stopped => {
            eprintln!("[服务] {name} 已是停止状态，无需处理");
            false
        }
        ServiceState::Running | ServiceState::Other => {
            on_event(&format!("正在停止服务 {name}..."));
            if let Err(e) = Command::new("sc").args(["stop", name]).output() {
                eprintln!("[服务] {name} sc stop 执行失败: {e}（继续等待状态确认）");
            }
            let deadline = Instant::now() + Duration::from_secs(STOP_TIMEOUT_SECS);
            loop {
                match query_state(name) {
                    ServiceState::Stopped => {
                        eprintln!("[服务] {name} 已停止");
                        return true;
                    }
                    ServiceState::NotInstalled => {
                        eprintln!("[服务] {name} 停止过程中已被移除");
                        return false;
                    }
                    _ => {}
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
            // 停止失败：卸载兜底，保证更新继续（不恢复）
            on_event(&format!("服务 {name} 停止超时，正在卸载..."));
            eprintln!("[服务] {name} 停止超时（{STOP_TIMEOUT_SECS}s），执行卸载兜底");
            match Command::new("sc").args(["delete", name]).output() {
                Ok(o) if o.status.success() => {
                    eprintln!("[服务] {name} 已卸载（更新后不会自动恢复）");
                }
                Ok(o) => eprintln!("[服务] {name} 卸载失败（exit {:?}），继续更新", o.status.code()),
                Err(e) => eprintln!("[服务] {name} 卸载执行失败: {e}，继续更新"),
            }
            false
        }
    }
}

/// 停止配置的服务列表（安装前调用，阻塞，编排层用 spawn_blocking 包裹）。
///
/// `on_event` 用于 UI 阶段文案；返回成功停止、需要在更新完成后恢复的服务名。
pub fn stop_configured(services: &[String], on_event: &dyn Fn(&str)) -> Vec<String> {
    let mut stopped = Vec::new();
    for name in services {
        if stop_one(name, on_event) {
            stopped.push(name.clone());
        }
    }
    stopped
}

/// 恢复更新前被停止的服务（更新收尾调用；启动失败仅记录，不阻断流程）
pub fn start_all(services: &[String]) {
    for name in services {
        match Command::new("sc").args(["start", name]).output() {
            Ok(o) if o.status.success() => eprintln!("[服务] {name} 已重新启动"),
            Ok(o) => eprintln!("[服务] {name} 启动失败（exit {:?}），请手动启动", o.status.code()),
            Err(e) => eprintln!("[服务] {name} 启动执行失败: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 sc.exe 冒烟：低风险的系统服务（打印假脱机，可停可启），测完立即恢复。
    /// 需要管理员权限运行（cargo test 直接以当前用户执行）。
    #[test]
    fn stop_and_restore_real_service() {
        const NAME: &str = "Spooler";

        // 前置：服务在运行
        assert_eq!(query_state(NAME), ServiceState::Running, "前置条件: Spooler 应在运行");
        // 未安装服务名 → NotInstalled 分支
        assert_eq!(query_state("qs_no_such_service_xyz"), ServiceState::NotInstalled);

        // 停止（on_event 文案回调被触发）→ 状态确认
        let events = std::cell::RefCell::new(Vec::new());
        let stopped = stop_configured(&[NAME.to_string()], &|e| {
            events.borrow_mut().push(e.to_string())
        });
        let events = events.into_inner();
        assert_eq!(stopped, vec![NAME.to_string()], "运行中的服务应被停止并列入恢复清单");
        assert_eq!(query_state(NAME), ServiceState::Stopped);
        assert!(events.iter().any(|e| e.contains("正在停止服务")), "应推送停止事件文案");

        // 已停止的服务再检测 → 跳过、不进恢复清单
        let again = stop_configured(&[NAME.to_string()], &|_| {});
        assert!(again.is_empty(), "已停止的服务无需恢复");

        // 恢复 → 轮询等待回到运行态（sc start 返回时可能仍在 START_PENDING 过渡）
        start_all(&stopped);
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
}
