use super::*;

/// Deserialize `allowed-tools` from either a space-delimited string or a YAML list.
pub(super) fn deserialize_allowed_tools<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct AllowedToolsVisitor;

    impl<'de> Visitor<'de> for AllowedToolsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a space-delimited string or a sequence of tool names")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        fn visit_string<E: de::Error>(self, s: String) -> Result<Self::Value, E> {
            Ok(s.split_whitespace().map(String::from).collect())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut tools = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                tools.push(value);
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_any(AllowedToolsVisitor)
}

/// Custom deserializer for `tools` — accepts both string names and full objects.
pub(super) fn deserialize_tool_declarations<'de, D>(
    deserializer: D,
) -> Result<Vec<ToolDeclaration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};
    use std::fmt;

    struct ToolDeclarationsVisitor;

    impl<'de> Visitor<'de> for ToolDeclarationsVisitor {
        type Value = Vec<ToolDeclaration>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "a sequence of tool name strings or tool declaration objects"
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut tools = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                match &value {
                    serde_json::Value::String(name) => {
                        tools.push(ToolDeclaration {
                            name: name.clone(),
                            ..Default::default()
                        });
                    }
                    serde_json::Value::Object(_) => {
                        let declaration: ToolDeclaration =
                            serde_json::from_value(value).map_err(de::Error::custom)?;
                        tools.push(declaration);
                    }
                    _ => {
                        return Err(de::Error::custom(
                            "each tool must be a string name or a declaration object",
                        ));
                    }
                }
            }
            Ok(tools)
        }
    }

    deserializer.deserialize_seq(ToolDeclarationsVisitor)
}

pub(super) fn default_dcc() -> String {
    DEFAULT_DCC.to_string()
}

pub(super) fn default_version() -> String {
    DEFAULT_VERSION.to_string()
}
