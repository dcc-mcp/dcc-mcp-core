//! Bedrock naming primitives: UUID truncation and cursor-safe alphabet.
//!
//! These are the smallest building blocks of the gateway naming contract.
//! They depend only on `uuid` and describe encoding *invariants* — no
//! transport, no logging, no shared state. Every other module in
//! [`crate::naming`] builds on top of them.

use uuid::Uuid;

/// Length of the truncated instance UUID prefix used in encoded tool
/// names (e.g. `i_abcdef01__create_sphere`).
///
/// 8 hex chars give 32 bits of entropy — enough to disambiguate among
/// the dozens of instances a gateway will ever see live, while staying
/// short enough to remain readable in log lines and error messages.
pub const ID_PREFIX_LEN: usize = 8;

/// Truncate a UUID to its first [`ID_PREFIX_LEN`] hex chars — the
/// canonical short form used inside encoded gateway tool names.
///
/// Always returns lowercase hex, so two truncations of the same UUID
/// are byte-for-byte equal regardless of the input casing.
#[must_use]
pub fn instance_short(id: &Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(ID_PREFIX_LEN);
    s
}

/// Return `true` iff every byte of `s` is in the cursor-safe alphabet
/// `[A-Za-z0-9_]` *and* `s` is non-empty.
///
/// This is the stricter regex some MCP clients (notably Cursor) enforce
/// on tool names. The [`crate::naming::encode`] module guarantees that
/// every name it emits passes this predicate; debug builds assert on
/// any violation.
#[must_use]
pub fn is_cursor_safe_alphabet(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
