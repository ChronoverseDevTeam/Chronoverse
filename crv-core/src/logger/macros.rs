#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)*) => {
        tracing::trace!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        tracing::info!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        tracing::error!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_span {
    ($level:expr, $name:expr) => {
        tracing::span!($level, $name)
    };
    ($level:expr, $name:expr, $($field:tt)*) => {
        tracing::span!($level, $name, $($field)*)
    };
}

