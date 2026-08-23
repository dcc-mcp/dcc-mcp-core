use super::EntryMap;

pub(super) fn membership_delta(current: &EntryMap, loaded: &EntryMap) -> (usize, usize) {
    let added = loaded
        .keys()
        .filter(|key| !current.contains_key(*key))
        .count();
    let removed = current
        .keys()
        .filter(|key| !loaded.contains_key(*key))
        .count();
    (added, removed)
}
