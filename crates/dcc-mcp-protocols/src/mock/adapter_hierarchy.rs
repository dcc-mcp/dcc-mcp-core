use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::adapters::{
    DccAdapter, DccCapabilities, DccConnection, DccHierarchy, DccRenderCapture, DccResult,
    DccSceneInfo, DccSceneManager, DccScriptEngine, DccSnapshot, DccTransform, SceneNode,
    SceneObject,
};

use super::MockDccAdapter;

impl DccHierarchy for MockDccAdapter {
    fn get_hierarchy(&self) -> DccResult<Vec<SceneNode>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_hierarchy")?;
        Ok(self.hierarchy.read().clone())
    }

    fn get_children(&self, object_name: Option<&str>) -> DccResult<Vec<SceneObject>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_children")?;

        match object_name {
            None => {
                let children = self
                    .hierarchy
                    .read()
                    .iter()
                    .map(|n| n.object.clone())
                    .collect();
                Ok(children)
            }
            Some(name) => {
                let hierarchy = self.hierarchy.read();
                let children = find_children_in_tree(&hierarchy, name);
                Ok(children)
            }
        }
    }

    fn get_parent(&self, object_name: &str) -> DccResult<Option<String>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_parent")?;

        let objects = self.objects.read();
        let obj = objects
            .iter()
            .find(|o| o.name == object_name || o.long_name == object_name);

        Ok(obj.and_then(|o| o.parent.clone()))
    }

    fn group_objects(
        &self,
        object_names: &[&str],
        group_name: &str,
        parent: Option<&str>,
    ) -> DccResult<SceneObject> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("group_objects")?;

        let group = SceneObject {
            name: group_name.to_string(),
            long_name: format!("|{group_name}"),
            object_type: "group".to_string(),
            parent: parent.map(String::from),
            visible: true,
            metadata: HashMap::from([("children".to_string(), object_names.join(","))]),
        };
        self.objects.write().push(group.clone());
        Ok(group)
    }

    fn ungroup(&self, group_name: &str) -> DccResult<Vec<String>> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("ungroup")?;

        let mut objects = self.objects.write();
        let pos = objects
            .iter()
            .position(|o| o.name == group_name || o.long_name == group_name);

        match pos {
            Some(idx) => {
                let group = objects.remove(idx);
                let children: Vec<String> = group
                    .metadata
                    .get("children")
                    .map(|s| s.split(',').map(String::from).collect())
                    .unwrap_or_default();
                Ok(children)
            }
            None => Err(crate::adapters::DccError {
                code: crate::adapters::DccErrorCode::InvalidInput,
                message: format!("Group not found: {group_name}"),
                details: None,
                recoverable: false,
            }),
        }
    }

    fn reparent(
        &self,
        object_name: &str,
        new_parent: Option<&str>,
        _preserve_world_transform: bool,
    ) -> DccResult<SceneObject> {
        self.hierarchy_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("reparent")?;

        let mut objects = self.objects.write();
        let obj = objects
            .iter_mut()
            .find(|o| o.name == object_name || o.long_name == object_name);

        match obj {
            Some(o) => {
                o.parent = new_parent.map(String::from);
                Ok(o.clone())
            }
            None => Err(crate::adapters::DccError {
                code: crate::adapters::DccErrorCode::InvalidInput,
                message: format!("Object not found: {object_name}"),
                details: None,
                recoverable: false,
            }),
        }
    }
}

impl DccAdapter for MockDccAdapter {
    fn info(&self) -> &crate::adapters::DccInfo {
        &self.info
    }

    fn capabilities(&self) -> DccCapabilities {
        self.capabilities.clone()
    }

    fn as_connection(&mut self) -> Option<&mut dyn DccConnection> {
        Some(self)
    }

    fn as_script_engine(&self) -> Option<&dyn DccScriptEngine> {
        Some(self)
    }

    fn as_scene_info(&self) -> Option<&dyn DccSceneInfo> {
        Some(self)
    }

    fn as_snapshot(&self) -> Option<&dyn DccSnapshot> {
        if self.snapshot_enabled {
            Some(self)
        } else {
            None
        }
    }

    fn as_scene_manager(&self) -> Option<&dyn DccSceneManager> {
        Some(self)
    }

    fn as_transform(&self) -> Option<&dyn DccTransform> {
        Some(self)
    }

    fn as_render_capture(&self) -> Option<&dyn DccRenderCapture> {
        if self.snapshot_enabled {
            Some(self)
        } else {
            None
        }
    }

    fn as_hierarchy(&self) -> Option<&dyn DccHierarchy> {
        Some(self)
    }
}

fn find_children_in_tree(nodes: &[SceneNode], target_name: &str) -> Vec<SceneObject> {
    for node in nodes {
        if node.object.name == target_name || node.object.long_name == target_name {
            return node.children.iter().map(|c| c.object.clone()).collect();
        }
        let found = find_children_in_tree(&node.children, target_name);
        if !found.is_empty() {
            return found;
        }
    }
    vec![]
}
