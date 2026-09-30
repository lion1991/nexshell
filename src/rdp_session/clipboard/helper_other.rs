//! 非 macOS 没有辅助进程：远端内容退回立即拉取文本（见 remote.rs）。

use super::wire::{Data, Offer, Request};

pub(super) const LAZY: bool = false;

pub(super) fn offer(_offer: Offer) -> bool {
    false
}

pub(super) fn reply(_id: u64, _data: Option<Data>) {}

/// 收不到任何请求；自持发送端，免得通道关闭后事件循环空转。
pub(in crate::rdp_session) struct PasteRequests {
    rx: async_channel::Receiver<Request>,
    _tx: async_channel::Sender<Request>,
}

impl PasteRequests {
    pub(super) fn register(_session: u64) -> Self {
        let (tx, rx) = async_channel::bounded(1);
        Self { rx, _tx: tx }
    }

    pub(in crate::rdp_session) async fn recv(&self) -> Option<Request> {
        self.rx.recv().await.ok()
    }
}
