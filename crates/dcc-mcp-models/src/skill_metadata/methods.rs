use super::*;

impl SkillMetadata {
    /// Access the `metadata.openclaw` section if present (ClawHub format).
    ///
    /// Returns `None` if this skill doesn't have ClawHub metadata.
    pub fn openclaw_metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_object().and_then(|metadata| {
            metadata
                .get("openclaw")
                .or_else(|| metadata.get("clawdbot"))
                .or_else(|| metadata.get("clawdis"))
        })
    }

    /// Union of DCC capabilities required by any tool in this skill (issue #354).
    pub fn required_capabilities(&self) -> Vec<String> {
        let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for tool in &self.tools {
            for capability in &tool.required_capabilities {
                if !capability.is_empty() {
                    set.insert(capability.clone());
                }
            }
        }
        set.into_iter().collect()
    }

    /// Get required environment variables declared by this skill (ClawHub).
    pub fn required_env_vars(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|requires| requires.get("env"))
            .and_then(|value| value.as_array())
            .map(|arr| arr.iter().filter_map(|value| value.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get required binaries declared by this skill (ClawHub).
    pub fn required_bins(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("requires"))
            .and_then(|requires| requires.get("bins"))
            .and_then(|value| value.as_array())
            .map(|arr| arr.iter().filter_map(|value| value.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get the primary credential environment variable (ClawHub `primaryEnv`).
    pub fn primary_env(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("primaryEnv"))
            .and_then(|value| value.as_str())
    }

    /// Get the emoji display for this skill (ClawHub).
    pub fn emoji(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("emoji"))
            .and_then(|value| value.as_str())
    }

    /// Get the homepage URL for this skill (ClawHub).
    pub fn homepage(&self) -> Option<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("homepage"))
            .and_then(|value| value.as_str())
    }

    /// Whether this skill is always active (no explicit load needed) (ClawHub `always`).
    pub fn always_active(&self) -> bool {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("always"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    /// Get OS restrictions for this skill (ClawHub `os`).
    pub fn os_restrictions(&self) -> Vec<&str> {
        self.openclaw_metadata()
            .and_then(|oc| oc.get("os"))
            .and_then(|value| value.as_array())
            .map(|arr| arr.iter().filter_map(|value| value.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get flat metadata key-value pairs (agentskills.io style).
    pub fn flat_metadata(&self) -> HashMap<&str, &str> {
        self.metadata
            .as_object()
            .map(|metadata| {
                metadata
                    .iter()
                    .filter_map(|(key, value)| value.as_str().map(|s| (key.as_str(), s)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns `true` iff no legacy top-level extension fields were used.
    pub fn is_spec_compliant(&self) -> bool {
        self.legacy_extension_fields.is_empty()
    }

    /// Returns true if this skill has any validation warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.name.len() > 64 {
            warnings.push(format!(
                "name '{}' exceeds 64 chars (agentskills.io limit)",
                self.name
            ));
        }
        if self.name.starts_with('-') || self.name.ends_with('-') {
            warnings.push(format!(
                "name '{}' must not start or end with a hyphen",
                self.name
            ));
        }
        if self.name.contains("--") {
            warnings.push(format!(
                "name '{}' must not contain consecutive hyphens",
                self.name
            ));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            warnings.push(format!(
                "name '{}' should be lowercase letters, digits, and hyphens only",
                self.name
            ));
        }

        if self.description.len() > 1024 {
            warnings.push(format!(
                "description length {} exceeds 1024 chars (agentskills.io limit)",
                self.description.len()
            ));
        }

        if self.compatibility.len() > 500 {
            warnings.push(format!(
                "compatibility length {} exceeds 500 chars (agentskills.io limit)",
                self.compatibility.len()
            ));
        }

        warnings
    }
}

impl std::fmt::Display for SkillMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{} ({})", self.name, self.version, self.dcc)
    }
}
