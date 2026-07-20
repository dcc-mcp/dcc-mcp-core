use crate::ComputerUsePoint;

const DRAG_UPDATE_INTERVAL_MS: u64 = 16;

pub(crate) fn drag_step_count(path_len: usize, duration_ms: u64) -> usize {
    path_len
        .saturating_sub(1)
        .max(duration_ms.div_ceil(DRAG_UPDATE_INTERVAL_MS) as usize)
}

pub(crate) fn interpolated_drag_path(
    path: &[ComputerUsePoint],
    duration_ms: u64,
) -> Vec<ComputerUsePoint> {
    let segment_count = path.len().saturating_sub(1);
    if segment_count == 0 {
        return Vec::new();
    }
    let step_count = drag_step_count(path.len(), duration_ms);
    let mut points = Vec::with_capacity(step_count);
    let mut allocated = 0;
    for segment in 0..segment_count {
        let segment_end = (segment + 1) * step_count / segment_count;
        let segment_steps = segment_end - allocated;
        let from = path[segment];
        let to = path[segment + 1];
        for step in 1..=segment_steps {
            let fraction = step as f64 / segment_steps as f64;
            points.push(ComputerUsePoint {
                x: from.x + (to.x - from.x) * fraction,
                y: from.y + (to.y - from.y) * fraction,
            });
        }
        allocated = segment_end;
    }
    points
}
