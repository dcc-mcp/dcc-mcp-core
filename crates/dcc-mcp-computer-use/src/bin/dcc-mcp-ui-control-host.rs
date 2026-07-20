//! Isolated current-user Windows host for DCC UI Control policy and state.

fn main() {
    // Re-enter the same executable as the short-lived, killable PrintWindow
    // worker before starting the long-lived UI Control host.
    if let Some(exit_code) = dcc_mcp_capture::capture_worker::run_embedded_if_requested() {
        std::process::exit(exit_code);
    }
    std::process::exit(dcc_mcp_computer_use::ui_control_host::run_from_env());
}
