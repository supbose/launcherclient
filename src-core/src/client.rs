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
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    })
}

/// 下载用共享 Client（连接 10s，无总超时）
pub fn download_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default()
    })
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
}
