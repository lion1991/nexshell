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

/// 当前 channel 应启用的 flag。nexshell 不启用任何 Warp app 特有 feature
/// （原 Warp 的 ~200 个 `#[cfg(feature=…)]` flag 在此全被排除），
/// 故仅取 channel 额外 flag +（release 构建时）RELEASE_FLAGS。
fn enabled_features() -> HashSet<FeatureFlag> {
    let mut flags = ChannelState::additional_features();
    if ChannelState::is_release_bundle() {
        flags.extend(RELEASE_FLAGS);
    }
    flags
}
