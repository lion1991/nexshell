// rdp_view — RDP 整页 UI 子组件层（类比 host_management_view/）。
// 纯渲染 Element + 几何纯函数；不含 impl RootView（那在 root_view/rdp_section.rs）。

pub mod geometry;
pub mod hotkey_guard;
pub mod keymap;
pub mod page_element;
pub mod pointer;

pub use geometry::{rdp_desktop_scale_factor, rdp_desktop_size, RdpViewport, ResizeDebounce};
pub use hotkey_guard::HotkeyGuardSlot;
pub use page_element::RdpPageElement;
pub use pointer::pointer_to_cursor;
