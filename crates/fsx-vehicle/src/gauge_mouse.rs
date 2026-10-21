//! Independently decoded x86 pack(4) mouse records. No pointer is callable.
use super::{callback_rva, u32_at, Pe, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Parent,
    Child,
    EndParent,
    Tooltip,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(kind: MouseKind, parent: Option<usize>, b: [i32; 4]) -> MouseRect {
        MouseRect {
            rva: 1,
            kind,
            parent,
            relative_box: b,
            cursor: 5,
            help_id: 0,
            mouse_flags: 1 << 29,
            event_id: 100,
            callback_rva: None,
            raw_function_slot: 0,
            tooltip_text_rva: None,
            api_data: 0,
        }
    }
    fn fixture() -> MouseList {
        MouseList {
            rva: 1,
            terminator_rva: 2,
            records: vec![
                rect(MouseKind::Parent, None, [0, 0, 0, 0]),
                rect(MouseKind::Parent, Some(0), [10, 10, 20, 20]),
                rect(MouseKind::Parent, Some(1), [15, 15, 30, 30]),
                rect(MouseKind::Child, Some(2), [0, 0, 30, 30]),
                rect(MouseKind::Child, Some(2), [4, 4, 10, 10]),
                rect(MouseKind::Tooltip, Some(2), [0, 0, i32::MAX, i32::MIN]),
            ],
        }
    }
    #[test]
    fn mouse_hits_use_gauge_parent_positions_clip_ancestors_and_preserve_overlap_order() {
        let list = fixture();
        let regions = list.regions([100, 100]).unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].rectangle, [15., 15., 30., 30.]);
        assert_eq!(regions[0].clip, [15., 15., 30., 30.]);
        let hits = list
            .hit_candidates([100, 100], [20., 20.], 1 << 29)
            .unwrap();
        assert_eq!(
            hits.iter().map(|h| h.record_index).collect::<Vec<_>>(),
            [3, 4]
        );
        assert_eq!(hits[0].child_local_point, [5., 5.]);
        assert_eq!(hits[1].child_local_point, [1., 1.]);
        for point in [[30., 20.], [20., 30.], [14.999, 20.], [-1., 20.]] {
            assert!(list
                .hit_candidates([100, 100], point, 1 << 29)
                .unwrap()
                .is_empty());
        }
        assert_eq!(
            list.hit_candidates([100, 100], [15., 15.], 1 << 29)
                .unwrap()
                .len(),
            1
        );
        assert!(list
            .hit_candidates([100, 100], [20., 20.], 1 << 31)
            .unwrap()
            .is_empty());
        let mut all = list.clone();
        all.records[3].mouse_flags = 1 << 10;
        assert_eq!(
            all.hit_candidates([100, 100], [20., 20.], 1 << 31)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn mouse_layout_rejects_invalid_parents_sizes_points_and_activity_flags() {
        let list = fixture();
        for flags in [0, 1 << 10, 1 << 15, (1 << 29) | (1 << 31)] {
            assert!(list.hit_candidates([100, 100], [20., 20.], flags).is_err());
        }
        for point in [[f64::NAN, 0.], [0., f64::INFINITY], [1e10, 0.]] {
            assert!(list.hit_candidates([100, 100], point, 1 << 29).is_err());
        }
        assert!(list.regions([0, 100]).is_err());
        assert!(list.regions([100, 65537]).is_err());
        let mut bad = list.clone();
        bad.records[3].parent = Some(3);
        assert!(bad.regions([100, 100]).is_err());
        let mut bad = list.clone();
        bad.records[3].relative_box[2] = 0;
        assert!(bad.regions([100, 100]).is_err());
        let mut bad = list;
        bad.records[3].relative_box[2] = -1;
        assert!(bad.regions([100, 100]).is_err());
    }
}

#[derive(Debug, Clone)]
pub struct MouseRect {
    pub rva: u32,
    pub kind: MouseKind,
    /// Index of the containing parent record; list order is retained.
    pub parent: Option<usize>,
    /// x/y/width/height. For Tooltip records these are overloaded metadata,
    /// not geometry: the final two fields carry metric/US help identifiers.
    pub relative_box: [i32; 4],
    pub cursor: i32,
    pub help_id: i32,
    pub mouse_flags: u32,
    /// Tooltip records use this field for an argument count, not an event.
    pub event_id: i32,
    pub callback_rva: Option<u32>,
    pub raw_function_slot: u32,
    /// A Tooltip's function slot is text metadata, never an executable callback.
    pub tooltip_text_rva: Option<u32>,
    /// Opaque API/argument slot; no runtime pointer or argument graph is followed.
    pub api_data: u32,
}

#[derive(Debug, Clone)]
pub struct MouseList {
    pub rva: u32,
    pub terminator_rva: u32,
    pub records: Vec<MouseRect>,
}
#[derive(Debug, Clone)]
pub struct MouseRegion {
    pub record_index: usize,
    /// Gauge-native x/y/width/height, before ancestor clipping.
    pub rectangle: [f64; 4],
    /// Left/top/right/bottom after clipping to gauge and containing parents.
    pub clip: [f64; 4],
}
#[derive(Debug, Clone)]
pub struct MouseHit {
    pub record_index: usize,
    pub child_local_point: [f64; 2],
}
impl MouseList {
    /// Authored positive-size regions. Runtime-installed zero-size children need
    /// installation semantics and fail explicitly rather than guessing an extent.
    pub fn regions(&self, native_size: [u32; 2]) -> Result<Vec<MouseRegion>> {
        if self.records.is_empty()
            || self.records.len() > 2048
            || native_size.iter().any(|&v| v == 0 || v > 65536)
        {
            return Err("Invalid native mouse layout budget/size".into());
        }
        let gauge_clip = [0., 0., native_size[0] as f64, native_size[1] as f64];
        let mut parents: Vec<Option<([f64; 2], [f64; 4])>> = vec![None; self.records.len()];
        let mut regions = Vec::new();
        for (i, record) in self.records.iter().enumerate() {
            if !matches!(record.kind, MouseKind::Parent | MouseKind::Child) {
                continue;
            }
            let b = record.relative_box;
            if b.iter().any(|v| v.unsigned_abs() > 65536) || b[2] < 0 || b[3] < 0 {
                return Err("Invalid native mouse geometry".into());
            }
            let parent = if let Some(index) = record.parent {
                if index >= i {
                    return Err("Native mouse parent is not an earlier record".into());
                }
                Some(
                    parents
                        .get(index)
                        .and_then(|p| *p)
                        .ok_or("Native mouse parent is not a region")?,
                )
            } else {
                None
            };
            let mut rectangle = b.map(f64::from);
            if i == 0 && record.kind == MouseKind::Parent && parent.is_none() && b == [0, 0, 0, 0] {
                rectangle = [0., 0., native_size[0] as f64, native_size[1] as f64];
            } else if b[2] == 0 || b[3] == 0 {
                return Err(
                    "Zero-size native mouse subregions require runtime installation".into(),
                );
            }
            if record.kind == MouseKind::Child {
                let (origin, _) = parent.ok_or("Native mouse child has no parent region")?;
                rectangle[0] += origin[0];
                rectangle[1] += origin[1];
            }
            // Published MOUSE_PARENT x/y are gauge-relative; child x/y are local
            // to their containing parent. Parent coordinates are not summed twice.
            let bounds = [
                rectangle[0],
                rectangle[1],
                rectangle[0] + rectangle[2],
                rectangle[1] + rectangle[3],
            ];
            let container = parent.map(|(_, clip)| clip).unwrap_or(gauge_clip);
            let clip = [
                bounds[0].max(container[0]),
                bounds[1].max(container[1]),
                bounds[2].min(container[2]),
                bounds[3].min(container[3]),
            ];
            if record.kind == MouseKind::Parent {
                parents[i] = Some(([rectangle[0], rectangle[1]], clip));
            } else {
                regions.push(MouseRegion {
                    record_index: i,
                    rectangle,
                    clip,
                });
            }
        }
        Ok(regions)
    }
    /// Return every candidate in authored order. This does not choose overlap
    /// priority, dispatch callbacks, or implement drag/repeat/wheel routing.
    pub fn hit_candidates(
        &self,
        native_size: [u32; 2],
        point: [f64; 2],
        event_flags: u32,
    ) -> Result<Vec<MouseHit>> {
        const EVENTS: u32 = 0xffc0_0000 | 0x000e_0000 | 0x0000_6000 | 0x0000_0800;
        if point.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
            || event_flags.count_ones() != 1
            || event_flags & EVENTS == 0
        {
            return Err("Mouse hit test requires finite coordinates and one activity flag".into());
        }
        let mut hits = Vec::new();
        for region in self.regions(native_size)? {
            let record = &self.records[region.record_index];
            let [left, top, right, bottom] = region.clip;
            if point[0] >= left
                && point[0] < right
                && point[1] >= top
                && point[1] < bottom
                && (record.mouse_flags & event_flags != 0 || record.mouse_flags & (1 << 10) != 0)
            {
                hits.push(MouseHit {
                    record_index: region.record_index,
                    child_local_point: [
                        point[0] - region.rectangle[0],
                        point[1] - region.rectangle[1],
                    ],
                });
            }
        }
        Ok(hits)
    }
    pub(super) fn decode(pe: &Pe<'_>, rva: u32) -> Result<Self> {
        let mut records = Vec::new();
        let mut parents = Vec::new();
        for index in 0..=2048u32 {
            let address = rva
                .checked_add(index * 44)
                .ok_or("Mouse list RVA overflow")?;
            let b = pe.slice(address, 44)?;
            let raw_kind = u32_at(b, 0)?;
            if raw_kind == 0 {
                if records.is_empty() || !parents.is_empty() {
                    return Err("Mouse list is empty or has unclosed parents".into());
                }
                return Ok(Self {
                    rva,
                    terminator_rva: address,
                    records,
                });
            }
            if index == 2048 {
                return Err("Native mouse list exceeds 2048 records".into());
            }
            let kind = match raw_kind {
                1 => MouseKind::Parent,
                2 => MouseKind::Child,
                3 => MouseKind::EndParent,
                4 => MouseKind::Tooltip,
                _ => return Err("Unknown native mouse record kind".into()),
            };
            if records.is_empty() && kind != MouseKind::Parent {
                return Err("Native mouse list must begin with a parent".into());
            }
            let parent = parents.last().copied();
            if kind == MouseKind::Child && parent.is_none() {
                return Err("Native mouse child has no parent".into());
            }
            let relative_box = [
                u32_at(b, 4)? as i32,
                u32_at(b, 8)? as i32,
                u32_at(b, 12)? as i32,
                u32_at(b, 16)? as i32,
            ];
            if matches!(kind, MouseKind::Parent | MouseKind::Child)
                && (relative_box.iter().any(|v| v.unsigned_abs() > 65536)
                    || relative_box[2] < 0
                    || relative_box[3] < 0)
            {
                return Err("Native mouse geometry exceeds bounds".into());
            }
            let pointer = u32_at(b, 36)?;
            let callback = if kind == MouseKind::Child {
                callback_rva(pe, pointer)?
            } else {
                None
            };
            let tooltip = if kind == MouseKind::Tooltip {
                callback_rva(pe, pointer)?
            } else {
                None
            };
            records.push(MouseRect {
                rva: address,
                kind,
                parent,
                relative_box,
                cursor: u32_at(b, 20)? as i32,
                help_id: u32_at(b, 24)? as i32,
                mouse_flags: u32_at(b, 28)?,
                event_id: u32_at(b, 32)? as i32,
                callback_rva: callback,
                raw_function_slot: pointer,
                tooltip_text_rva: tooltip,
                api_data: u32_at(b, 40)?,
            });
            match kind {
                MouseKind::Parent => {
                    if parents.len() >= 32 {
                        return Err("Native mouse nesting exceeds 32 parents".into());
                    }
                    parents.push(records.len() - 1);
                }
                MouseKind::EndParent => {
                    parents
                        .pop()
                        .ok_or("Native mouse end-parent has no matching parent")?;
                }
                _ => {}
            }
        }
        Err("Unterminated native mouse list".into())
    }
}
