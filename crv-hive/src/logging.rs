use std::time::Instant;

use tonic::{Request, Status};
use tracing::Span;

/// 初始化统一日志系统（全局）。
///
/// - 默认使用 `RUST_LOG` 控制日志级别；
/// - 若未设置 `RUST_LOG`，默认输出 `info`；
/// - 输出格式为文本（适合本地开发/容器日志收集）。
pub fn init_logging() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // 多次调用时避免 panic（测试/多入口场景）
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .try_init();
}

/// 每次 RPC 的统一日志对象：携带 request_id、method、user，并用 Span 贯穿整个调用链。
#[derive(Clone, Debug)]
pub struct HiveLog {
    span: Span,
    started_at: Instant,
}

impl HiveLog {
    pub fn new(method: &'static str) -> Self {
        let request_id = uuid::Uuid::new_v4();
        let span = tracing::info_span!(
            "hive_rpc",
            request_id = %request_id,
            method = method,
            user = tracing::field::Empty
        );

        Self {
            span,
            started_at: Instant::now(),
        }
    }

    /// 从 tonic Request 创建日志对象（当前只取 method；其余信息在 handler 中补充）。
    pub fn from_request<T>(method: &'static str, _req: &Request<T>) -> Self {
        Self::new(method)
    }

    /// 在认证后补充 username 字段。
    pub fn with_user(self, username: impl Into<String>) -> Self {
        let username = username.into();
        self.span
            .record("user", &tracing::field::display(username));
        self
    }

    /// 进入当前 Span（用于在当前作用域内自动附带字段）。
    pub fn enter(&self) -> tracing::span::Entered<'_> {
        self.span.enter()
    }

    pub fn info(&self, msg: &str) {
        tracing::info!(parent: &self.span, "{msg}");
    }

    pub fn warn(&self, msg: &str) {
        tracing::warn!(parent: &self.span, "{msg}");
    }

    pub fn error(&self, msg: &str) {
        tracing::error!(parent: &self.span, "{msg}");
    }

    pub fn debug(&self, msg: &str) {
        tracing::debug!(parent: &self.span, "{msg}");
    }

    pub fn finish_ok(&self) {
        let ms = self.started_at.elapsed().as_millis();
        tracing::info!(parent: &self.span, elapsed_ms = ms, "rpc finished: ok");
    }

    pub fn finish_err(&self, status: &Status) {
        let ms = self.started_at.elapsed().as_millis();
        tracing::warn!(
            parent: &self.span,
            elapsed_ms = ms,
            grpc_code = ?status.code(),
            grpc_message = status.message(),
            "rpc finished: err"
        );
    }
}

