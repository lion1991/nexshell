use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NexPaneId(u64);

impl NexPaneId {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NexPaneId(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub fn position_id(&self) -> String {
        format!("nexshell_pane_{}", self.0)
    }
}
