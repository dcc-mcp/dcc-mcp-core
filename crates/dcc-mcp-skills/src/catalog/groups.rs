use super::*;

impl SkillCatalog {
    /// Activate a tool group: enable every [`ActionMeta`] whose
    /// ``group`` field matches ``group_name``.
    ///
    /// Returns the number of actions whose ``enabled`` state changed.
    pub fn activate_group(&self, group_name: &str) -> usize {
        self.active_groups.insert(group_name.to_string());
        self.registry.set_group_enabled(group_name, true)
    }

    /// Deactivate a tool group (inverse of [`activate_group`]).
    pub fn deactivate_group(&self, group_name: &str) -> usize {
        self.active_groups.remove(group_name);
        self.registry.set_group_enabled(group_name, false)
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
