//! 替代 warp::view_components 的本地 no-op stub（砍 ai/toast 后只剩类型签名需求）。

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
