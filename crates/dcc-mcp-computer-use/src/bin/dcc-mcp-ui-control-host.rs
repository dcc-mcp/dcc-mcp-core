//! Isolated current-user Windows host for DCC UI Control policy and state.

fn main() {
    std::process::exit(dcc_mcp_computer_use::ui_control_host::run_from_env());
}
