//! Minimal `{{name}}` substitution engine for [`crate::prompts`].

use std::collections::HashMap;

use super::spec::{PromptError, PromptResult};

/// Render a `{{name}}` template against an argument map.
///
/// Substitution rules:
///
/// - `{{name}}` → value from `args[name]`; missing → [`PromptError::MissingArg`].
/// - Whitespace inside the placeholder is tolerated: `{{ name }}`.
/// - Literal `{{` or `}}` outside a placeholder are preserved verbatim.
/// - The engine does NOT support `{{{raw}}}`, filters, blocks, or
///   conditionals (Handlebars-style). Keep templates simple.
///
/// # Errors
///
/// Returns the first missing-argument name encountered.
pub fn render_template(template: &str, args: &HashMap<String, String>) -> PromptResult<String> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `{{`
        if i + 1 < bytes.len()
            && bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && let Some(end_rel) = template[i + 2..].find("}}")
        {
            let raw = &template[i + 2..i + 2 + end_rel];
            let name = raw.trim();
            if !name.is_empty() && is_valid_placeholder(name) {
                match args.get(name) {
                    Some(v) => out.push_str(v),
                    None => return Err(PromptError::MissingArg(name.to_string())),
                }
                i = i + 2 + end_rel + 2;
                continue;
            }
            // Not a valid placeholder — emit `{{` literally and advance by 1.
            out.push_str("{{");
            i += 2;
            continue;
        }
        // Regular char — UTF-8 safe advance.
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    Ok(out)
}

fn is_valid_placeholder(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn next_char_boundary(s: &str, start: usize) -> usize {
    // Walk forward to the next UTF-8 char boundary.
    let mut j = start + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}
