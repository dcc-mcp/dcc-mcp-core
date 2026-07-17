//! Isolated Windows `PrintWindow` worker.
//!
//! This binary is shipped next to the Python extension and standalone server.
//! The parent process owns its lifetime and terminates it when a capture
//! exceeds the caller's deadline.

fn main() {
    std::process::exit(dcc_mcp_capture::helper::run_dedicated_from_env());
}
