//! Gateway capability policy value types (ADR-037 Cut 1).
//!
//! `GatewayPolicy` and its denial vocabulary are operator-written
//! configuration: the allowlists that bound the gateway's dynamic
//! capability surface. They are pure serde value types with no I/O and no
//! async runtime, and they do not depend on the gateway domain crate —
//! which is why they live in the transport-neutral configuration crate
//! instead of `dcc-mcp-gateway-core`.
//!
//! Policy *evaluation* ([`GatewayPolicy::enforce_record`]) is generic over
//! [`PolicySubject`] so this crate never names `CapabilityRecord`.
//! `dcc-mcp-gateway-core` implements the trait for its own records and
//! re-exports these types, so `dcc_mcp_gateway_core::policy::*` paths keep
//! compiling unchanged.

use serde::{Deserialize, Serialize};

/// The subset of a capability record that policy evaluation inspects.
///
/// Implemented by `dcc_mcp_gateway_core::capability::CapabilityRecord` so
/// the policy value types stay free of any gateway-domain dependency.
pub trait PolicySubject {
    /// DCC type bucket of the subject (for example `"maya"`).
    fn policy_dcc_type(&self) -> &str;
    /// Owning skill name, when the backend advertised one.
    fn policy_skill_name(&self) -> Option<&str>;
    /// Canonical gateway tool slug.
    fn policy_tool_slug(&self) -> &str;
    /// Read-only hint advertised by the backend, when present.
    fn policy_read_only_hint(&self) -> Option<bool> {
        None
    }
}

/// Blanket forwarding impl so `&T` and `&mut T` can be passed wherever a
/// `PolicySubject` is expected, keeping existing call sites unchanged.
impl<T: PolicySubject + ?Sized> PolicySubject for &T {
    fn policy_dcc_type(&self) -> &str {
        (**self).policy_dcc_type()
    }

    fn policy_skill_name(&self) -> Option<&str> {
        (**self).policy_skill_name()
    }

    fn policy_tool_slug(&self) -> &str {
        (**self).policy_tool_slug()
    }

    fn policy_read_only_hint(&self) -> Option<bool> {
        (**self).policy_read_only_hint()
    }
}

/// Operation being evaluated by [`GatewayPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayPolicyOperation {
    /// Capability or skill discovery.
    Search,
    /// Schema or skill detail lookup.
    Describe,
    /// Progressive skill loading or tool-group activation.
    LoadSkill,
    /// Backend capability execution.
    Call,
}

impl GatewayPolicyOperation {
    /// Stable wire name for this operation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Describe => "describe",
            Self::LoadSkill => "load_skill",
            Self::Call => "call",
        }
    }
}

/// Machine-readable reason for a policy denial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayPolicyDenyReason {
    /// Read-only mode rejected a state-changing operation.
    ReadOnly,
    /// The DCC type is outside `allowed_dcc_types`.
    DccAllowlist,
    /// The skill name is outside `allowed_skill_names` /
    /// `allowed_skill_families`.
    SkillAllowlist,
    /// The canonical tool slug is outside `allowed_tool_slugs` /
    /// `allowed_tool_slug_prefixes`.
    ToolAllowlist,
}

impl GatewayPolicyDenyReason {
    /// Stable wire name for this denial reason.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::DccAllowlist => "dcc-allowlist",
            Self::SkillAllowlist => "skill-allowlist",
            Self::ToolAllowlist => "tool-allowlist",
        }
    }
}

/// Structured policy denial carried in `policy-denied` errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPolicyDenial {
    /// Reason for the denial.
    pub reason: GatewayPolicyDenyReason,
    /// Operation that was denied.
    pub operation: GatewayPolicyOperation,
    /// Human-readable explanation.
    pub message: String,
    /// Effective read-only flag at the time of evaluation.
    pub read_only: bool,
    /// DCC type involved in the decision, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    /// Skill name involved in the decision, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    /// Tool slug involved in the decision, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_slug: Option<String>,
}

/// Gateway policy controlling the dynamic capability surface.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayPolicy {
    /// When true, `load_skill` and non-read-only backend calls are rejected.
    pub read_only: bool,
    /// Allowed DCC types. Empty means any DCC type is allowed.
    pub allowed_dcc_types: Vec<String>,
    /// Exact allowed skill names. Empty with no families means any skill name is allowed.
    pub allowed_skill_names: Vec<String>,
    /// Allowed skill family prefixes. Empty with no names means any skill name is allowed.
    pub allowed_skill_families: Vec<String>,
    /// Exact canonical gateway tool slugs. Empty with no prefixes means any tool slug is allowed.
    pub allowed_tool_slugs: Vec<String>,
    /// Allowed canonical gateway tool slug prefixes.
    pub allowed_tool_slug_prefixes: Vec<String>,
}

impl GatewayPolicy {
    /// Return true when the policy has no active restrictions.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        !self.read_only
            && self.allowed_dcc_types.is_empty()
            && self.allowed_skill_names.is_empty()
            && self.allowed_skill_families.is_empty()
            && self.allowed_tool_slugs.is_empty()
            && self.allowed_tool_slug_prefixes.is_empty()
    }

    /// Return true when a DCC type is allowed.
    #[must_use]
    pub fn allows_dcc(&self, dcc_type: &str) -> bool {
        self.allowed_dcc_types.is_empty()
            || self
                .allowed_dcc_types
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(dcc_type))
    }

    /// Return true when a skill is allowed.
    #[must_use]
    pub fn allows_skill(&self, skill_name: Option<&str>) -> bool {
        if self.allowed_skill_names.is_empty() && self.allowed_skill_families.is_empty() {
            return true;
        }
        let Some(skill_name) = skill_name else {
            return false;
        };
        let skill_lc = skill_name.to_ascii_lowercase();
        self.allowed_skill_names
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(skill_name))
            || self.allowed_skill_families.iter().any(|family| {
                let family = family.to_ascii_lowercase();
                skill_lc == family || skill_lc.starts_with(&family)
            })
    }

    /// Return true when a canonical gateway tool slug is allowed.
    #[must_use]
    pub fn allows_tool_slug(&self, tool_slug: &str) -> bool {
        if self.allowed_tool_slugs.is_empty() && self.allowed_tool_slug_prefixes.is_empty() {
            return true;
        }
        let slug_lc = tool_slug.to_ascii_lowercase();
        self.allowed_tool_slugs
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(tool_slug))
            || self.allowed_tool_slug_prefixes.iter().any(|prefix| {
                let prefix = prefix.to_ascii_lowercase();
                slug_lc == prefix || slug_lc.starts_with(&prefix)
            })
    }

    /// Enforce policy for a capability record.
    ///
    /// Search and describe ignore read-only mode so discovery remains useful.
    /// Call enforces read-only by requiring `annotations.readOnlyHint = true`.
    pub fn enforce_record(
        &self,
        operation: GatewayPolicyOperation,
        record: &impl PolicySubject,
    ) -> Result<(), GatewayPolicyDenial> {
        if !self.allows_dcc(record.policy_dcc_type()) {
            return Err(self.denial(
                GatewayPolicyDenyReason::DccAllowlist,
                operation,
                Some(record.policy_dcc_type()),
                record.policy_skill_name(),
                Some(record.policy_tool_slug()),
            ));
        }
        if !self.allows_skill(record.policy_skill_name()) {
            return Err(self.denial(
                GatewayPolicyDenyReason::SkillAllowlist,
                operation,
                Some(record.policy_dcc_type()),
                record.policy_skill_name(),
                Some(record.policy_tool_slug()),
            ));
        }
        if !self.allows_tool_slug(record.policy_tool_slug()) {
            return Err(self.denial(
                GatewayPolicyDenyReason::ToolAllowlist,
                operation,
                Some(record.policy_dcc_type()),
                record.policy_skill_name(),
                Some(record.policy_tool_slug()),
            ));
        }
        if operation == GatewayPolicyOperation::Call && self.read_only {
            let read_only_hint = record.policy_read_only_hint();
            if read_only_hint != Some(true) {
                return Err(self.denial(
                    GatewayPolicyDenyReason::ReadOnly,
                    operation,
                    Some(record.policy_dcc_type()),
                    record.policy_skill_name(),
                    Some(record.policy_tool_slug()),
                ));
            }
        }
        Ok(())
    }

    /// Enforce policy for a skill lifecycle operation.
    pub fn enforce_skill_operation<'a, I>(
        &self,
        operation: GatewayPolicyOperation,
        dcc_type: Option<&str>,
        skill_names: I,
    ) -> Result<(), GatewayPolicyDenial>
    where
        I: IntoIterator<Item = &'a str>,
    {
        if self.read_only && operation == GatewayPolicyOperation::LoadSkill {
            return Err(self.denial(
                GatewayPolicyDenyReason::ReadOnly,
                operation,
                dcc_type,
                None,
                None,
            ));
        }
        if let Some(dcc_type) = dcc_type
            && !self.allows_dcc(dcc_type)
        {
            return Err(self.denial(
                GatewayPolicyDenyReason::DccAllowlist,
                operation,
                Some(dcc_type),
                None,
                None,
            ));
        }
        for skill_name in skill_names {
            if !self.allows_skill(Some(skill_name)) {
                return Err(self.denial(
                    GatewayPolicyDenyReason::SkillAllowlist,
                    operation,
                    dcc_type,
                    Some(skill_name),
                    None,
                ));
            }
        }
        Ok(())
    }

    fn denial(
        &self,
        reason: GatewayPolicyDenyReason,
        operation: GatewayPolicyOperation,
        dcc_type: Option<&str>,
        skill_name: Option<&str>,
        tool_slug: Option<&str>,
    ) -> GatewayPolicyDenial {
        let subject = tool_slug.or(skill_name).or(dcc_type).unwrap_or("operation");
        let message = format!(
            "Gateway policy denied {} for {subject}: {}",
            operation.as_str(),
            reason.as_str()
        );
        GatewayPolicyDenial {
            reason,
            operation,
            message,
            read_only: self.read_only,
            dcc_type: dcc_type.map(str::to_string),
            skill_name: skill_name.map(str::to_string),
            tool_slug: tool_slug.map(str::to_string),
        }
    }
}
