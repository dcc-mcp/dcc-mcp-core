use std::sync::atomic::Ordering;

use crate::adapters::{
    BoundingBox, DccError, DccErrorCode, DccResult, DccTransform, ObjectTransform,
};

use super::MockDccAdapter;

impl DccTransform for MockDccAdapter {
    fn get_transform(&self, object_name: &str) -> DccResult<ObjectTransform> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_transform")?;

        let transforms = self.transforms.read();
        transforms
            .get(object_name)
            .cloned()
            .ok_or_else(|| DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Transform not found for: {object_name}"),
                details: None,
                recoverable: false,
            })
    }

    fn set_transform(
        &self,
        object_name: &str,
        translate: Option<[f64; 3]>,
        rotate: Option<[f64; 3]>,
        scale: Option<[f64; 3]>,
    ) -> DccResult<ObjectTransform> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("set_transform")?;

        let mut transforms = self.transforms.write();
        let entry = transforms
            .entry(object_name.to_string())
            .or_insert_with(ObjectTransform::identity);

        if let Some(t) = translate {
            entry.translate = t;
        }
        if let Some(r) = rotate {
            entry.rotate = r;
        }
        if let Some(s) = scale {
            entry.scale = s;
        }

        Ok(entry.clone())
    }

    fn get_bounding_box(&self, object_name: &str) -> DccResult<BoundingBox> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("get_bounding_box")?;

        let bbs = self.bounding_boxes.read();
        bbs.get(object_name).cloned().ok_or_else(|| DccError {
            code: DccErrorCode::InvalidInput,
            message: format!("Bounding box not found for: {object_name}"),
            details: None,
            recoverable: false,
        })
    }

    fn rename_object(&self, old_name: &str, new_name: &str) -> DccResult<String> {
        self.transform_count.fetch_add(1, Ordering::Relaxed);
        self.require_connected("rename_object")?;

        let mut objects = self.objects.write();
        let found = objects
            .iter_mut()
            .find(|o| o.name == old_name || o.long_name == old_name);

        match found {
            Some(obj) => {
                obj.name = new_name.to_string();
                Ok(new_name.to_string())
            }
            None => Err(DccError {
                code: DccErrorCode::InvalidInput,
                message: format!("Object not found: {old_name}"),
                details: None,
                recoverable: false,
            }),
        }
    }
}
