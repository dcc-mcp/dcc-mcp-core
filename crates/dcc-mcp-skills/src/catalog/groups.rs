use super::*;

impl SkillCatalog {
    /// Activate a tool group: enable every [`ToolMeta`] whose
    /// ``group`` field matches ``group_name``.
    ///
    /// Returns the number of actions whose ``enabled`` state changed.
    pub fn activate_group(&self, group_name: &str) -> usize {
        let inserted = self.active_groups.insert(group_name.to_string());
        let count = self.registry.set_group_enabled(group_name, true);
        if inserted {
            self.notify_after_group_change_hook(group_name, true);
        }
        count
    }

    /// Deactivate a tool group (inverse of [`activate_group`]).
    pub fn deactivate_group(&self, group_name: &str) -> usize {
        let removed = self.active_groups.remove(group_name).is_some();
        let count = self.registry.set_group_enabled(group_name, false);
        if removed {
            self.notify_after_group_change_hook(group_name, false);
        }
        count
    }

    /// Fire the after-group-change observer (#1405). Failures are logged
    /// only — they never roll back the group state change.
    fn notify_after_group_change_hook(&self, group_name: &str, activated: bool) {
        let Some(hook) = self.after_group_change_hook.read().clone() else {
            return;
        };
        if let Err(reason) = hook(group_name, activated) {
            tracing::warn!(
                group = group_name,
                activated,
                error = %reason,
                "SkillCatalog after-group-change hook failed"
            );
        }
    }

    /// Return all currently-active tool group names.
    pub fn active_groups(&self) -> Vec<String> {
        self.active_groups.iter().map(|e| e.clone()).collect()
    }

    /// Return every distinct group name declared across loaded skills.
    pub fn list_groups(&self) -> Vec<(String, String, bool)> {
        let mut out: Vec<(String, String, bool)> = Vec::new();
        for entry in self.entries.iter() {
            let skill = entry.key().clone();
            for g in &entry.value().metadata.groups {
                let active = self.active_groups.contains(&g.name);
                out.push((skill.clone(), g.name.clone(), active));
            }
        }
        out
    }
}
