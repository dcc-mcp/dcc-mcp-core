//! Shared skill-catalog reload hook used by admin handlers that mutate skill paths.

use super::state::AdminState;
use crate::gateway::capability::RefreshReason;
use crate::gateway::capability_service::refresh_all_live_backends;

/// Run the optional disk-catalog reload callback, then refresh live backends.
pub async fn reload_skill_paths_and_refresh_backends(state: &AdminState, reason: RefreshReason) {
    if let Some(cb) = state.skill_paths_reload.clone() {
        cb();
    }
    refresh_all_live_backends(&state.gateway, reason).await;
}
