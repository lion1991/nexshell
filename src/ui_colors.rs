//! UI 与主机概览的派生颜色门面。真正的派生逻辑已迁到 lib 的 design_tokens；
//! 此处只 re-export，保持旧类型名 + `from_theme` 签名，消费方 call-site 零改动。

pub(crate) use nexshell::design_tokens::ChromeColors as UiColors;
pub(crate) use nexshell::design_tokens::OverviewColors as HostOverviewColors;
