//! Version-bound contract for adapter dependency declarations on `dcc-mcp-core`.
//!
//! # Why this module exists
//!
//! `dcc-mcp-core` ships `0.MINOR.PATCH` releases, so **every minor bump may
//! break an adapter**. Adapters publish a bounded range on PyPI
//! (`dcc-mcp-core>=0.19.3,<0.19.5`), but the same adapter has historically been
//! requested from package environments as `dcc_mcp_core-0` — an "any 0.x"
//! request. Resolvers happily pick a core five minor lines newer than the
//! adapter ever claimed to support, the top-level imports still succeed, and
//! the failure only surfaces deep inside server startup.
//!
//! This module turns that observation into a checkable contract:
//!
//! 1. A declaration must carry a lower **and** an upper bound
//!    ([`CoreBoundPolicy::require_lower_bound`] /
//!    [`CoreBoundPolicy::require_upper_bound`]).
//! 2. The upper bound must not admit more than
//!    [`CoreBoundPolicy::max_minor_lines`] minor line(s) — one by default.
//! 3. When only a minimum core version is known,
//!    [`derive_requirement`] produces the canonical bounded range
//!    (`0.19.3` → `>=0.19.3,<0.20.0`).
//! 4. [`check_runtime`] compares a declaration against the core that is
//!    actually executing, so an out-of-range combination is reported where the
//!    operator can act on it instead of at an unrelated import site.
//! 5. [`compare_declarations`] catches the reported failure directly: a package
//!    environment request that admits versions the packaging metadata excludes.
//!
//! # Declaration forms
//!
//! Two forms are parsed by [`CoreRequirement::parse`]:
//!
//! * **Packaging requirements** — a PEP 440 specifier set, optionally
//!   prefixed with the distribution name and suffixed with a marker:
//!   `dcc-mcp-core>=0.19.3,<0.19.5`, `dcc_mcp_core~=0.20.14`,
//!   `>=0.19.3,<0.19.5; python_version >= "3.8"`.
//! * **Package environment requests** — `<name>-<range>` where the range is a
//!   version prefix (`dcc_mcp_core-0`, `dcc_mcp_core-0.20`) or a `..` pair
//!   (`dcc_mcp_core-0.19.3..0.20.0`, lower inclusive, upper exclusive). A bare
//!   name (`dcc_mcp_core`) is unbounded.

use std::fmt;

/// Distribution name adapters depend on.
pub const CORE_DISTRIBUTION: &str = "dcc-mcp-core";

/// Import name of [`CORE_DISTRIBUTION`].
pub const CORE_IMPORT_NAME: &str = "dcc_mcp_core";

// ── versions ──────────────────────────────────────────────────────────────────

/// A `MAJOR.MINOR.PATCH` core version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl CoreVersion {
    /// Build a version from its numeric components.
    #[must_use]
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse `0.19.3`, `0.20`, `0`, `v0.19.3`, or `0.19.3rc1`.
    ///
    /// Missing components default to `0`; pre-release and build suffixes are
    /// dropped. Returns `None` for anything that is not numeric.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim().trim_start_matches(['v', 'V']);
        let numeric = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
        if numeric.is_empty() {
            return None;
        }
        let mut parts = numeric.split('.');
        let major = Self::component(parts.next()?)?;
        let minor = match parts.next() {
            Some(part) => Self::component(part)?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(part) => Self::component(part)?,
            None => 0,
        };
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// First version of the next minor line: `0.19.3` → `0.20.0`.
    #[must_use]
    pub fn next_minor(self) -> Self {
        Self {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
        }
    }

    /// Exclusive upper bound that admits `lines` minor lines starting here.
    ///
    /// `0.19.3.minor_line_limit(1)` is `0.20.0`; with `2` it is `0.21.0`.
    #[must_use]
    pub fn minor_line_limit(self, lines: u32) -> Self {
        Self {
            major: self.major,
            minor: self.minor + lines.max(1),
            patch: 0,
        }
    }

    /// The `MAJOR.MINOR` line this version belongs to (e.g. `"0.19"`).
    #[must_use]
    pub fn minor_line(self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    fn component(text: &str) -> Option<u32> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    }
}

impl fmt::Display for CoreVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ── requirements ─────────────────────────────────────────────────────────────

/// Whether a bound includes the version it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundKind {
    Inclusive,
    Exclusive,
}

/// A parsed dependency range on [`CORE_DISTRIBUTION`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreRequirement {
    /// Lowest admitted version and whether it is admitted itself.
    pub lower: Option<(CoreVersion, BoundKind)>,
    /// Highest admitted version and whether it is admitted itself.
    pub upper: Option<(CoreVersion, BoundKind)>,
    /// Specifiers this parser could not interpret.
    ///
    /// Never empty when the declaration cannot be trusted: an unknown
    /// specifier is a range the checker refuses to guess.
    pub unsupported: Vec<String>,
}

impl CoreRequirement {
    /// Whether the declaration carried no readable bound at all.
    #[must_use]
    pub fn is_unbounded(&self) -> bool {
        self.lower.is_none() && self.upper.is_none() && self.unsupported.is_empty()
    }

    /// Whether `version` falls inside the declared range.
    ///
    /// Unknown specifiers are ignored here; call [`evaluate`] to learn that the
    /// declaration is unusable.
    #[must_use]
    pub fn contains(&self, version: CoreVersion) -> bool {
        if let Some((lower, kind)) = self.lower {
            let admitted = match kind {
                BoundKind::Inclusive => version >= lower,
                BoundKind::Exclusive => version > lower,
            };
            if !admitted {
                return false;
            }
        }
        if let Some((upper, kind)) = self.upper {
            let admitted = match kind {
                BoundKind::Inclusive => version <= upper,
                BoundKind::Exclusive => version < upper,
            };
            if !admitted {
                return false;
            }
        }
        true
    }

    /// Canonical specifier set for this range (`">=0.19.3,<0.20.0"`).
    #[must_use]
    pub fn to_spec(&self) -> String {
        let mut parts = Vec::new();
        if let Some((lower, _)) = self.lower {
            parts.push(format!(">={lower}"));
        }
        if let Some((upper, kind)) = self.upper {
            match kind {
                BoundKind::Inclusive => parts.push(format!("<={upper}")),
                BoundKind::Exclusive => parts.push(format!("<{upper}")),
            }
        }
        if parts.is_empty() {
            "*".to_string()
        } else {
            parts.join(",")
        }
    }

    /// Parse either a packaging requirement or a package environment request.
    #[must_use]
    pub fn parse(declaration: &str) -> Self {
        let text = declaration.trim();
        if text.is_empty() {
            // Callers distinguish an empty declaration from an unreadable one
            // through [`evaluate`]; there is no specifier to report here.
            return Self::default();
        }
        if text.contains(['<', '>', '=', '!', '~']) {
            Self::parse_specifier_set(text)
        } else {
            Self::parse_package_environment(text)
        }
    }

    fn parse_specifier_set(text: &str) -> Self {
        let without_marker = text.split(';').next().unwrap_or(text);
        let unparenthesized: String = without_marker
            .chars()
            .filter(|character| !matches!(character, '(' | ')'))
            .collect();
        let start = unparenthesized.find(['<', '>', '=', '!', '~']).unwrap_or(0);
        let mut requirement = Self::default();
        for raw in unparenthesized[start..].split(',') {
            let specifier = raw.trim();
            if !specifier.is_empty() {
                requirement.apply_specifier(specifier);
            }
        }
        requirement
    }

    fn apply_specifier(&mut self, specifier: &str) {
        let (operator, version_text) = split_operator(specifier);
        let Some(version) = CoreVersion::parse(version_text) else {
            self.unsupported.push(specifier.to_string());
            return;
        };
        match operator {
            ">=" => self.raise_lower(version, BoundKind::Inclusive),
            ">" => self.raise_lower(version, BoundKind::Exclusive),
            "<=" => self.lower_upper(version, BoundKind::Inclusive),
            "<" => self.lower_upper(version, BoundKind::Exclusive),
            "==" | "===" => {
                self.raise_lower(version, BoundKind::Inclusive);
                self.lower_upper(version, BoundKind::Inclusive);
            }
            // PEP 440 compatible release: `~=0.19.3` is `>=0.19.3,<0.20.0`.
            "~=" => {
                self.raise_lower(version, BoundKind::Inclusive);
                self.lower_upper(version.next_minor(), BoundKind::Exclusive);
            }
            _ => self.unsupported.push(specifier.to_string()),
        }
    }

    fn parse_package_environment(text: &str) -> Self {
        let Some(range) = request_range(text) else {
            // A bare name such as `dcc_mcp_core` or `dcc-mcp-3dsmax`.
            return Self::default();
        };
        if let Some((lower_text, upper_text)) = range.split_once("..") {
            let mut unsupported = Vec::new();
            let lower = Self::version_or_unsupported(lower_text, &mut unsupported);
            let upper = Self::version_or_unsupported(upper_text, &mut unsupported);
            return Self {
                lower: lower.map(|version| (version, BoundKind::Inclusive)),
                upper: upper.map(|version| (version, BoundKind::Exclusive)),
                unsupported,
            };
        }
        let mut unsupported = Vec::new();
        match Self::version_or_unsupported(range, &mut unsupported) {
            Some(version) => {
                let components = range.split('.').count();
                let upper = match components {
                    // `dcc_mcp_core-0.19.3` pins exactly one release.
                    3.. => (version, BoundKind::Inclusive),
                    // `dcc_mcp_core-0.20` admits the whole 0.20 line.
                    2 => (version.next_minor(), BoundKind::Exclusive),
                    // `dcc_mcp_core-0` admits every 0.x line.
                    _ => (
                        CoreVersion::new(version.major + 1, 0, 0),
                        BoundKind::Exclusive,
                    ),
                };
                Self {
                    lower: Some((version, BoundKind::Inclusive)),
                    upper: Some(upper),
                    unsupported,
                }
            }
            None => Self {
                unsupported,
                ..Self::default()
            },
        }
    }

    fn version_or_unsupported(text: &str, unsupported: &mut Vec<String>) -> Option<CoreVersion> {
        let version = CoreVersion::parse(text);
        if version.is_none() {
            unsupported.push(text.to_string());
        }
        version
    }

    fn raise_lower(&mut self, version: CoreVersion, kind: BoundKind) {
        if self.lower.is_none_or(|(current, current_kind)| {
            stricter_lower((version, kind), (current, current_kind))
        }) {
            self.lower = Some((version, kind));
        }
    }

    fn lower_upper(&mut self, version: CoreVersion, kind: BoundKind) {
        if self.upper.is_none_or(|(current, current_kind)| {
            stricter_upper((version, kind), (current, current_kind))
        }) {
            self.upper = Some((version, kind));
        }
    }
}

impl fmt::Display for CoreRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_spec())
    }
}

fn split_operator(specifier: &str) -> (&str, &str) {
    for operator in ["===", "==", "!=", "~=", "<=", ">="] {
        if let Some(version) = specifier.strip_prefix(operator) {
            return (operator, version.trim());
        }
    }
    for operator in ["<", ">"] {
        if let Some(version) = specifier.strip_prefix(operator) {
            return (operator, version.trim());
        }
    }
    ("", specifier.trim())
}

fn stricter_lower(candidate: (CoreVersion, BoundKind), current: (CoreVersion, BoundKind)) -> bool {
    (candidate.0, matches!(candidate.1, BoundKind::Exclusive))
        > (current.0, matches!(current.1, BoundKind::Exclusive))
}

fn stricter_upper(candidate: (CoreVersion, BoundKind), current: (CoreVersion, BoundKind)) -> bool {
    (candidate.0, matches!(candidate.1, BoundKind::Inclusive))
        < (current.0, matches!(current.1, BoundKind::Inclusive))
}

/// Version range of a `<name>-<range>` request, if the suffix is version-like.
fn request_range(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    for index in (0..bytes.len()).rev() {
        if bytes[index] != b'-' || index + 1 >= bytes.len() {
            continue;
        }
        let suffix = &text[index + 1..];
        if !suffix.is_empty()
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return Some(suffix);
        }
    }
    None
}

// ── policy ───────────────────────────────────────────────────────────────────

/// Why a declaration does not satisfy the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreBoundCode {
    /// No lower bound: the declaration admits every older core release.
    MissingLowerBound,
    /// No upper bound: the declaration admits every newer core release.
    MissingUpperBound,
    /// Upper bound admits more minor lines than the policy allows.
    UpperBoundTooWide,
    /// A specifier could not be interpreted.
    UnsupportedSpecifier,
    /// Empty or unreadable declaration.
    UnparsableDeclaration,
    /// The range admits no version at all.
    InvertedRange,
}

impl CoreBoundCode {
    /// Machine-readable snake_case identifier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingLowerBound => "missing_lower_bound",
            Self::MissingUpperBound => "missing_upper_bound",
            Self::UpperBoundTooWide => "upper_bound_too_wide",
            Self::UnsupportedSpecifier => "unsupported_specifier",
            Self::UnparsableDeclaration => "unparsable_declaration",
            Self::InvertedRange => "inverted_range",
        }
    }

    /// Operator-facing explanation.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::MissingLowerBound => {
                "declares no lower bound, so any older core release resolves"
            }
            Self::MissingUpperBound => {
                "declares no upper bound, so any newer core release resolves"
            }
            Self::UpperBoundTooWide => {
                "upper bound admits more than one core minor line, and every minor bump may break an adapter"
            }
            Self::UnsupportedSpecifier => "contains a specifier that cannot be interpreted",
            Self::UnparsableDeclaration => "is empty or carries no readable version range",
            Self::InvertedRange => "admits no core release at all",
        }
    }
}

impl fmt::Display for CoreBoundCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The version-bound contract an adapter declaration is checked against.
///
/// Defaults mirror `compatibility/core-bounds.json`
/// (`tests::contract_defaults_match_json_contract` keeps them in sync).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreBoundPolicy {
    /// Reject declarations without a lower bound.
    pub require_lower_bound: bool,
    /// Reject declarations without an upper bound.
    pub require_upper_bound: bool,
    /// Number of core minor lines one declaration may admit.
    pub max_minor_lines: u32,
}

impl Default for CoreBoundPolicy {
    fn default() -> Self {
        Self {
            require_lower_bound: true,
            require_upper_bound: true,
            max_minor_lines: 1,
        }
    }
}

/// Result of checking one declaration against a [`CoreBoundPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBoundReport {
    /// The declaration as supplied by the caller.
    pub declaration: String,
    /// Parsed lower bound (`">=0.19.3"` / `">0.19.3"`).
    pub lower: Option<String>,
    /// Parsed upper bound (`"<0.20.0"` / `"<=0.20.3"`).
    pub upper: Option<String>,
    /// Contract violations; empty means the declaration satisfies the policy.
    pub codes: Vec<CoreBoundCode>,
    /// Canonical bounded range that would satisfy the policy, when derivable.
    pub suggestion: Option<String>,
}

impl CoreBoundReport {
    /// Whether the declaration satisfies the policy.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.codes.is_empty()
    }

    /// One-line operator-facing summary, or `"ok"` when compliant.
    #[must_use]
    pub fn message(&self) -> String {
        if self.codes.is_empty() {
            return "ok".to_string();
        }
        let reasons: Vec<String> = self
            .codes
            .iter()
            .map(|code| format!("declaration '{}' {}", self.declaration, code.description()))
            .collect();
        let mut message = reasons.join("; ");
        if let Some(suggestion) = self.suggestion.as_deref() {
            message.push_str(&format!("; use '{suggestion}'"));
        }
        message
    }
}

/// Check one declaration against `policy`.
#[must_use]
pub fn evaluate(declaration: &str, policy: &CoreBoundPolicy) -> CoreBoundReport {
    let requirement = CoreRequirement::parse(declaration);
    let mut codes = Vec::new();

    if declaration.trim().is_empty() {
        codes.push(CoreBoundCode::UnparsableDeclaration);
    }
    if !requirement.unsupported.is_empty() {
        codes.push(CoreBoundCode::UnsupportedSpecifier);
    }

    match (requirement.lower, requirement.upper) {
        (None, None) => {
            if policy.require_lower_bound && !codes.contains(&CoreBoundCode::UnparsableDeclaration)
            {
                codes.push(CoreBoundCode::MissingLowerBound);
            }
            if policy.require_upper_bound && !codes.contains(&CoreBoundCode::UnparsableDeclaration)
            {
                codes.push(CoreBoundCode::MissingUpperBound);
            }
        }
        (None, Some(_)) => {
            if policy.require_lower_bound {
                codes.push(CoreBoundCode::MissingLowerBound);
            }
        }
        (Some(_), None) => {
            if policy.require_upper_bound {
                codes.push(CoreBoundCode::MissingUpperBound);
            }
        }
        (Some((lower, _)), Some((upper, upper_kind))) => {
            let inverted = lower > upper || (lower == upper && upper_kind == BoundKind::Exclusive);
            if inverted {
                codes.push(CoreBoundCode::InvertedRange);
            } else if !within_minor_lines(lower, upper, upper_kind, policy.max_minor_lines) {
                codes.push(CoreBoundCode::UpperBoundTooWide);
            }
        }
    }

    let suggestion = suggestion_for(&requirement, policy, &codes);
    CoreBoundReport {
        declaration: declaration.to_string(),
        lower: requirement
            .lower
            .map(|(version, kind)| format!("{}{version}", operator_prefix(kind, true))),
        upper: requirement
            .upper
            .map(|(version, kind)| format!("{}{version}", operator_prefix(kind, false))),
        codes,
        suggestion,
    }
}

fn operator_prefix(kind: BoundKind, is_lower: bool) -> &'static str {
    match (kind, is_lower) {
        (BoundKind::Inclusive, true) => ">=",
        (BoundKind::Exclusive, true) => ">",
        (BoundKind::Inclusive, false) => "<=",
        (BoundKind::Exclusive, false) => "<",
    }
}

fn within_minor_lines(
    lower: CoreVersion,
    upper: CoreVersion,
    upper_kind: BoundKind,
    max_minor_lines: u32,
) -> bool {
    let limit = lower.minor_line_limit(max_minor_lines);
    match upper_kind {
        BoundKind::Exclusive => upper <= limit,
        BoundKind::Inclusive => upper < limit,
    }
}

fn suggestion_for(
    requirement: &CoreRequirement,
    policy: &CoreBoundPolicy,
    codes: &[CoreBoundCode],
) -> Option<String> {
    if codes.is_empty() {
        return None;
    }
    let (lower, _) = requirement.lower?;
    if lower == CoreVersion::new(0, 0, 0) {
        // `dcc_mcp_core-0` carries no usable floor, so narrowing it would invent
        // a supported range the adapter never declared.
        return None;
    }
    Some(format!(
        ">={lower},<{}",
        lower.minor_line_limit(policy.max_minor_lines)
    ))
}

/// Canonical bounded range for a minimum core version.
///
/// `derive_requirement("0.19.3")` is `Some(">=0.19.3,<0.20.0")`; unreadable
/// input yields `None`.
#[must_use]
pub fn derive_requirement(min_core_version: &str) -> Option<String> {
    let lower = CoreVersion::parse(min_core_version)?;
    Some(format!(
        ">={lower},<{}",
        lower.minor_line_limit(CoreBoundPolicy::default().max_minor_lines)
    ))
}

// ── runtime comparison ───────────────────────────────────────────────────────

/// How the executing core relates to an adapter's declared range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeVerdict {
    /// The running core is inside the declared range.
    Supported,
    /// The running core is newer than the declared upper bound.
    CoreNewerThanDeclared,
    /// The running core is older than the declared lower bound.
    CoreOlderThanDeclared,
    /// The declaration itself violates the policy or could not be parsed.
    DeclarationUnusable,
    /// The running core version could not be read.
    UnknownCoreVersion,
}

impl RuntimeVerdict {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::CoreNewerThanDeclared => "core_newer_than_declared",
            Self::CoreOlderThanDeclared => "core_older_than_declared",
            Self::DeclarationUnusable => "declaration_unusable",
            Self::UnknownCoreVersion => "unknown_core_version",
        }
    }
}

impl fmt::Display for RuntimeVerdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Result of comparing a declaration against the executing core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreRuntimeReport {
    /// The declaration as supplied by the caller.
    pub declaration: String,
    /// The executing core version, when readable.
    pub running: Option<String>,
    /// Relationship between the declaration and the executing core.
    pub verdict: RuntimeVerdict,
    /// The bound check for the same declaration.
    pub bound: CoreBoundReport,
}

impl CoreRuntimeReport {
    /// Whether the executing core is inside a policy-compliant declaration.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.verdict == RuntimeVerdict::Supported
    }
}

/// Compare a declaration against the core version that is actually running.
#[must_use]
pub fn check_runtime(
    declaration: &str,
    running_core: &str,
    policy: &CoreBoundPolicy,
) -> CoreRuntimeReport {
    let bound = evaluate(declaration, policy);
    let running = CoreVersion::parse(running_core);
    let verdict = if !bound.is_ok() {
        RuntimeVerdict::DeclarationUnusable
    } else {
        match running {
            None => RuntimeVerdict::UnknownCoreVersion,
            Some(version) => {
                let requirement = CoreRequirement::parse(declaration);
                if requirement.contains(version) {
                    RuntimeVerdict::Supported
                } else if requirement.lower.is_some_and(|(lower, _)| version < lower) {
                    RuntimeVerdict::CoreOlderThanDeclared
                } else {
                    RuntimeVerdict::CoreNewerThanDeclared
                }
            }
        }
    };
    CoreRuntimeReport {
        declaration: declaration.to_string(),
        running: running.map(|version| version.to_string()),
        verdict,
        bound,
    }
}

// ── cross-declaration drift ──────────────────────────────────────────────────

/// Why two declarations of the same dependency disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationDrift {
    /// Both sides admit the same versions.
    Aligned,
    /// The package environment request admits versions the packaging metadata
    /// excludes — the failure that lets an untested core resolve.
    EnvironmentWider,
    /// The packaging metadata admits versions the environment request excludes.
    PackagingWider,
    /// One or both declarations violate the bound policy.
    DeclarationUnusable,
}

impl DeclarationDrift {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aligned => "aligned",
            Self::EnvironmentWider => "environment_wider",
            Self::PackagingWider => "packaging_wider",
            Self::DeclarationUnusable => "declaration_unusable",
        }
    }
}

impl fmt::Display for DeclarationDrift {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Result of comparing the packaging declaration with the package environment
/// request of the same adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreBoundComparison {
    /// Packaging metadata declaration (PyPI `Requires-Dist`, `pyproject.toml`).
    pub packaging: String,
    /// Package environment request (package environment / resolver request).
    pub environment: String,
    /// How the two declarations relate.
    pub drift: DeclarationDrift,
}

/// Compare the two declarations an adapter publishes for the same dependency.
///
/// This is the check that catches the reported failure: packaging metadata
/// pinning `>=0.19.3,<0.19.5` while the package environment requests
/// `dcc_mcp_core-0`.
#[must_use]
pub fn compare_declarations(packaging: &str, environment: &str) -> CoreBoundComparison {
    let packaging_requirement = CoreRequirement::parse(packaging);
    let environment_requirement = CoreRequirement::parse(environment);
    // Drift is the actionable finding, so it wins over a policy violation: an
    // environment request that is both too wide and wider than the packaging
    // declaration is still reported as `EnvironmentWider`. Only a declaration
    // whose range cannot be read at all makes the comparison unusable.
    let readable = |declaration: &str| {
        !declaration.trim().is_empty() && CoreRequirement::parse(declaration).unsupported.is_empty()
    };
    let drift = if !readable(packaging) || !readable(environment) {
        DeclarationDrift::DeclarationUnusable
    } else {
        let environment_wider = lower_rank(&environment_requirement)
            < lower_rank(&packaging_requirement)
            || upper_rank(&environment_requirement) > upper_rank(&packaging_requirement);
        let packaging_wider = lower_rank(&packaging_requirement)
            < lower_rank(&environment_requirement)
            || upper_rank(&packaging_requirement) > upper_rank(&environment_requirement);
        if environment_wider {
            DeclarationDrift::EnvironmentWider
        } else if packaging_wider {
            DeclarationDrift::PackagingWider
        } else {
            DeclarationDrift::Aligned
        }
    };
    CoreBoundComparison {
        packaging: packaging.to_string(),
        environment: environment.to_string(),
        drift,
    }
}

/// Lower-bound strictness: higher admits fewer older versions.
fn lower_rank(requirement: &CoreRequirement) -> (u8, CoreVersion, u8) {
    match requirement.lower {
        None => (0, CoreVersion::new(0, 0, 0), 0),
        Some((version, kind)) => (1, version, u8::from(matches!(kind, BoundKind::Exclusive))),
    }
}

/// Upper-bound strictness: lower admits fewer newer versions.
fn upper_rank(requirement: &CoreRequirement) -> (u8, CoreVersion, u8) {
    match requirement.upper {
        None => (1, CoreVersion::new(0, 0, 0), 0),
        Some((version, kind)) => (0, version, u8::from(matches!(kind, BoundKind::Inclusive))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTRACT_JSON: &str = include_str!("../../../compatibility/core-bounds.json");

    fn policy() -> CoreBoundPolicy {
        CoreBoundPolicy::default()
    }

    #[test]
    fn parses_packaging_requirements() {
        let requirement = CoreRequirement::parse("dcc-mcp-core>=0.19.3,<0.19.5");
        assert_eq!(
            requirement.lower,
            Some((CoreVersion::new(0, 19, 3), BoundKind::Inclusive))
        );
        assert_eq!(
            requirement.upper,
            Some((CoreVersion::new(0, 19, 5), BoundKind::Exclusive))
        );
        assert_eq!(requirement.to_spec(), ">=0.19.3,<0.19.5");
    }

    #[test]
    fn parses_named_requirements_with_extras_and_markers() {
        let requirement = CoreRequirement::parse(
            "dcc-mcp-core[server] (>=0.20.14,<0.21.0); python_version >= \"3.8\"",
        );
        assert_eq!(requirement.to_spec(), ">=0.20.14,<0.21.0");
    }

    #[test]
    fn parses_compatible_release_operator() {
        let requirement = CoreRequirement::parse("~=0.19.3");
        assert_eq!(requirement.to_spec(), ">=0.19.3,<0.20.0");
        assert!(evaluate("~=0.19.3", &policy()).is_ok());
    }

    #[test]
    fn parses_package_environment_requests() {
        assert_eq!(
            CoreRequirement::parse("dcc_mcp_core-0.20").to_spec(),
            ">=0.20.0,<0.21.0"
        );
        assert_eq!(
            CoreRequirement::parse("dcc_mcp_core-0.19.3").to_spec(),
            ">=0.19.3,<=0.19.3"
        );
        assert_eq!(
            CoreRequirement::parse("dcc_mcp_core-0.12.18..1").to_spec(),
            ">=0.12.18,<1.0.0"
        );
        // `dcc_mcp_core-0` means "any 0.x": one hundred breaking minor lines.
        assert_eq!(
            CoreRequirement::parse("dcc_mcp_core-0").to_spec(),
            ">=0.0.0,<1.0.0"
        );
    }

    #[test]
    fn bare_names_and_versionless_suffixes_are_unbounded() {
        assert!(CoreRequirement::parse("dcc_mcp_core").is_unbounded());
        assert!(CoreRequirement::parse("dcc-mcp-3dsmax").is_unbounded());
    }

    #[test]
    fn unknown_specifiers_are_reported_not_guessed() {
        let requirement = CoreRequirement::parse("dcc-mcp-core>=0.19.3,===weird");
        assert_eq!(requirement.unsupported, vec!["===weird".to_string()]);
        // The unknown specifier also means the upper bound is unknown, so the
        // declaration fails closed on both counts.
        let report = evaluate("dcc-mcp-core>=0.19.3,===weird", &policy());
        assert!(
            report.codes.contains(&CoreBoundCode::UnsupportedSpecifier),
            "{:?}",
            report.codes
        );
        assert!(
            report.codes.contains(&CoreBoundCode::MissingUpperBound),
            "{:?}",
            report.codes
        );
    }

    #[test]
    fn compliant_declarations_pass() {
        for declaration in [
            ">=0.19.3,<0.19.5",
            "dcc-mcp-core>=0.20.14,<0.21.0",
            "dcc_mcp_core-0.20",
            ">=0.20.0,<=0.20.9",
        ] {
            let report = evaluate(declaration, &policy());
            assert!(report.is_ok(), "{declaration}: {}", report.message());
        }
    }

    #[test]
    fn unbounded_and_wide_declarations_fail() {
        let cases = [
            ("dcc_mcp_core", CoreBoundCode::MissingUpperBound),
            ("dcc_mcp_core-0", CoreBoundCode::UpperBoundTooWide),
            (">=0.20.14", CoreBoundCode::MissingUpperBound),
            ("<0.21.0", CoreBoundCode::MissingLowerBound),
            (">=0.12.18,<1.0.0", CoreBoundCode::UpperBoundTooWide),
            (">=0.18.21,<1.0.0", CoreBoundCode::UpperBoundTooWide),
            ("<=0.19.0", CoreBoundCode::MissingLowerBound),
        ];
        for (declaration, expected) in cases {
            let report = evaluate(declaration, &policy());
            assert!(!report.is_ok(), "{declaration} should violate the policy");
            assert!(
                report.codes.contains(&expected),
                "{declaration}: {:?}",
                report.codes
            );
        }
    }

    #[test]
    fn empty_and_inverted_declarations_fail() {
        assert_eq!(
            evaluate("", &policy()).codes,
            vec![CoreBoundCode::UnparsableDeclaration]
        );
        assert_eq!(
            evaluate("   ", &policy()).codes,
            vec![CoreBoundCode::UnparsableDeclaration]
        );
        // A suffix that is not version-like is an unbounded request, not an
        // unreadable one: no range was ever declared.
        assert_eq!(
            evaluate("dcc_mcp_core-?", &policy()).codes,
            vec![
                CoreBoundCode::MissingLowerBound,
                CoreBoundCode::MissingUpperBound
            ]
        );
        assert!(
            evaluate(">=0.20.0,<0.19.0", &policy())
                .codes
                .contains(&CoreBoundCode::InvertedRange)
        );
    }

    #[test]
    fn suggestion_narrows_to_one_minor_line() {
        let maya = evaluate(">=0.19.3", &policy());
        assert_eq!(maya.suggestion.as_deref(), Some(">=0.19.3,<0.20.0"));
        // `dcc_mcp_core-0` has no usable floor, so no range is invented for it.
        let any_zero = evaluate("dcc_mcp_core-0", &policy());
        assert_eq!(any_zero.suggestion, None);
    }

    #[test]
    fn derive_requirement_bounds_one_minor_line() {
        assert_eq!(
            derive_requirement("0.19.3").as_deref(),
            Some(">=0.19.3,<0.20.0")
        );
        assert_eq!(
            derive_requirement("0.20").as_deref(),
            Some(">=0.20.0,<0.21.0")
        );
        assert_eq!(derive_requirement("not-a-version"), None);
    }

    #[test]
    fn runtime_check_flags_the_reported_combination() {
        // dcc-mcp-maya 0.9.4 declares >=0.19.3,<0.19.5 but the environment
        // resolved core 0.20.28.
        let report = check_runtime(">=0.19.3,<0.19.5", "0.20.28", &policy());
        assert_eq!(report.verdict, RuntimeVerdict::CoreNewerThanDeclared);
        assert!(!report.is_ok());

        let older = check_runtime(">=0.19.3,<0.19.5", "0.18.0", &policy());
        assert_eq!(older.verdict, RuntimeVerdict::CoreOlderThanDeclared);

        let supported = check_runtime(">=0.19.3,<0.19.5", "0.19.4", &policy());
        assert_eq!(supported.verdict, RuntimeVerdict::Supported);
        assert!(supported.is_ok());

        let unusable = check_runtime("dcc_mcp_core-0", "0.20.28", &policy());
        assert_eq!(unusable.verdict, RuntimeVerdict::DeclarationUnusable);
    }

    #[test]
    fn comparison_catches_environment_wider_than_packaging() {
        let comparison = compare_declarations(">=0.19.3,<0.19.5", "dcc_mcp_core-0");
        assert_eq!(comparison.drift, DeclarationDrift::EnvironmentWider);

        let aligned = compare_declarations(">=0.20.0,<0.21.0", "dcc_mcp_core-0.20");
        assert_eq!(aligned.drift, DeclarationDrift::Aligned);

        let packaging_wider = compare_declarations(">=0.19.3,<0.21.0", "dcc_mcp_core-0.20");
        assert_eq!(packaging_wider.drift, DeclarationDrift::PackagingWider);

        // An unreadable range makes the comparison unusable instead of guessing.
        let unusable = compare_declarations(">=0.19.3,<0.19.5", ">=0.19.3,===weird");
        assert_eq!(unusable.drift, DeclarationDrift::DeclarationUnusable);
        let empty = compare_declarations(">=0.19.3,<0.19.5", "");
        assert_eq!(empty.drift, DeclarationDrift::DeclarationUnusable);
    }

    #[test]
    fn contract_defaults_match_json_contract() {
        let contract: serde_json::Value =
            serde_json::from_str(CONTRACT_JSON).expect("compatibility/core-bounds.json must parse");
        let policy_json = &contract["adapter_requirement_policy"];
        let defaults = CoreBoundPolicy::default();
        assert_eq!(
            policy_json["require_lower_bound"],
            defaults.require_lower_bound
        );
        assert_eq!(
            policy_json["require_upper_bound"],
            defaults.require_upper_bound
        );
        assert_eq!(policy_json["max_minor_lines"], defaults.max_minor_lines);
        assert_eq!(contract["distribution"], CORE_DISTRIBUTION);
        assert_eq!(contract["import_name"], CORE_IMPORT_NAME);
    }

    #[test]
    fn reported_cases_match_the_contract() {
        let contract: serde_json::Value =
            serde_json::from_str(CONTRACT_JSON).expect("compatibility/core-bounds.json must parse");
        let cases = contract["reported_cases"]
            .as_array()
            .expect("reported_cases array");
        assert!(!cases.is_empty());
        for case in cases {
            for (field, declaration) in [
                (
                    "packaging_requirement",
                    case["packaging_requirement"].as_str().unwrap_or_default(),
                ),
                (
                    "package_environment_request",
                    case["package_environment_request"]
                        .as_str()
                        .unwrap_or_default(),
                ),
            ] {
                let expected: Vec<&str> = case["expected_codes"][field]
                    .as_array()
                    .map(|codes| codes.iter().filter_map(|code| code.as_str()).collect())
                    .unwrap_or_default();
                let actual: Vec<String> = evaluate(declaration, &policy())
                    .codes
                    .iter()
                    .map(|code| code.as_str().to_string())
                    .collect();
                assert_eq!(
                    actual,
                    expected,
                    "{} {} {}: expected {expected:?}, got {actual:?}",
                    case["adapter"].as_str().unwrap_or_default(),
                    case["adapter_version"].as_str().unwrap_or_default(),
                    field
                );
            }
            // The observed core must be outside an adapter's declared range
            // whenever the environment request is the wider side.
            let comparison = compare_declarations(
                case["packaging_requirement"].as_str().unwrap_or_default(),
                case["package_environment_request"]
                    .as_str()
                    .unwrap_or_default(),
            );
            let observed = case["observed_core"].as_str().unwrap_or_default();
            if comparison.drift == DeclarationDrift::EnvironmentWider {
                let runtime = check_runtime(
                    case["packaging_requirement"].as_str().unwrap_or_default(),
                    observed,
                    &policy(),
                );
                assert_ne!(
                    runtime.verdict,
                    RuntimeVerdict::Supported,
                    "{}: observed core {observed} should be outside the declared range",
                    case["adapter"].as_str().unwrap_or_default()
                );
            }
        }
    }
}
