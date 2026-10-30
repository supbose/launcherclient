//! 共享 HTTP Client（进程级单例）
//!
//! 每请求新建 reqwest Client 的代价：连接池与 TLS 会话全部作废重新建。
//! manifest 故障转移 N 个地址 = N 次完整握手，下载再建一个 —— 弱网下
//! 检测/启动明显变慢。
//!
//! 两个单例分别服务检测与下载语义（超时策略不同，不能合并）：
//! - `manifest_client`：连接 3s + 总超时 5s（弱网快速失败，防拖慢启动）
//! - `download_client`：连接 10s、无总超时（大文件慢速下载可合理超过
//!   任何固定上限，读空闲超时在 `download_file` 的流式循环内逐 chunk
//!   控制，不依赖 Client 级超时）

use std::sync::OnceLock;
use std::time::Duration;

/// manifest 检测用共享 Client（连接 3s + 总超时 5s）
pub fn manifest_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        build_client("manifest", || {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
        })
    })
}

/// 下载用共享 Client（连接 10s，无总超时）
pub fn download_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        build_client("download", || {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .build()
        })
    })
}

/// 构建 Client；失败时记录原因并回退系统默认配置
///
/// 原实现是 `unwrap_or_default()`：构建失败（TLS 后端初始化异常等）会被
/// 无声吞掉，超时策略退化成默认值，弱网下表现为检测/下载莫名其妙变慢。
/// 回退本身仍要发生（没有 Client 连 manifest 都拉不了），但必须留下日志。
fn build_client<F, E>(kind: &str, build: F) -> reqwest::Client
where
    F: FnOnce() -> Result<reqwest::Client, E>,
    E: std::fmt::Display,
{
    match build() {
        Ok(client) => client,
        Err(e) => {
            eprintln!(
                "[HTTP] {} Client 构建失败（{}），回退系统默认配置（自定义超时不再生效）",
                kind, e
            );
            reqwest::Client::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_client_is_singleton() {
        // 同一进程内多次获取必须是同一实例（连接池/TLS 会话复用才有意义）
        assert!(std::ptr::eq(manifest_client(), manifest_client()));
    }

    #[test]
    fn download_client_is_singleton() {
        assert!(std::ptr::eq(download_client(), download_client()));
    }

    #[test]
    fn manifest_and_download_clients_are_distinct() {
        // 检测与下载语义不同（超时策略），必须是两个实例
        assert!(!std::ptr::eq(manifest_client(), download_client()));
    }

    #[test]
    fn build_client_falls_back_when_builder_fails() {
        // 构建失败不得 panic，必须回退到一个仍可发请求的默认 Client
        let client = build_client("test", || Err::<reqwest::Client, _>("模拟构建失败"));
        assert!(
            client.get("http://127.0.0.1:9/").build().is_ok(),
            "回退 Client 应仍可构造请求"
        );
    }
}
