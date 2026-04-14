//! Plugin logger with automatic target prefixing.
//!
//! The [`PluginLogger`] wraps the `log` crate and automatically
//! sets the log target to `basalt::plugin::<name>`, ensuring
//! consistent, filterable log output across all plugins.

/// A logger scoped to a specific plugin.
///
/// Obtained via [`ServerContext::logger()`](crate::ServerContext::logger).
/// All messages are logged with target `basalt::plugin::<name>`,
/// making them easy to filter in log output.
///
/// # Example
///
/// ```ignore
/// registrar.on::<PlayerJoinedEvent>(Stage::Post, 0, |event, ctx| {
///     let log = ctx.logger();
///     log.info(&format!("{} joined", event.info.username));
///     log.debug("sending welcome message");
/// });
/// ```
pub struct PluginLogger {
    target: String,
}

impl PluginLogger {
    /// Creates a new logger for the given plugin name.
    pub(crate) fn new(plugin_name: &str) -> Self {
        Self {
            target: format!("basalt::plugin::{plugin_name}"),
        }
    }

    /// Logs at ERROR level.
    pub fn error(&self, msg: &str) {
        log::log!(target: &self.target, log::Level::Error, "{msg}");
    }

    /// Logs at WARN level.
    pub fn warn(&self, msg: &str) {
        log::log!(target: &self.target, log::Level::Warn, "{msg}");
    }

    /// Logs at INFO level.
    pub fn info(&self, msg: &str) {
        log::log!(target: &self.target, log::Level::Info, "{msg}");
    }

    /// Logs at DEBUG level.
    pub fn debug(&self, msg: &str) {
        log::log!(target: &self.target, log::Level::Debug, "{msg}");
    }

    /// Logs at TRACE level.
    pub fn trace(&self, msg: &str) {
        log::log!(target: &self.target, log::Level::Trace, "{msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logger_target_format() {
        let logger = PluginLogger::new("chat");
        assert_eq!(logger.target, "basalt::plugin::chat");
    }

    #[test]
    fn logger_does_not_panic() {
        let logger = PluginLogger::new("test");
        // These should not panic even without a logger initialized
        logger.error("test error");
        logger.warn("test warn");
        logger.info("test info");
        logger.debug("test debug");
        logger.trace("test trace");
    }
}
