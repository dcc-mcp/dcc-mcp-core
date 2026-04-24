//! Unit tests for [`SkillWatcher`](super::SkillWatcher) and helpers.

use super::filter::{is_skill_related, should_reload};
use super::*;
use dcc_mcp_utils::constants::SKILL_METADATA_FILE;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;

// Helpers

fn write_skill(dir: &Path, name: &str) {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    let content = format!("---\nname: {name}\ndcc: python\n---\n# {name}\n\nTest skill.");
    fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
}

mod test_new {
    use super::*;

    #[test]
    fn create_with_default_debounce() {
        let watcher = SkillWatcher::new(Duration::from_millis(300));
        assert!(watcher.is_ok());
    }

    #[test]
    fn create_with_zero_debounce() {
        let watcher = SkillWatcher::new(Duration::ZERO);
        assert!(watcher.is_ok());
    }
}

mod test_watch {
    use super::*;

    #[test]
    fn watch_nonexistent_dir_returns_error() {
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        let result = watcher.watch("/path/that/does/not/exist/xyz");
        assert!(result.is_err());
    }

    #[test]
    fn watch_valid_dir_succeeds() {
        let tmp = tempdir().unwrap();
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        let result = watcher.watch(tmp.path());
        assert!(result.is_ok());
    }

    #[test]
    fn watch_and_immediate_skill_load() {
        let tmp = tempdir().unwrap();
        write_skill(tmp.path(), "alpha");
        write_skill(tmp.path(), "beta");

        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        watcher.watch(tmp.path()).unwrap();

        let skills = watcher.skills();
        assert_eq!(
            skills.len(),
            2,
            "Should have loaded 2 skills, got {skills:?}"
        );
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn watched_paths_contains_added_path() {
        let tmp = tempdir().unwrap();
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        watcher.watch(tmp.path()).unwrap();

        let paths = watcher.watched_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], tmp.path());
    }
}

mod test_unwatch {
    use super::*;

    #[test]
    fn unwatch_removes_path() {
        let tmp = tempdir().unwrap();
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        watcher.watch(tmp.path()).unwrap();
        assert_eq!(watcher.watched_paths().len(), 1);

        let removed = watcher.unwatch(tmp.path());
        assert!(removed, "unwatch should return true for known path");
        assert_eq!(watcher.watched_paths().len(), 0);
    }

    #[test]
    fn unwatch_unknown_path_returns_false() {
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        let removed = watcher.unwatch("/no/such/path");
        assert!(!removed);
    }
}

mod test_reload {
    use super::*;

    #[test]
    fn manual_reload_updates_skill_count() {
        let tmp = tempdir().unwrap();
        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        watcher.watch(tmp.path()).unwrap();
        assert_eq!(watcher.skill_count(), 0);

        // Add a skill after initial watch
        write_skill(tmp.path(), "new-skill");

        // Trigger manual reload
        watcher.reload();
        assert_eq!(watcher.skill_count(), 1);
    }

    #[test]
    fn reload_reflects_removed_skill() {
        let tmp = tempdir().unwrap();
        write_skill(tmp.path(), "removable");

        let mut watcher = SkillWatcher::new(Duration::from_millis(100)).unwrap();
        watcher.watch(tmp.path()).unwrap();
        assert_eq!(watcher.skill_count(), 1);

        // Remove the skill directory
        fs::remove_dir_all(tmp.path().join("removable")).unwrap();
        watcher.reload();
        assert_eq!(watcher.skill_count(), 0);
    }
}

mod test_skill_related {
    use super::*;

    #[test]
    fn skill_md_is_related() {
        assert!(is_skill_related(Path::new("/skills/my-skill/SKILL.md")));
    }

    #[test]
    fn depends_md_is_related() {
        assert!(is_skill_related(Path::new(
            "/skills/my-skill/metadata/depends.md"
        )));
    }

    #[test]
    fn python_script_is_related() {
        assert!(is_skill_related(Path::new(
            "/skills/my-skill/scripts/run.py"
        )));
    }

    #[test]
    fn mel_script_is_related() {
        assert!(is_skill_related(Path::new(
            "/skills/my-skill/scripts/rig.mel"
        )));
    }

    #[test]
    fn text_file_is_not_related() {
        assert!(!is_skill_related(Path::new("/skills/notes.txt")));
    }

    #[test]
    fn json_config_is_not_related() {
        assert!(!is_skill_related(Path::new("/skills/config.json")));
    }
}

mod test_should_reload {
    use super::*;
    use notify::EventKind;
    use notify::event::{CreateKind, ModifyKind, RemoveKind};

    fn make_event(kind: EventKind, path: &str) -> notify::Event {
        notify::Event {
            kind,
            paths: vec![PathBuf::from(path)],
            attrs: Default::default(),
        }
    }

    #[test]
    fn create_skill_md_triggers_reload() {
        let event = make_event(
            EventKind::Create(CreateKind::File),
            "/skills/new-skill/SKILL.md",
        );
        assert!(should_reload(&event));
    }

    #[test]
    fn modify_python_script_triggers_reload() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            "/skills/my-skill/scripts/run.py",
        );
        assert!(should_reload(&event));
    }

    #[test]
    fn remove_skill_md_triggers_reload() {
        let event = make_event(
            EventKind::Remove(RemoveKind::File),
            "/skills/old-skill/SKILL.md",
        );
        assert!(should_reload(&event));
    }

    #[test]
    fn access_event_does_not_trigger_reload() {
        let event = make_event(
            EventKind::Access(notify::event::AccessKind::Read),
            "/skills/my-skill/SKILL.md",
        );
        assert!(!should_reload(&event));
    }

    #[test]
    fn modify_non_skill_file_does_not_trigger_reload() {
        let event = make_event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            "/skills/my-skill/README.md",
        );
        // "readme.md" is not SKILL.md / depends.md, and .md is not a
        // supported script extension — should not reload.
        assert!(!should_reload(&event));
    }
}

mod test_debug {
    use super::*;

    #[test]
    fn debug_format_shows_counts() {
        let watcher = SkillWatcher::new(Duration::from_millis(200)).unwrap();
        let debug = format!("{watcher:?}");
        assert!(debug.contains("SkillWatcher"));
        assert!(debug.contains("debounce_ms"));
    }
}
