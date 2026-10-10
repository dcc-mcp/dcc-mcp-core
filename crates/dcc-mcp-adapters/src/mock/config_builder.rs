use std::collections::HashMap;

use crate::adapters::{SceneInfo, SceneObject, ScriptLanguage};

use super::{MockConfig, MockConfigBuilder};

impl MockConfigBuilder {
    /// Set the DCC type.
    #[must_use]
    pub fn dcc_type(mut self, dcc_type: impl Into<String>) -> Self {
        self.config.dcc_type = dcc_type.into();
        self
    }

    /// Set the version string.
    #[must_use]
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Set the Python version.
    #[must_use]
    pub fn python_version(mut self, version: impl Into<String>) -> Self {
        self.config.python_version = Some(version.into());
        self
    }

    /// Set no Python version (e.g. for Unity mock).
    #[must_use]
    pub fn no_python(mut self) -> Self {
        self.config.python_version = None;
        self
    }

    /// Set the platform.
    #[must_use]
    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.config.platform = platform.into();
        self
    }

    /// Set the process ID.
    #[must_use]
    pub fn pid(mut self, pid: u32) -> Self {
        self.config.pid = pid;
        self
    }

    /// Add metadata entry.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.metadata.insert(key.into(), value.into());
        self
    }

    /// Set supported script languages.
    #[must_use]
    pub fn supported_languages(mut self, languages: Vec<ScriptLanguage>) -> Self {
        self.config.supported_languages = languages;
        self
    }

    /// Set initial scene info.
    #[must_use]
    pub fn scene(mut self, scene: SceneInfo) -> Self {
        self.config.scene = scene;
        self
    }

    /// Enable or disable snapshot support.
    #[must_use]
    pub fn snapshot_enabled(mut self, enabled: bool) -> Self {
        self.config.snapshot_enabled = enabled;
        self
    }

    /// Set a custom script execution handler.
    #[must_use]
    pub fn script_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str, ScriptLanguage, Option<u64>) -> Result<String, String> + Send + Sync + 'static,
    {
        self.config.script_handler = Some(Box::new(handler));
        self
    }

    /// Set the simulated health check latency.
    #[must_use]
    pub fn health_check_latency_ms(mut self, ms: u64) -> Self {
        self.config.health_check_latency_ms = ms;
        self
    }

    /// Make connect() fail with the given message.
    #[must_use]
    pub fn connect_should_fail(mut self, message: impl Into<String>) -> Self {
        self.config.connect_should_fail = true;
        self.config.connect_error_message = message.into();
        self
    }

    /// Set initial scene objects (for DccSceneManager).
    #[must_use]
    pub fn objects(mut self, objects: Vec<SceneObject>) -> Self {
        self.config.objects = objects;
        self
    }

    /// Set initial render settings.
    #[must_use]
    pub fn render_settings(mut self, settings: HashMap<String, String>) -> Self {
        self.config.render_settings = settings;
        self
    }

    /// Set simulated render time in milliseconds.
    #[must_use]
    pub fn render_time_ms(mut self, ms: u64) -> Self {
        self.config.render_time_ms = ms;
        self
    }

    /// Build the configuration.
    #[must_use]
    pub fn build(self) -> MockConfig {
        self.config
    }
}
