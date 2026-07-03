// rdp_view — RDP 整页 UI 子组件层（类比 host_management_view/）。
// 纯渲染 Element + 几何纯函数；不含 impl RootView（那在 root_view/rdp_section.rs）。

pub mod geometry;
pub mod keymap;
pub mod page_element;

pub use geometry::{rdp_desktop_size, RdpViewport};
pub use page_element::RdpPageElement;
