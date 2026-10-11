// toast_section — RootView 底部提示条：把 host_state.notice（操作结果 / 报错）淡入显示几秒再淡出。
//
// 本文件只含 impl RootView，无自由函数。flush_notice_toast 由 handle_action 收尾与 idle refresh
// 调用（异步回调里写的 notice 由后者 ≤100ms 接走）；render_toast 由 mod.rs render 挂到根 Stack。

use nexshell::ui_anim::{scale_alpha, ToastTimeline};
use std::time::Instant;
use warpui::elements::{Border, ConstrainedBox, Container, CornerRadius, Radius, Text};
use warpui::r#async::Timer;
use warpui::{Element, ViewContext};

use crate::RootView;

const TOAST_MAX_WIDTH: f32 = 520.0;
/// 淡入时从下方上浮的距离。
pub(in crate::root_view) const TOAST_RISE: f32 = 8.0;

impl RootView {
    /// 接走新写入的 notice：重新计时显示，到点唤醒 tick 播淡出。
    pub(in crate::root_view) fn flush_notice_toast(&mut self, ctx: &mut ViewContext<Self>) {
        let now = Instant::now();
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, timeline)| timeline.is_finished(now))
        {
            self.toast = None;
        }
        let Some(text) = self.host_state.notice.take() else {
            return;
        };
        let timeline = ToastTimeline::new(&text, now);
        ctx.spawn(Timer::after(timeline.fade_out_delay()), |me, _, ctx| {
            me.wake_ui_anim(ctx);
        });
        self.toast = Some((text, timeline));
        self.wake_ui_anim(ctx);
        ctx.notify();
    }

    pub(in crate::root_view) fn toast_is_fading(&self, now: Instant) -> bool {
        self.toast
            .as_ref()
            .is_some_and(|(_, timeline)| timeline.is_fading(now))
    }

    /// 返回提示条及入场进度（供上浮偏移）；未显示 / 已淡出返回 None。
    pub(in crate::root_view) fn render_toast(&self) -> Option<(Box<dyn Element>, f32)> {
        let (text, timeline) = self.toast.as_ref()?;
        let now = Instant::now();
        if timeline.is_finished(now) {
            return None;
        }
        let opacity = timeline.opacity(now);
        let colors = self.design_tokens.overview;
        let card = Container::new(
            Text::new(text.clone(), self.ui_font, 13.0)
                .with_color(scale_alpha(colors.text_primary, opacity))
                .finish(),
        )
        .with_horizontal_padding(16.0)
        .with_vertical_padding(10.0)
        .with_background_color(scale_alpha(colors.card_bg, opacity))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.0)))
        .with_border(Border::all(1.0).with_border_color(scale_alpha(colors.panel_border, opacity)))
        .finish();
        Some((
            ConstrainedBox::new(card)
                .with_max_width(TOAST_MAX_WIDTH)
                .finish(),
            timeline.entered(now),
        ))
    }
}
