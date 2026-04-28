use super::*;

use crate::constants::{DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_METADATA_FILE};

pub(super) mod fixtures;
mod test_enumerate;
mod test_extract_frontmatter;
mod test_layer_field;
mod test_load_all_skills;
mod test_merge_depends;
mod test_metadata_compat;
mod test_next_tools;
mod test_parse_skill_md;
mod test_scan_and_load;
mod test_scan_and_load_lenient;
mod test_tool_annotations;
