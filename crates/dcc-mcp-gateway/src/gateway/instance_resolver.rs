//! Session-scoped default instance resolution (RFC-0007 §3.1).
//!
//! Two live Blender instances used to make every call that omitted
//! `instance_id` fail with a list of bare UUIDs the caller had to guess
//! between. Multi-instance is a legal state, so the fix is not to refuse it —
//! it is to resolve deterministically and **always say which process won**.
//!
//! Resolution order (§3.1):
//!
//! 1. **Bound** — this session called `bind_instance` for the `dcc_type`.
//! 2. **Sticky** — this session already resolved and used an instance for this
//!    `dcc_type`; keep using it.
//! 3. **Single** — exactly one live candidate.
//! 4. **Newest** — largest `registered_at`, ties broken by `instance_id` so the
//!    choice is deterministic across gateway restarts.
//! 5. Otherwise error — but with human labels, not bare UUIDs.
//!
//! Three properties make the automatic choice safe rather than surprising:
//!
//! * **Scoped** — bindings are keyed by `(session_key, dcc_type)`. A missing
//!   session key means *not sticky*; the resolver never invents a shared
//!   default key, because that would let two unrelated clients pin each other.
//! * **Validated** — a binding is checked against the live candidate set on
//!   every use. A dead binding is dropped and re-resolved, and the response
//!   reports the switch.
//! * **Visible** — every automatic resolution is echoed back as
//!   `resolved_instance` so the caller can see (and correct) what happened.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use dcc_mcp_transport::discovery::types::ServiceEntry;

/// Session key for REST callers.
///
/// MCP clients already carry `Mcp-Session-Id`; REST clients opt in with this
/// header. Both are read at the HTTP boundary and passed down explicitly — the
/// resolver never synthesises a key.
pub const REST_SESSION_HEADER: &str = "x-dcc-session-id";

/// Session key for MCP callers.
pub const MCP_SESSION_HEADER: &str = "Mcp-Session-Id";

/// Upper bound on tracked `(session, dcc_type)` bindings.
///
/// Sessions that go away without unbinding would otherwise leak a row forever.
/// When the map exceeds this the least-recently-used binding is dropped.
const MAX_BINDINGS: usize = 4096;

/// A binding untouched for this long is treated as gone.
const BINDING_TTL: Duration = Duration::from_secs(30 * 60);

/// How the gateway picked an instance when the caller did not name one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveVia {
    /// The caller named the instance (UUID, prefix, or alias).
    Explicit,
    /// This session called `bind_instance` for the `dcc_type`.
    Bound,
    /// This session already resolved this `dcc_type` once; reuse that row.
    Sticky,
    /// Exactly one live candidate existed.
    Single,
    /// Several candidates existed; the most recently registered one won.
    Newest,
}

impl ResolveVia {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Bound => "bound",
            Self::Sticky => "sticky",
            Self::Single => "single",
            Self::Newest => "newest",
        }
    }
}

/// An instance plus the provenance of *how* it was chosen.
#[derive(Debug, Clone)]
pub struct ResolvedInstance {
    pub entry: ServiceEntry,
    pub via: ResolveVia,
    /// Set when a previous binding was discarded because its instance died.
    ///
    /// A silent fallback to a different process would be worse than the
    /// original error, so the switch is always surfaced to the caller.
    pub previous_instance_id: Option<Uuid>,
}

impl ResolvedInstance {
    /// Wire form echoed on every auto-resolved call (§3.1 rule 4).
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "instance_id": self.entry.instance_id.to_string(),
            "instance_short": entry_to_short(&self.entry.instance_id),
            "dcc_type": self.entry.dcc_type,
            "display_name": human_instance_label(&self.entry),
            "via": self.via.as_str(),
            "switched": self.previous_instance_id.is_some(),
        });
        if let Some(pid) = self.entry.pid {
            out["pid"] = json!(pid);
        }
        if let Some(previous) = self.previous_instance_id {
            out["previous_instance_id"] = json!(previous.to_string());
            out["notice"] = json!(format!(
                "instance {} is no longer live; re-resolved to {}",
                entry_to_short(&previous),
                entry_to_short(&self.entry.instance_id),
            ));
        }
        out
    }
}

/// Sort key for "most recently registered wins", with a deterministic tiebreak.
fn newest_first(a: &ServiceEntry, b: &ServiceEntry) -> std::cmp::Ordering {
    b.registered_at
        .cmp(&a.registered_at)
        .then_with(|| a.instance_id.cmp(&b.instance_id))
}

/// Pick the newest registration from a non-empty candidate slice.
///
/// Callers guarantee `candidates` is non-empty.
fn pick_newest(candidates: &mut Vec<ServiceEntry>) -> ServiceEntry {
    candidates.sort_by(newest_first);
    candidates
        .drain(..)
        .next()
        .expect("pick_newest requires at least one candidate")
}

/// Human label for one instance, degrading gracefully (§3.1 rule 6).
///
/// 1. `display_name` — `"Blender 5.1.1 — shot_light.blend"`
/// 2. `basename(scene)` — `"shot_light.blend"`
/// 3. `"pid 543720 (started 2m ago)"`
///
/// Today most registry rows carry no `display_name`, so the lower rungs are the
/// common case rather than an edge case.
#[must_use]
pub fn human_instance_label(entry: &ServiceEntry) -> String {
    if let Some(name) = entry
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }
    if let Some(base) = entry
        .scene
        .as_deref()
        .map(str::trim)
        .filter(|scene| !scene.is_empty())
        .map(scene_basename)
        .filter(|base| !base.is_empty())
    {
        return base;
    }
    match entry.pid {
        Some(pid) => format!("pid {pid} (started {})", age_label(entry.registered_at)),
        None => format!("{} instance", entry.dcc_type),
    }
}

/// Operator-facing candidate string used inside `MultipleMatches`.
///
/// Keeps the short id — the caller still needs something selectable — but leads
/// with a label a human can act on.
#[must_use]
pub fn instance_candidate(entry: &ServiceEntry) -> String {
    format!(
        "{}:{} ({})",
        entry.dcc_type,
        entry_to_short(&entry.instance_id),
        human_instance_label(entry),
    )
}

fn entry_to_short(instance_id: &Uuid) -> String {
    let simple = instance_id.simple().to_string();
    simple[..8].to_string()
}

fn scene_basename(scene: &str) -> String {
    let normalized = scene.replace('\\', "/");
    Path::new(&normalized)
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .unwrap_or_default()
}

fn age_label(registered_at: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(registered_at) else {
        return "just now".to_string();
    };
    let secs = elapsed.as_secs();
    if secs < 60 {
        return format!("{secs}s ago");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

#[derive(Debug, Clone)]
struct Binding {
    instance_id: Uuid,
    /// Set only by `bind_instance`; outranks sticky on later calls.
    bound: bool,
    last_used: Instant,
}

#[derive(Debug, Default)]
struct StickyInner {
    bindings: HashMap<(String, String), Binding>,
    /// `(session, alias) -> (dcc_type, instance_id)`.
    aliases: HashMap<(String, String), (String, Uuid)>,
    last_pruned: Option<Instant>,
}

impl StickyInner {
    fn prune_locked(&mut self, now: Instant) {
        let due = self
            .last_pruned
            .is_none_or(|last| now.duration_since(last) > Duration::from_secs(60));
        if !due {
            return;
        }
        self.last_pruned = Some(now);
        self.bindings
            .retain(|_, binding| now.duration_since(binding.last_used) <= BINDING_TTL);
        // An alias is only meaningful while the binding it points at survives.
        let live: std::collections::HashSet<(String, String, Uuid)> = self
            .bindings
            .iter()
            .map(|((session, dcc), binding)| (session.clone(), dcc.clone(), binding.instance_id))
            .collect();
        self.aliases
            .retain(|(session, _), (dcc, id)| live.contains(&(session.clone(), dcc.clone(), *id)));
        if self.bindings.len() > MAX_BINDINGS {
            let mut keys: Vec<((String, String), Instant)> = self
                .bindings
                .iter()
                .map(|(key, binding)| (key.clone(), binding.last_used))
                .collect();
            keys.sort_by_key(|(_, last_used)| *last_used);
            let excess = self.bindings.len() - MAX_BINDINGS;
            for (key, _) in keys.into_iter().take(excess) {
                self.bindings.remove(&key);
            }
        }
    }
}

/// Session-scoped sticky/bound instance store.
///
/// `RwLock` rather than `tokio::sync::RwLock`: the critical sections are a
/// hash lookup and an insert, so no guard is ever held across an `await`.
#[derive(Debug, Default)]
pub struct InstanceResolver {
    inner: RwLock<StickyInner>,
}

impl InstanceResolver {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the binding for `(session, dcc_type)`, if it is still fresh.
    fn lookup(&self, session: &str, dcc_type: &str) -> Option<Binding> {
        let now = Instant::now();
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.prune_locked(now);
        let binding = inner
            .bindings
            .get_mut(&(session.to_string(), dcc_type.to_ascii_lowercase()))?;
        binding.last_used = now;
        Some(binding.clone())
    }

    /// Record that `instance_id` was used for `(session, dcc_type)`.
    pub fn record(&self, session: &str, dcc_type: &str, instance_id: Uuid) {
        let now = Instant::now();
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.prune_locked(now);
        let key = (session.to_string(), dcc_type.to_ascii_lowercase());
        match inner.bindings.get_mut(&key) {
            Some(existing) => {
                existing.instance_id = instance_id;
                existing.last_used = now;
            }
            None => {
                inner.bindings.insert(
                    key,
                    Binding {
                        instance_id,
                        bound: false,
                        last_used: now,
                    },
                );
            }
        }
    }

    /// Pin `(session, dcc_type)` to `instance_id` until it dies or is unbound.
    ///
    /// `alias` lets the caller address the row by a friendly name later.
    pub fn bind(
        &self,
        session: &str,
        dcc_type: &str,
        instance_id: Uuid,
        alias: Option<&str>,
    ) -> Option<String> {
        let now = Instant::now();
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.prune_locked(now);
        let dcc_key = dcc_type.to_ascii_lowercase();
        inner.bindings.insert(
            (session.to_string(), dcc_key.clone()),
            Binding {
                instance_id,
                bound: true,
                last_used: now,
            },
        );
        let alias = alias
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .map(str::to_ascii_lowercase);
        if let Some(alias) = alias.clone() {
            inner
                .aliases
                .insert((session.to_string(), alias), (dcc_key, instance_id));
        }
        alias
    }

    /// Drop the pin for `(session, dcc_type)` (or every DCC when `None`).
    ///
    /// Returns the number of bindings removed. Sticky memory is cleared too, so
    /// the next call re-resolves from scratch instead of restoring the pin.
    pub fn unbind(&self, session: &str, dcc_type: Option<&str>) -> usize {
        let now = Instant::now();
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.prune_locked(now);
        let filter = dcc_type.map(str::to_ascii_lowercase);
        let before = inner.bindings.len();
        inner.bindings.retain(|(owner, dcc), _| {
            owner != session || filter.as_deref().is_some_and(|f| f != dcc)
        });
        let removed = before - inner.bindings.len();
        if removed > 0 {
            let live: std::collections::HashSet<(String, String, Uuid)> = inner
                .bindings
                .iter()
                .map(|((owner, dcc), binding)| (owner.clone(), dcc.clone(), binding.instance_id))
                .collect();
            inner
                .aliases
                .retain(|(owner, _), (dcc, id)| live.contains(&(owner.clone(), dcc.clone(), *id)));
        }
        removed
    }

    /// Resolve an alias to `(dcc_type, instance_id)` for this session.
    pub fn resolve_alias(&self, session: &str, alias: &str) -> Option<(String, Uuid)> {
        let alias = alias.trim().to_ascii_lowercase();
        if alias.is_empty() {
            return None;
        }
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.aliases.get(&(session.to_string(), alias)).cloned()
    }

    /// Snapshot of every binding for one session (diagnostics / tests).
    #[must_use]
    pub fn session_snapshot(&self, session: &str) -> Vec<Value> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<Value> = inner
            .bindings
            .iter()
            .filter(|((owner, _), _)| owner == session)
            .map(|((_, dcc), binding)| {
                json!({
                    "dcc_type": dcc,
                    "instance_id": binding.instance_id.to_string(),
                    "bound": binding.bound,
                })
            })
            .collect();
        rows.sort_by(|a, b| {
            a["dcc_type"]
                .as_str()
                .unwrap_or_default()
                .cmp(b["dcc_type"].as_str().unwrap_or_default())
        });
        rows
    }

    /// Run the full §3.1 resolution ladder.
    ///
    /// `candidates` must already be filtered by `dcc_type` and liveness. The
    /// session key is `None` when the caller supplied no session identity — in
    /// that case rules 1 and 2 are skipped rather than falling back to a shared
    /// default, so unrelated clients can never pin each other.
    pub fn resolve(
        &self,
        candidates: Vec<ServiceEntry>,
        dcc_filter: Option<&str>,
        session: Option<&str>,
    ) -> Result<ResolvedInstance, Vec<ServiceEntry>> {
        let session = session.map(str::trim).filter(|key| !key.is_empty());
        let mut candidates = candidates;

        if let Some(session) = session {
            let dcc_key = dcc_filter.unwrap_or_default().to_ascii_lowercase();
            // Rules 1 and 2: bound, then sticky. Both are validated against the
            // live candidate set — a dead target is dropped, not followed.
            let (bound_target, sticky_target) = {
                let binding = self.lookup(session, &dcc_key);
                match binding {
                    Some(binding) => (
                        binding.bound.then_some(binding.instance_id),
                        Some(binding.instance_id),
                    ),
                    None => (None, None),
                }
            };
            for (via_slot, target) in [
                (Some(ResolveVia::Bound), bound_target),
                (Some(ResolveVia::Sticky), sticky_target),
            ] {
                let Some(target) = target else { continue };
                let Some(via) = via_slot else { continue };
                if let Some(position) = candidates.iter().position(|e| e.instance_id == target) {
                    let entry = candidates.remove(position);
                    return Ok(ResolvedInstance {
                        entry,
                        via,
                        previous_instance_id: None,
                    });
                }
                // The pinned row is gone. Re-resolve below and report the
                // switch instead of silently using something else.
                let previous = target;
                return match candidates.as_slice() {
                    [] => Err(candidates),
                    [_] => {
                        let entry = candidates.remove(0);
                        Ok(ResolvedInstance {
                            entry,
                            via: ResolveVia::Single,
                            previous_instance_id: Some(previous),
                        })
                    }
                    _ => {
                        let entry = pick_newest(&mut candidates);
                        Ok(ResolvedInstance {
                            entry,
                            via: ResolveVia::Newest,
                            previous_instance_id: Some(previous),
                        })
                    }
                };
            }
        }

        match candidates.as_slice() {
            [] => Err(candidates),
            [_] => {
                let entry = candidates.remove(0);
                Ok(ResolvedInstance {
                    entry,
                    via: ResolveVia::Single,
                    previous_instance_id: None,
                })
            }
            _ => {
                let entry = pick_newest(&mut candidates);
                Ok(ResolvedInstance {
                    entry,
                    via: ResolveVia::Newest,
                    previous_instance_id: None,
                })
            }
        }
    }
}

/// Inject `resolved_instance` into a backend text payload.
///
/// Backends return either a JSON object or free text. Only the JSON case is
/// annotated — rewriting prose would corrupt the message. When the payload is
/// not a JSON object the caller still reports the resolution out of band.
#[must_use]
pub fn annotate_resolved_instance(text: &str, resolved: &Value) -> String {
    let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(text) else {
        return text.to_string();
    };
    if obj.contains_key("resolved_instance") {
        return text.to_string();
    }
    obj.insert("resolved_instance".to_string(), resolved.clone());
    let pretty = text.contains('\n');
    if pretty {
        serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_else(|_| text.to_string())
    } else {
        serde_json::to_string(&Value::Object(obj)).unwrap_or_else(|_| text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn entry(dcc: &str, id: &str, display: Option<&str>, secs_ago: u64) -> ServiceEntry {
        let mut e = ServiceEntry::new(dcc.to_string(), "127.0.0.1", 9000);
        e.instance_id = Uuid::parse_str(id).expect("valid uuid");
        e.display_name = display.map(str::to_string);
        e.registered_at = SystemTime::now() - Duration::from_secs(secs_ago);
        e
    }

    #[test]
    fn no_session_key_means_not_sticky() {
        let resolver = InstanceResolver::new();
        let a = entry("blender", "0202de99-0000-0000-0000-000000000001", None, 10);
        let b = entry("blender", "51e71dbf-0000-0000-0000-000000000002", None, 5);

        let first = resolver
            .resolve(vec![a.clone(), b.clone()], Some("blender"), None)
            .expect("resolves");
        let second = resolver
            .resolve(vec![a.clone(), b.clone()], Some("blender"), None)
            .expect("resolves");

        // Without a session key nothing is remembered, but the deterministic
        // newest-wins rule keeps both calls on the same instance anyway.
        assert_eq!(first.entry.instance_id, second.entry.instance_id);
        assert_eq!(first.via, ResolveVia::Newest);
        assert_eq!(first.entry.instance_id, b.instance_id);
    }

    #[test]
    fn sticky_binding_is_scoped_to_session_and_dcc() {
        let resolver = InstanceResolver::new();
        let older = entry("blender", "0202de99-0000-0000-0000-000000000001", None, 60);
        let newer = entry("blender", "51e71dbf-0000-0000-0000-000000000002", None, 5);
        let maya = entry("maya", "aaaaaaaa-0000-0000-0000-000000000003", None, 1);

        // Session A binds the *older* Blender explicitly; sticky must honour it
        // even though newest-wins would pick the other one.
        resolver.bind("sess-a", "blender", older.instance_id, None);
        let resolved = resolver
            .resolve(
                vec![older.clone(), newer.clone()],
                Some("blender"),
                Some("sess-a"),
            )
            .expect("resolves");
        assert_eq!(resolved.entry.instance_id, older.instance_id);
        assert_eq!(resolved.via, ResolveVia::Bound);

        // Session B has no binding for blender: it must not inherit session A.
        let other = resolver
            .resolve(
                vec![older.clone(), newer.clone()],
                Some("blender"),
                Some("sess-b"),
            )
            .expect("resolves");
        assert_eq!(other.entry.instance_id, newer.instance_id);
        assert_eq!(other.via, ResolveVia::Newest);

        // Blender binding does not leak into the maya scope.
        let maya_resolved = resolver
            .resolve(vec![maya.clone()], Some("maya"), Some("sess-a"))
            .expect("resolves");
        assert_eq!(maya_resolved.via, ResolveVia::Single);
    }

    #[test]
    fn dead_binding_re_resolves_and_reports_the_switch() {
        let resolver = InstanceResolver::new();
        let doomed = entry("blender", "0202de99-0000-0000-0000-000000000001", None, 60);
        let survivor = entry("blender", "51e71dbf-0000-0000-0000-000000000002", None, 5);
        resolver.bind("sess-a", "blender", doomed.instance_id, None);

        // The bound instance is no longer among the live candidates.
        let resolved = resolver
            .resolve(vec![survivor.clone()], Some("blender"), Some("sess-a"))
            .expect("resolves");

        assert_eq!(resolved.entry.instance_id, survivor.instance_id);
        assert_eq!(resolved.previous_instance_id, Some(doomed.instance_id));
        let json = resolved.to_json();
        assert_eq!(json["switched"], json!(true));
        assert_eq!(
            json["previous_instance_id"],
            json!(doomed.instance_id.to_string())
        );
    }

    #[test]
    fn unbind_clears_the_pin_for_that_dcc_only() {
        let resolver = InstanceResolver::new();
        let id = Uuid::parse_str("0202de99-0000-0000-0000-000000000001").expect("valid uuid");
        resolver.bind("sess-a", "blender", id, Some("main"));
        resolver.bind("sess-a", "maya", id, None);

        assert_eq!(resolver.unbind("sess-a", Some("blender")), 1);
        assert!(resolver.resolve_alias("sess-a", "main").is_none());
        assert_eq!(resolver.session_snapshot("sess-a").len(), 1);
        assert_eq!(resolver.unbind("sess-a", None), 1);
        assert!(resolver.session_snapshot("sess-a").is_empty());
    }

    #[test]
    fn alias_resolves_across_dcc_scopes() {
        let resolver = InstanceResolver::new();
        let id = Uuid::parse_str("0202de99-0000-0000-0000-000000000001").expect("valid uuid");
        resolver.bind("sess-a", "blender", id, Some("Main"));
        assert_eq!(
            resolver.resolve_alias("sess-a", "main"),
            Some(("blender".to_string(), id))
        );
        assert!(resolver.resolve_alias("sess-b", "main").is_none());
    }

    #[test]
    fn human_label_degrades_display_name_scene_then_pid() {
        let mut named = entry("blender", "0202de99-0000-0000-0000-000000000001", None, 30);
        named.display_name = Some("Blender 5.1.1 — shot_light.blend".to_string());
        assert_eq!(
            human_instance_label(&named),
            "Blender 5.1.1 — shot_light.blend"
        );

        let mut scened = entry("blender", "0202de99-0000-0000-0000-000000000002", None, 30);
        scened.scene = Some("C:/shots/lighting/shot_light.blend".to_string());
        assert_eq!(human_instance_label(&scened), "shot_light.blend");

        let mut pided = entry("blender", "0202de99-0000-0000-0000-000000000003", None, 120);
        pided.pid = Some(543_720);
        assert_eq!(human_instance_label(&pided), "pid 543720 (started 2m ago)");
    }

    #[test]
    fn candidate_string_leads_with_the_human_label() {
        let mut e = entry("blender", "0202de99-0000-0000-0000-000000000004", None, 30);
        e.pid = Some(543_720);
        assert_eq!(
            instance_candidate(&e),
            "blender:0202de99 (pid 543720 (started 30s ago))"
        );
    }

    #[test]
    fn annotate_injects_resolved_instance_into_json_payloads_only() {
        let resolved = json!({"instance_id": "x", "via": "newest"});
        let annotated = annotate_resolved_instance("{\"ok\":true}", &resolved);
        assert!(annotated.contains("\"resolved_instance\""));

        let untouched = annotate_resolved_instance("plain text payload", &resolved);
        assert_eq!(untouched, "plain text payload");

        let already =
            annotate_resolved_instance("{\"resolved_instance\":{\"via\":\"single\"}}", &resolved);
        assert!(already.contains("\"via\":\"single\""));
    }
}
