// tab_bar_section::title_bar — RootView 标题栏 chrome 渲染：tab bar 容器 + 侧栏/文件/git/设置按钮 +
// 新建标签复合按钮 + Windows 窗口控制 + host tab + 下拉菜单委托。
// 本文件只含 impl RootView，无自由函数。单个 tab 渲染见 tab_render.rs。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。

use crate::terminal_grid_element::TerminalGridAction;
use crate::title_bar_chrome::{
    current_title_bar_chrome_platform, render_windows_window_control_icon, title_bar_chrome_layout,
    TitleBarChromePlatform, WindowControlKind,
};
use crate::{
    AppPage, RootView, TabBarDropTargetData, TabBarLocation, TabModel,
    FILE_PANEL_BUTTON_POSITION_ID, GIT_PANEL_BUTTON_POSITION_ID, ICON_BUTTON_PADDING,
    ICON_BUTTON_SIZE, ICON_PATH_CHEVRON_DOWN, ICON_PATH_FOLDER, ICON_PATH_GEAR,
    ICON_PATH_GIT_BRANCH, ICON_PATH_HOME, ICON_PATH_PLUS, ICON_PATH_SIDEBAR_OPEN,
    NEW_TAB_BUTTON_HEIGHT, NEW_TAB_BUTTON_LEFT_MARGIN, NEW_TAB_BUTTON_POSITION_ID,
    NEW_TAB_CHEVRON_WIDTH, NEW_TAB_PLUS_WIDTH, SETTINGS_BUTTON_POSITION_ID,
    SIDEBAR_TOGGLE_MARGIN_RIGHT, TAB_BAR_PADDING_LEFT, TAB_BAR_POSITION_ID,
    TAB_CONTENT_HORIZONTAL_PADDING, TITLE_BAR_BORDER_HEIGHT, TITLE_BAR_HEIGHT,
    WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH,
};
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, ChildAnchor, Clipped, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, DropTarget, Empty, EventHandler, Flex, Hoverable,
    Icon, MainAxisSize, MouseStateHandle, OffsetPositioning, ParentAnchor, ParentElement,
    ParentOffsetBounds, Radius, Rect, SavePosition, Shrinkable, Stack,
};
use warpui::geometry::vector::vec2f;
use warpui::{platform, AppContext, Element};

use std::time::Instant;

// 标题栏图标按钮 hover 过渡 key（titlebar_btn_transitions）。
const BTN_SIDEBAR: usize = 0;
const BTN_FILE: usize = 1;
const BTN_GIT: usize = 2;
const BTN_SETTINGS: usize = 3;

impl RootView {
    /// 图标按钮背景 eased 过渡：on(hover||active) → hover_bg，否则回落 base_bg。
    fn titlebar_btn_bg(&self, key: usize, on: bool, hover_bg: ColorU, base_bg: ColorU) -> ColorU {
        let now = Instant::now();
        let target = if on { hover_bg } else { base_bg };
        let mut m = self.titlebar_btn_transitions.borrow_mut();
        m.retarget(key, target, now);
        m.sample(&key, now).unwrap_or(target)
    }

    pub(in crate::root_view) fn render_new_session_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.new_session_menu).finish()
    }

    pub(in crate::root_view) fn render_settings_menu(&self) -> Box<dyn Element> {
        warpui::elements::ChildView::new(&self.settings_menu).finish()
    }

    pub(in crate::root_view) fn render_title_bar(
        &self,
        tabs: &[TabModel],
        app: &AppContext,
    ) -> Box<dyn Element> {
        let bar_contents = ConstrainedBox::new(
            // warp/app/src/workspace/view.rs:17539-17549:
            // whole tab bar is a drop target after the last tab.
            DropTarget::new(
                self.render_tab_bar_contents(tabs, app),
                TabBarDropTargetData {
                    tab_bar_location: TabBarLocation::AfterTabIndex(tabs.len()),
                },
            )
            .finish(),
        )
        .with_height(TITLE_BAR_HEIGHT)
        .finish();

        let tab_bar_container = Container::new(
            // warp/app/src/workspace/view.rs:17555-17566:
            // EventHandler wraps Clipped(render_tab_bar_hoverable(contents)).
            EventHandler::new(Clipped::new(self.render_tab_bar_hoverable(bar_contents)).finish())
                .on_back_mouse_down(move |ctx, _app, _position| {
                    ctx.dispatch_typed_action(TerminalGridAction::ActivatePrevTab);
                    DispatchEventResult::StopPropagation
                })
                .on_forward_mouse_down(move |ctx, _app, _position| {
                    ctx.dispatch_typed_action(TerminalGridAction::ActivateNextTab);
                    DispatchEventResult::StopPropagation
                })
                .finish(),
        )
        .with_background_color(self.ui_colors().title_bar_bg)
        .with_border(
            Border::bottom(TITLE_BAR_BORDER_HEIGHT)
                .with_border_color(self.ui_colors().title_bar_border),
        )
        .finish();

        // warp/app/src/workspace/view.rs:17574-17584 saves TAB_BAR_POSITION_ID.
        SavePosition::new(tab_bar_container, TAB_BAR_POSITION_ID).finish()
    }

    fn render_tab_bar_hoverable(&self, content: Box<dyn Element>) -> Box<dyn Element> {
        // warp/app/src/workspace/view.rs:16946-16953.
        Hoverable::new(self.tab_bar_hover_state.clone(), |_| content).finish()
    }

    fn render_tab_bar_contents(&self, tabs: &[TabModel], app: &AppContext) -> Box<dyn Element> {
        let chrome_layout = title_bar_chrome_layout(current_title_bar_chrome_platform());
        // Stretch 让 tab 背景色撑满 bar 高度; icon buttons 用 Align 居中。
        let sidebar_button = Container::new(Align::new(self.render_sidebar_toggle()).finish())
            .with_margin_right(SIDEBAR_TOGGLE_MARGIN_RIGHT)
            .finish();
        let new_tab_button = Align::new(self.render_new_tab_button()).finish();
        let file_panel_button =
            Container::new(Align::new(self.render_file_panel_button()).finish())
                .with_margin_left(TAB_BAR_PADDING_LEFT)
                .finish();
        let git_panel_button = Container::new(Align::new(self.render_git_panel_button()).finish())
            .with_margin_left(TAB_BAR_PADDING_LEFT)
            .finish();
        let settings_button = Container::new(Align::new(self.render_settings_button()).finish())
            .with_margin_left(TAB_BAR_PADDING_LEFT)
            .finish();

        let host_tab = self.render_host_tab();

        let mut bar_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        bar_row.add_child(sidebar_button);
        bar_row.add_child(host_tab);

        for (index, tab) in tabs.iter().enumerate() {
            bar_row.add_child(self.render_tab(tab, index, app));
        }
        // 清理已关闭 tab 的过渡条目，防泄漏。
        let tab_count = tabs.len();
        self.tab_hover_transitions
            .borrow_mut()
            .retain(|k| *k < tab_count);
        bar_row.add_child(new_tab_button);
        bar_row
            .add_child(Shrinkable::new(0.5, Align::new(Empty::new().finish()).finish()).finish());
        // warp view.rs:17462-17465 settings button 在 shrinkable 之后。
        bar_row.add_child(file_panel_button);
        bar_row.add_child(git_panel_button);
        bar_row.add_child(settings_button);
        if chrome_layout.windows_controls_width > 0.0 {
            bar_row.add_child(
                Container::new(self.render_windows_window_controls(app))
                    .with_margin_left(TAB_BAR_PADDING_LEFT)
                    .finish(),
            );
        }

        let content = EventHandler::new(
            Container::new(bar_row.finish())
                .with_padding_left(chrome_layout.left_padding)
                .with_padding_right(chrome_layout.right_padding)
                .finish(),
        )
        .finish();
        // accent 底条 overlay：两根弹簧追活动 tab 上一帧实测位置，切 tab 时滑动过去。
        // 坐标基准复用外层已存的 TAB_BAR_POSITION_ID（其水平原点与本 Stack 一致）。
        let mut stack = Stack::new().with_child(content);
        if let Some((bar_x, bar_y, bar_w)) = self.tab_accent_bar_layout(tabs, app) {
            if bar_w > 1.0 {
                stack.add_positioned_overlay_child(
                    ConstrainedBox::new(
                        Rect::new()
                            .with_background_color(self.ui_colors().tab_accent_bar)
                            .finish(),
                    )
                    .with_width(bar_w)
                    .with_height(2.0)
                    .finish(),
                    OffsetPositioning::offset_from_parent(
                        vec2f(bar_x, bar_y),
                        ParentOffsetBounds::Unbounded,
                        ParentAnchor::TopLeft,
                        ChildAnchor::TopLeft,
                    ),
                );
            }
        }
        stack.finish()
    }

    /// accent 底条弹簧布局：返回（相对 tab 行的 x, y, 宽）。位置取上一帧缓存；
    /// y 直接钉在活动 tab 自己的底边（对容器高度差异免疫）。
    /// 缺位帧（新 tab 首帧无缓存）沿用当前弹簧值继续画，避免闪断。
    fn tab_accent_bar_layout(
        &self,
        tabs: &[TabModel],
        app: &AppContext,
    ) -> Option<(f32, f32, f32)> {
        let active = tabs.iter().position(|t| t.active)?;
        let now = Instant::now();
        let rects = app
            .element_position_by_id_at_last_frame(self.window_id, TAB_BAR_POSITION_ID)
            .zip(app.element_position_by_id_at_last_frame(
                self.window_id,
                format!("nexshell_tab_position_{active}"),
            ));
        let mut x = self.tab_accent_bar_x.borrow_mut();
        let mut w = self.tab_accent_bar_w.borrow_mut();
        if let Some((bar, tab)) = rects {
            let (tx, tw) = (tab.min_x() - bar.min_x(), tab.width());
            // NEXSHELL_BAR_DEBUG=1 时打印弹簧轨迹（定位滑动异常用，临时观测件）。
            if std::env::var_os("NEXSHELL_BAR_DEBUG").is_some() {
                eprintln!(
                    "[bar] active={active} bar_min_x={:.1} tab=({:.1},{:.1} w{:.1}) target_x={tx:.1} target_w={tw:.1}",
                    bar.min_x(),
                    tab.min_x(),
                    tab.min_y(),
                    tab.width(),
                );
            }
            self.tab_accent_bar_y.set(tab.max_y() - bar.min_y() - 2.0);
            if self.tab_accent_bar_init.get() {
                x.set_target(tx);
                w.set_target(tw);
            } else {
                // 首次出现：直接落位不滑动。
                x.snap(tx);
                w.snap(tw);
                self.tab_accent_bar_init.set(true);
            }
        } else if !self.tab_accent_bar_init.get() {
            return None;
        }
        // 取整到整像素：2px 细条在小数坐标发虚，收敛尾部亚像素位移显成抖动。
        let (sx, sw) = (x.sample(now).round(), w.sample(now).round());
        if std::env::var_os("NEXSHELL_BAR_DEBUG").is_some()
            && (x.is_animating() || w.is_animating())
        {
            eprintln!("[bar] sample x={sx:.1} w={sw:.1}");
        }
        Some((sx, self.tab_accent_bar_y.get(), sw))
    }

    fn render_sidebar_toggle(&self) -> Box<dyn Element> {
        let icon_path = ICON_PATH_SIDEBAR_OPEN;
        let is_active = self.sidebar_open;
        let state = self.sidebar_button_state.clone();
        let uc = self.ui_colors();
        let icon_active = uc.icon_color_active;
        let icon_inactive = uc.icon_color_inactive;
        let hover_bg = uc.icon_button_hover_bg;
        let is_hovered_now = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
        let btn_bg = self.titlebar_btn_bg(BTN_SIDEBAR, is_hovered_now, hover_bg, uc.title_bar_bg);

        Hoverable::new(state, move |_mouse| {
            let icon_color = if is_active {
                icon_active
            } else {
                icon_inactive
            };
            let icon = ConstrainedBox::new(Icon::new(icon_path, icon_color).finish())
                .with_width(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .with_height(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .finish();

            Container::new(
                ConstrainedBox::new(Align::new(icon).finish())
                    .with_width(ICON_BUTTON_SIZE)
                    .with_height(ICON_BUTTON_SIZE)
                    .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(btn_bg)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_hover(|_, ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
        })
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleSidebar);
        })
        .finish()
    }

    /// 文件面板开关按钮（settings 左侧）。
    /// active 态判定来自当前 tab 的 file_panel_open，跟 host_overview sidebar 一致。
    fn render_file_panel_button(&self) -> Box<dyn Element> {
        // 孤儿查看伪 tab（源终端已关）：与 git 面板按钮一致隐藏，避免点出破损空面板。
        if self.source_terminal_tab_index().is_none() {
            return Empty::new().finish();
        }
        let state = self.file_panel_button_state.clone();
        let is_open = self
            .file_panel_tab()
            .map(|tab| tab.file_panel_open)
            .unwrap_or(false);
        let uc = self.ui_colors();
        let icon_active = uc.icon_color_active;
        let icon_inactive = uc.icon_color_inactive;
        let hover_bg = uc.icon_button_hover_bg;
        let is_hovered_now = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
        let btn_bg = self.titlebar_btn_bg(
            BTN_FILE,
            is_hovered_now || is_open,
            hover_bg,
            uc.title_bar_bg,
        );

        let button = Hoverable::new(state, move |_mouse| {
            let icon_color = if is_open { icon_active } else { icon_inactive };
            let icon = ConstrainedBox::new(Icon::new(ICON_PATH_FOLDER, icon_color).finish())
                .with_width(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .with_height(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .finish();
            Container::new(
                ConstrainedBox::new(Align::new(icon).finish())
                    .with_width(ICON_BUTTON_SIZE)
                    .with_height(ICON_BUTTON_SIZE)
                    .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(btn_bg)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_hover(|_, ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
        })
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleFilePanel);
        })
        .finish();

        SavePosition::new(Align::new(button).finish(), FILE_PANEL_BUTTON_POSITION_ID).finish()
    }

    /// git 面板开关按钮（settings 左侧、file_panel 右侧）。active 态来自 ShellModel 共用开关。
    /// 非 Local tab 不显示按钮，避免误开后看到 "不支持" 占位。
    fn render_git_panel_button(&self) -> Box<dyn Element> {
        if !self.active_tab_supports_git_panel() {
            return Empty::new().finish();
        }
        let state = self.git_panel_button_state.clone();
        let is_open = self.active_git_panel_open();
        let uc = self.ui_colors();
        let icon_active = uc.icon_color_active;
        let icon_inactive = uc.icon_color_inactive;
        let hover_bg = uc.icon_button_hover_bg;
        let is_hovered_now = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
        let btn_bg = self.titlebar_btn_bg(
            BTN_GIT,
            is_hovered_now || is_open,
            hover_bg,
            uc.title_bar_bg,
        );

        let button = Hoverable::new(state, move |_mouse| {
            let icon_color = if is_open { icon_active } else { icon_inactive };
            let icon = ConstrainedBox::new(Icon::new(ICON_PATH_GIT_BRANCH, icon_color).finish())
                .with_width(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .with_height(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .finish();
            Container::new(
                ConstrainedBox::new(Align::new(icon).finish())
                    .with_width(ICON_BUTTON_SIZE)
                    .with_height(ICON_BUTTON_SIZE)
                    .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(btn_bg)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_hover(|_, ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
        })
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleGitPanel);
        })
        .finish();

        SavePosition::new(Align::new(button).finish(), GIT_PANEL_BUTTON_POSITION_ID).finish()
    }

    fn render_settings_button(&self) -> Box<dyn Element> {
        let state = self.settings_button_state.clone();
        let is_open = self.settings_menu_open;
        let uc = self.ui_colors();
        let icon_active = uc.icon_color_active;
        let icon_inactive = uc.icon_color_inactive;
        let hover_bg = uc.icon_button_hover_bg;
        let is_hovered_now = state.lock().map(|s| s.is_hovered()).unwrap_or(false);
        let btn_bg = self.titlebar_btn_bg(
            BTN_SETTINGS,
            is_hovered_now || is_open,
            hover_bg,
            uc.title_bar_bg,
        );

        let button = Hoverable::new(state, move |_mouse| {
            let icon_color = if is_open { icon_active } else { icon_inactive };
            let icon = ConstrainedBox::new(Icon::new(ICON_PATH_GEAR, icon_color).finish())
                .with_width(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .with_height(ICON_BUTTON_SIZE - ICON_BUTTON_PADDING * 2.0)
                .finish();

            Container::new(
                ConstrainedBox::new(Align::new(icon).finish())
                    .with_width(ICON_BUTTON_SIZE)
                    .with_height(ICON_BUTTON_SIZE)
                    .finish(),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .with_background_color(btn_bg)
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_hover(|_, ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::WakeUiAnim);
        })
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ToggleSettingsMenu);
        })
        .finish();

        SavePosition::new(Align::new(button).finish(), SETTINGS_BUTTON_POSITION_ID).finish()
    }

    fn render_windows_window_controls(&self, app: &AppContext) -> Box<dyn Element> {
        let fullscreen_state = app
            .windows()
            .platform_window(self.window_id)
            .map(|window| window.fullscreen_state())
            .unwrap_or_default();
        let maximize_kind = if fullscreen_state == platform::FullscreenState::Normal {
            WindowControlKind::Maximize
        } else {
            WindowControlKind::Restore
        };

        let controls = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_windows_window_control_button(
                WindowControlKind::Minimize,
                self.window_control_minimize_state.clone(),
            ))
            .with_child(self.render_windows_window_control_button(
                maximize_kind,
                self.window_control_maximize_state.clone(),
            ))
            .with_child(self.render_windows_window_control_button(
                WindowControlKind::Close,
                self.window_control_close_state.clone(),
            ))
            .finish();

        ConstrainedBox::new(controls)
            .with_width(
                title_bar_chrome_layout(TitleBarChromePlatform::Windows).windows_controls_width,
            )
            .with_height(TITLE_BAR_HEIGHT)
            .finish()
    }

    fn render_windows_window_control_button(
        &self,
        kind: WindowControlKind,
        state: MouseStateHandle,
    ) -> Box<dyn Element> {
        let uc = self.ui_colors();
        let icon_color = uc.icon_color_active;
        let hover_bg = if kind == WindowControlKind::Close {
            self.design_tokens.semantic.windows_close_hover
        } else {
            uc.icon_button_hover_bg
        };
        let hover_icon_color = if kind == WindowControlKind::Close {
            ColorU::new(255, 255, 255, 0xff)
        } else {
            icon_color
        };

        let action = match kind {
            WindowControlKind::Minimize => TerminalGridAction::WindowMinimize,
            WindowControlKind::Maximize | WindowControlKind::Restore => {
                TerminalGridAction::WindowToggleMaximize
            }
            WindowControlKind::Close => TerminalGridAction::WindowClose,
        };

        Hoverable::new(state, move |mouse| {
            let color = if mouse.is_hovered() {
                hover_icon_color
            } else {
                icon_color
            };
            let icon = render_windows_window_control_icon(kind, color);
            let child = ConstrainedBox::new(Align::new(icon).finish())
                .with_width(WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH)
                .with_height(TITLE_BAR_HEIGHT)
                .finish();
            let mut container = Container::new(child);
            if mouse.is_hovered() {
                container = container.with_background_color(hover_bg);
            }
            container.finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    // warp/app/src/workspace/view.rs:17617-17758 render_new_session_button:
    // 复合按钮 = [Plus button] + [ChevronDown menu button],
    // 整体外层圆角 4，左半 with_left 圆角，右半 with_right 圆角；
    // 外层 hover 时整体提亮 (neutral_1)，内半 hover 时各自再提亮 (neutral_2)。
    fn render_new_tab_button(&self) -> Box<dyn Element> {
        let combo_state = self.new_tab_combo_state.clone();
        let plus_state = self.new_tab_plus_state.clone();
        let chevron_state = self.new_tab_chevron_state.clone();
        let menu_open = self.new_session_menu_open;
        let uc = self.ui_colors();
        let combo_outer = uc.combo_outer_hover_bg;
        let combo_inner = uc.combo_inner_hover_bg;
        let combo_chevron = uc.combo_chevron_active_bg;
        let bar_bg = uc.title_bar_bg;
        let icon_active = uc.icon_color_active;

        let button = Hoverable::new(combo_state, move |mouse| {
            let outer_bg = if mouse.is_hovered() {
                combo_outer
            } else {
                bar_bg
            };

            let plus_button = Hoverable::new(plus_state.clone(), |mouse| {
                let bg = if mouse.is_hovered() {
                    combo_inner
                } else {
                    bar_bg
                };
                let icon = ConstrainedBox::new(Icon::new(ICON_PATH_PLUS, icon_active).finish())
                    .with_width(18.0)
                    .with_height(18.0)
                    .finish();
                Container::new(
                    ConstrainedBox::new(Align::new(icon).finish())
                        .with_width(NEW_TAB_PLUS_WIDTH)
                        .with_height(NEW_TAB_BUTTON_HEIGHT)
                        .finish(),
                )
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_left(Radius::Pixels(4.0)))
                .finish()
            })
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::NewTab);
            })
            .finish();

            let chevron_button = Hoverable::new(chevron_state.clone(), move |mouse| {
                let bg = if menu_open {
                    combo_chevron
                } else if mouse.is_hovered() {
                    combo_inner
                } else {
                    bar_bg
                };
                let icon =
                    ConstrainedBox::new(Icon::new(ICON_PATH_CHEVRON_DOWN, icon_active).finish())
                        .with_width(12.0)
                        .with_height(12.0)
                        .finish();
                Container::new(
                    ConstrainedBox::new(Align::new(icon).finish())
                        .with_width(NEW_TAB_CHEVRON_WIDTH)
                        .with_height(NEW_TAB_BUTTON_HEIGHT)
                        .finish(),
                )
                .with_background_color(bg)
                .with_corner_radius(CornerRadius::with_right(Radius::Pixels(4.0)))
                .finish()
            })
            .with_cursor(warpui::platform::Cursor::PointingHand)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::ToggleNewSessionMenu);
            })
            .finish();

            let inner_row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(plus_button)
                .with_child(chevron_button)
                .finish();

            Container::new(
                ConstrainedBox::new(inner_row)
                    .with_width(NEW_TAB_PLUS_WIDTH + NEW_TAB_CHEVRON_WIDTH)
                    .with_height(NEW_TAB_BUTTON_HEIGHT)
                    .finish(),
            )
            .with_margin_left(NEW_TAB_BUTTON_LEFT_MARGIN)
            .with_background_color(outer_bg)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
            .finish()
        })
        .finish();

        SavePosition::new(button, NEW_TAB_BUTTON_POSITION_ID).finish()
    }

    fn render_host_tab(&self) -> Box<dyn Element> {
        let is_active = self.app_page == AppPage::HostManagement;
        let state = self.host_tab_state.clone();
        let uc = self.ui_colors();
        let tab_active = uc.tab_bg_active;
        let tab_hover = uc.tab_bg_hover;
        let bar_bg = uc.title_bar_bg;
        let border_active = uc.tab_border_active;
        let border_inactive = uc.tab_border_inactive;
        let ic_active = uc.icon_color_active;
        let ic_inactive = uc.icon_color_inactive;

        Hoverable::new(state, move |hover| {
            let bg = if is_active {
                tab_active
            } else if hover.is_hovered() {
                tab_hover
            } else {
                bar_bg
            };
            let border = if is_active {
                border_active
            } else {
                border_inactive
            };
            let icon_color = if is_active { ic_active } else { ic_inactive };

            let icon = Align::new(
                ConstrainedBox::new(Icon::new(ICON_PATH_HOME, icon_color).finish())
                    .with_width(16.0)
                    .with_height(16.0)
                    .finish(),
            )
            .finish();

            Container::new(
                ConstrainedBox::new(icon)
                    .with_width(ICON_BUTTON_SIZE + TAB_CONTENT_HORIZONTAL_PADDING * 2.0)
                    .with_height(TITLE_BAR_HEIGHT - TITLE_BAR_BORDER_HEIGHT)
                    .finish(),
            )
            .with_background_color(bg)
            .with_border(
                Border::bottom(if is_active { 2.0 } else { 0.0 }).with_border_color(border),
            )
            .finish()
        })
        .with_cursor(warpui::platform::Cursor::PointingHand)
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ShowHostManagement);
        })
        .finish()
    }
}
