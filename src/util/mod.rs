pub mod bindings;
pub mod clipboard;
pub mod extensions;
pub mod grid;

use std::cmp::Ordering;
use std::ops::Range;

// 合并相邻/重叠区间（端口自 warp app/src/util/mod.rs）
pub fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut i = 1;
    while i < ranges.len() {
        if ranges[i - 1].end.cmp(&ranges[i].start) >= Ordering::Equal {
            let removed = ranges.remove(i);
            if removed.start.cmp(&ranges[i - 1].start) < Ordering::Equal {
                ranges[i - 1].start = removed.start;
            }
            if removed.end.cmp(&ranges[i - 1].end) > Ordering::Equal {
                ranges[i - 1].end = removed.end;
            }
        } else {
            i += 1;
        }
    }
    ranges
}
