//! 单调操作代号，用于阻止已失效实例的异步结果命中新实例。

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Generation(u64);

impl Generation {
    pub const INVALID: Self = Self(0);

    pub fn new(value: u64) -> Option<Self> {
        (value != 0).then_some(Self(value))
    }
}

#[derive(Default)]
pub struct GenerationAllocator {
    next: u64,
}

impl GenerationAllocator {
    pub fn allocate(&mut self) -> Generation {
        self.next = self.next.wrapping_add(1);
        if self.next == 0 {
            self.next = 1;
        }
        Generation(self.next)
    }
}

pub fn accepts_generation(current: Option<Generation>, incoming: Generation) -> bool {
    incoming != Generation::INVALID && current == Some(incoming)
}

#[cfg(test)]
mod tests {
    use super::{accepts_generation, Generation, GenerationAllocator};

    #[test]
    fn allocator_skips_the_invalid_generation_when_wrapping() {
        let mut allocator = GenerationAllocator { next: u64::MAX };

        assert_eq!(allocator.allocate(), Generation::new(1).unwrap());
    }

    #[test]
    fn invalid_generation_never_matches_an_inactive_operation() {
        assert!(!accepts_generation(
            Some(Generation::INVALID),
            Generation::INVALID,
        ));
    }
}
