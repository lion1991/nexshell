use std::collections::HashSet;

use warp_core::channel::ChannelState;
pub use warp_core::features::*;

/// 在当前 channel 上启用应启用的全部 feature flag；勿在单测调用。
pub fn init_feature_flags() {
    for flag in enabled_features() {
        flag.set_enabled(true);
    }
    mark_initialized();
}

/// 内置编辑器(code_viewer)需要的 Warp editor flag：vim 模式 + alt 多光标。
/// 默认 false，解耦前由 warp app 的 init_feature_flags 启用，本地化后须在此显式补回。
const NEXSHELL_EDITOR_FLAGS: [FeatureFlag; 2] =
    [FeatureFlag::VimCodeEditor, FeatureFlag::RichTextMultiselect];

/// 当前 channel 应启用的 flag。nexshell 不启用 Warp app 特有 feature
/// （原 Warp 的 ~200 个 `#[cfg(feature=…)]` flag 全被排除），仅取 channel 额外 flag
/// + 内置编辑器必需 flag +（release 构建时）RELEASE_FLAGS。
fn enabled_features() -> HashSet<FeatureFlag> {
    let mut flags = ChannelState::additional_features();
    flags.extend(NEXSHELL_EDITOR_FLAGS);
    if ChannelState::is_release_bundle() {
        flags.extend(RELEASE_FLAGS);
    }
    flags
}
