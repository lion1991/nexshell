// 切片扩展：二分查找插入位置（端口自 warp app/src/util/extensions.rs）
use std::cmp::Ordering;

pub trait SliceExt<T: 'static> {
    fn find_insertion_index<'a, F, E>(&'a self, compare: F) -> Result<usize, E>
    where
        F: FnMut(&'a T) -> Result<Ordering, E>;
}

impl<T: 'static> SliceExt<T> for [T] {
    fn find_insertion_index<'a, F, E>(&'a self, mut f: F) -> Result<usize, E>
    where
        F: FnMut(&'a T) -> Result<Ordering, E>,
    {
        use Ordering::*;

        let mut size = self.len();
        if size == 0 {
            return Ok(0);
        }
        let mut base = 0usize;
        while size > 1 {
            let half = size / 2;
            let mid = base + half;
            // mid 始终落在 [0, size)
            let cmp = f(unsafe { self.get_unchecked(mid) })?;
            base = if cmp == Greater { base } else { mid };
            size -= half;
        }
        // base 始终落在 [0, size)（base <= mid）
        let cmp = f(unsafe { self.get_unchecked(base) })?;
        if cmp == Equal {
            Ok(base)
        } else {
            Ok(base + (cmp == Less) as usize)
        }
    }
}
