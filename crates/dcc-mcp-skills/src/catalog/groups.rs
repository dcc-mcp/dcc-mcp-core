use super::*;

impl SkillCatalog {
    fn skill_group_key(skill_name: &str, group_name: &str) -> String {
        format!("{skill_name}:{group_name}")
    }

    pub(super) fn has_skill_group(&self, skill_name: &str, group_name: &str) -> bool {
        self.entries.get(skill_name).is_some_and(|entry| {
            entry
                .metadata
                .groups
                .iter()
                .any(|item| item.name == group_name)
                || entry
                    .metadata
                    .tools
                    .iter()
                    .any(|tool| tool.group == group_name)
        })
    }

    /// Activate a tool group: enable every [`ToolMeta`] whose
    /// ``group`` field matches ``group_name``.
    ///
    /// Returns the number of actions whose ``enabled`` state changed.
    pub fn activate_group(&self, group_name: &str) -> usize {
        let inserted = self.active_groups.insert(group_name.to_string());
        let count = self.registry.set_group_enabled(group_name, true);
        if inserted {
            self.notify_after_group_change_hook(group_name, true);
            self.notify_after_scoped_group_change_hook(group_name, true);
        }
        count
    }

    /// Deactivate a tool group (inverse of [`activate_group`]).
    pub fn deactivate_group(&self, group_name: &str) -> usize {
        let mut removed_keys = Vec::new();
        if self.active_groups.remove(group_name).is_some() {
            removed_keys.push(group_name.to_string());
        }
        let scoped_suffix = format!(":{group_name}");
        let scoped_keys: Vec<_> = self
            .active_groups
            .iter()
            .filter(|key| key.ends_with(&scoped_suffix))
            .map(|key| key.clone())
            .collect();
        for key in scoped_keys {
            if self.active_groups.remove(&key).is_some() {
                removed_keys.push(key);
            }
        }
        let count = self.registry.set_group_enabled(group_name, false);
        if !removed_keys.is_empty() {
            self.notify_after_group_change_hook(group_name, false);
        }
        for key in removed_keys {
            self.notify_after_scoped_group_change_hook(&key, false);
        }
        count
    }

    /// Activate one group belonging to one skill.
    pub fn activate_skill_group(&self, skill_name: &str, group_name: &str) -> usize {
        let key = Self::skill_group_key(skill_name, group_name);
        let inserted = self.active_groups.insert(key.clone());
        let count = self
            .registry
            .set_skill_group_enabled(skill_name, group_name, true);
        if inserted {
            self.notify_after_group_change_hook(group_name, true);
            self.notify_after_scoped_group_change_hook(&key, true);
        }
        count
    }

    /// Deactivate one group belonging to one skill.
    pub fn deactivate_skill_group(&self, skill_name: &str, group_name: &str) -> usize {
        let legacy_active = self.active_groups.contains(group_name);
        if legacy_active {
            let other_owners: Vec<_> = self
                .loaded
                .iter()
                .map(|entry| entry.key().clone())
                .filter(|owner| owner != skill_name && self.has_skill_group(owner, group_name))
                .collect();
            for owner in other_owners {
                let key = Self::skill_group_key(&owner, group_name);
                if self.active_groups.insert(key.clone()) {
                    self.notify_after_scoped_group_change_hook(&key, true);
                }
            }
            if self.active_groups.remove(group_name).is_some() {
                self.notify_after_scoped_group_change_hook(group_name, false);
            }
        }
        let key = Self::skill_group_key(skill_name, group_name);
        let removed = self.active_groups.remove(&key).is_some();
        let count = self
            .registry
            .set_skill_group_enabled(skill_name, group_name, false);
        if removed {
            self.notify_after_scoped_group_change_hook(&key, false);
        }
        if legacy_active || removed {
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

    /// Fire the scoped persistence observer. Failures never roll back state.
    fn notify_after_scoped_group_change_hook(&self, key: &str, activated: bool) {
        let Some(hook) = self.after_scoped_group_change_hook.read().clone() else {
            return;
        };
        if let Err(reason) = hook(key, activated) {
            tracing::warn!(
                group = key,
                activated,
                error = %reason,
                "SkillCatalog after-scoped-group-change hook failed"
            );
        }
    }

    /// Return all currently-active tool group names.
    pub fn active_groups(&self) -> Vec<String> {
        let mut groups = std::collections::BTreeSet::new();
        for key in self.active_group_keys() {
            let group = key
                .rsplit_once(':')
                .filter(|(skill, group)| self.has_skill_group(skill, group))
                .map_or(key.as_str(), |(_, group)| group);
            groups.insert(group.to_string());
        }
        groups.into_iter().collect()
    }

    pub(crate) fn active_group_keys(&self) -> Vec<String> {
        self.active_groups.iter().map(|key| key.clone()).collect()
    }

    /// Return every distinct group name declared across loaded skills.
    pub fn list_groups(&self) -> Vec<(String, String, bool)> {
        let mut out: Vec<(String, String, bool)> = Vec::new();
        for entry in self.entries.iter() {
            let skill = entry.key().clone();
            for g in &entry.value().metadata.groups {
                let active = self.active_groups.contains(&g.name)
                    || self
                        .active_groups
                        .contains(&Self::skill_group_key(&skill, &g.name));
                out.push((skill.clone(), g.name.clone(), active));
            }
        }
        out
    }
}
