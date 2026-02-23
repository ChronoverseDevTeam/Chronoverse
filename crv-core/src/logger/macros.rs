#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)*) => {
        $crate::logger::tracing::trace!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logger::tracing::debug!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logger::tracing::info!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logger::tracing::warn!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logger::tracing::error!($($arg)*);
    };
}

#[macro_export]
macro_rules! log_span {
    ($level:expr, $name:expr) => {
        $crate::logger::tracing::span!($level, $name)
    };
    ($level:expr, $name:expr, $($field:tt)*) => {
        $crate::logger::tracing::span!($level, $name, $($field)*)
    };
}
