//! Gateway capability policy domain types.
//!
//! The policy is intentionally pure: it depends only on gateway domain
//! records and returns structured denials. Transport layers decide how to
//! serialize those denials for MCP or REST.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityRecord;

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
        record: &CapabilityRecord,
    ) -> Result<(), GatewayPolicyDenial> {
        if !self.allows_dcc(&record.dcc_type) {
            return Err(self.denial(
                GatewayPolicyDenyReason::DccAllowlist,
                operation,
                Some(&record.dcc_type),
                record.skill_name.as_deref(),
                Some(&record.tool_slug),
            ));
        }
        if !self.allows_skill(record.skill_name.as_deref()) {
            return Err(self.denial(
                GatewayPolicyDenyReason::SkillAllowlist,
                operation,
                Some(&record.dcc_type),
                record.skill_name.as_deref(),
                Some(&record.tool_slug),
            ));
        }
        if !self.allows_tool_slug(&record.tool_slug) {
            return Err(self.denial(
                GatewayPolicyDenyReason::ToolAllowlist,
                operation,
                Some(&record.dcc_type),
                record.skill_name.as_deref(),
                Some(&record.tool_slug),
            ));
        }
        if operation == GatewayPolicyOperation::Call && self.read_only {
            let read_only_hint = record
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.read_only_hint);
            if read_only_hint != Some(true) {
                return Err(self.denial(
                    GatewayPolicyDenyReason::ReadOnly,
                    operation,
                    Some(&record.dcc_type),
                    record.skill_name.as_deref(),
                    Some(&record.tool_slug),
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

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::capability::{CapabilityAnnotations, tool_slug};

    fn record(
        dcc: &str,
        skill: Option<&str>,
        tool: &str,
        read_only: Option<bool>,
    ) -> CapabilityRecord {
        let iid = Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
        CapabilityRecord::new(
            tool_slug(dcc, &iid, tool),
            tool.to_string(),
            tool.to_string(),
            skill.map(str::to_string),
            "",
            Vec::new(),
            dcc.to_string(),
            iid,
            true,
            true,
            None,
        )
        .with_surface_metadata(
            read_only.map(|read_only_hint| CapabilityAnnotations {
                title: None,
                read_only_hint: Some(read_only_hint),
                destructive_hint: None,
                idempotent_hint: None,
                open_world_hint: None,
            }),
            None,
        )
    }

    #[test]
    fn unrestricted_policy_allows_records() {
        let policy = GatewayPolicy::default();
        let record = record("maya", Some("maya-modeling"), "create_cube", Some(false));

        assert!(
            policy
                .enforce_record(GatewayPolicyOperation::Call, &record)
                .is_ok()
        );
    }

    #[test]
    fn dcc_and_tool_allowlists_deny_unlisted_records() {
        let policy = GatewayPolicy {
            allowed_dcc_types: vec!["maya".to_string()],
            allowed_tool_slug_prefixes: vec!["maya.abcdef01.read".to_string()],
            ..Default::default()
        };

        let photoshop = record("photoshop", Some("layers"), "read_layer", Some(true));
        let maya_write = record("maya", Some("maya-modeling"), "write_scene", Some(false));

        let dcc_err = policy
            .enforce_record(GatewayPolicyOperation::Describe, &photoshop)
            .unwrap_err();
        assert_eq!(dcc_err.reason, GatewayPolicyDenyReason::DccAllowlist);

        let tool_err = policy
            .enforce_record(GatewayPolicyOperation::Describe, &maya_write)
            .unwrap_err();
        assert_eq!(tool_err.reason, GatewayPolicyDenyReason::ToolAllowlist);
    }

    #[test]
    fn skill_family_prefix_allows_related_skills() {
        let policy = GatewayPolicy {
            allowed_skill_families: vec!["maya-".to_string()],
            ..Default::default()
        };

        let allowed = record("maya", Some("maya-modeling"), "read_scene", Some(true));
        let denied = record("custom", Some("custom-admin"), "read_scene", Some(true));

        assert!(
            policy
                .enforce_record(GatewayPolicyOperation::Search, &allowed)
                .is_ok()
        );
        let err = policy
            .enforce_record(GatewayPolicyOperation::Search, &denied)
            .unwrap_err();
        assert_eq!(err.reason, GatewayPolicyDenyReason::SkillAllowlist);
    }

    #[test]
    fn read_only_blocks_load_skill_and_non_read_only_calls_only() {
        let policy = GatewayPolicy {
            read_only: true,
            ..Default::default()
        };
        let read = record("maya", Some("maya-inspect"), "read_scene", Some(true));
        let write = record("maya", Some("maya-modeling"), "create_cube", Some(false));

        assert!(
            policy
                .enforce_record(GatewayPolicyOperation::Describe, &write)
                .is_ok()
        );
        assert!(
            policy
                .enforce_record(GatewayPolicyOperation::Call, &read)
                .is_ok()
        );
        let err = policy
            .enforce_record(GatewayPolicyOperation::Call, &write)
            .unwrap_err();
        assert_eq!(err.reason, GatewayPolicyDenyReason::ReadOnly);

        let err = policy
            .enforce_skill_operation(
                GatewayPolicyOperation::LoadSkill,
                Some("maya"),
                ["maya-modeling"],
            )
            .unwrap_err();
        assert_eq!(err.reason, GatewayPolicyDenyReason::ReadOnly);
    }
}
