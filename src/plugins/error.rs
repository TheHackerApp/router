use std::fmt::{Display, Formatter};

/// The plugin was registered but disabled
#[derive(Clone, Copy, Debug)]
pub struct PluginDisabledError;

impl std::error::Error for PluginDisabledError {}

impl Display for PluginDisabledError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "plugin registered, but explicitly disabled")
    }
}
