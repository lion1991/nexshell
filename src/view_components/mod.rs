//! 替代 warp::view_components 的本地子集（code_editor 解耦所需：action_button + find）。

pub mod action_button;
pub mod find;

#[derive(Clone, Default)]
pub struct DismissibleToast;

impl DismissibleToast {
    pub fn error(_msg: impl Into<String>) -> Self {
        Self
    }
    pub fn success(_msg: impl Into<String>) -> Self {
        Self
    }
}
