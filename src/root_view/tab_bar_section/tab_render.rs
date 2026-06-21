// tab_bar_section::tab_render — RootView 单个 tab 渲染（hover/拖拽/关闭按钮/重命名编辑/tooltip）。
// 本文件只含 impl RootView，无自由函数。被 title_bar::render_tab_bar_contents 调用。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。

use std::time::Duration;

use pathfinder_geometry::vector::vec2f;

use crate::terminal_grid_element::TerminalGridAction;
use crate::terminal_view_helpers::close_button_element;
use crate::{
    RootView, TabModel, ICON_PATH_TERMINAL, TAB_CLOSE_BUTTON_HORIZONTAL_INSET,
    TAB_CLOSE_BUTTON_WIDTH, TAB_CONTENT_HORIZONTAL_PADDING, TAB_VERTICAL_PADDING, UI_FONT_SIZE,
    WARP_2_HOVERED_TAB_COLOR_OPACITY, WARP_2_TAB_COLOR_OPACITY,
};
use nexshell::warp_horizontal_tabs::{
    TabWidthConstraint, COMPACT_TAB_WIDTH_THRESHOLD, TAB_INDICATOR_HEIGHT,
};
use nexshell::warp_tab_context_menu::{
    tab_rename_editor_top_margin, TabContextMenuAnchor, TAB_COLOR_ICON_PATH,
};
use warp::appearance::Appearance;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::AnsiColorIdentifier;
use warpui::color::ColorU;
use warpui::elements::{
    Align, Border, ChildAnchor, Clipped, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DragAxis, Draggable, DraggableState, Fill, Flex, Hoverable, Icon,
    MainAxisAlignment, MainAxisSize, OffsetPositioning, Padding, ParentAnchor, ParentElement,
    ParentOffsetBounds, PositionedElementAnchor, PositionedElementOffsetBounds, Radius,
    SavePosition, Shrinkable, SizeConstraintCondition, SizeConstraintSwitch, Stack, Text,
};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{AppContext, Element, SingletonEntity};

impl RootView {
    pub(in crate::root_view) fn render_tab(&self, tab: &TabModel, index: usize, app: &AppContext) -> Box<dyn Element> {
        let uc = self.ui_colors();
        let is_active = tab.active;
        let is_settings = tab.is_settings;
        // 编辑器标签前缀：远程保存中 …（ADR 0005），有未保存改动 ●（ADR 0003）；tooltip 仍用原始 label。
        let label = if is_settings {
            tab.label.clone()
        } else {
            match self.terminal_tabs.get(index) {
                Some(t) if t.code_viewer_saving => format!("… {}", tab.label),
                Some(t) if t.code_viewer_dirty => format!("● {}", tab.label),
                _ => tab.label.clone(),
            }
        };
        let tooltip_label = tab.label.clone();
        let label_color = if is_active {
            uc.tab_text_active
        } else {
            uc.tab_text_inactive
        };
        let ui_font = self.ui_font;
        let (
            tab_state,
            tooltip_state,
            close_state_for_render,
            close_state_for_mouse_down,
            draggable_state,
        ) = if is_settings {
            (
                self.settings_tab_state.clone(),
                self.settings_tab_state.clone(),
                self.settings_tab_close_state.clone(),
                self.settings_tab_close_state.clone(),
                DraggableState::default(),
            )
        } else {
            (
                self.tab_states[index].clone(),
                self.tab_tooltip_states[index].clone(),
                self.tab_close_states[index].clone(),
                self.tab_close_states[index].clone(),
                self.tab_draggable_states[index].clone(),
            )
        };
        let tab_drag_in_progress = self.tab_drag_in_progress;
        let hover_fixed_width = if is_settings {
            None
        } else {
            self.tab_fixed_width
        };
        let is_tab_being_renamed = !is_settings && self.tab_being_renamed == Some(index);
        let tab_rename_editor = self.tab_rename_editor.clone();
        let tab_rename_editor_margin_top = tab_rename_editor_top_margin(true);
        let selected_tab_background = if is_settings {
            None
        } else {
            self.tab_selected_colors
                .get(index)
                .copied()
                .flatten()
                .map(|color| {
                    let terminal_colors = Appearance::as_ref(app).theme().terminal_colors().normal;
                    ColorU::from(color.to_ansi_color(&terminal_colors))
                })
        };

        // 录制中红点：相位由 idle tick 每 500ms 翻转；不可见相位画透明占位防文字抖动。
        let recording_red = (!is_settings
            && self.terminal_tabs.get(index).is_some_and(|t| t.is_recording()))
        .then(|| {
            let terminal_colors = Appearance::as_ref(app).theme().terminal_colors().normal;
            ColorU::from(AnsiColorIdentifier::Red.to_ansi_color(&terminal_colors))
        });
        let recording_phase_on = self.recording_blink.phase_visible;

        // 离线红点（远程/串口断开）：常亮，区别于录制红点的闪烁。
        let offline_red = (!is_settings
            && self
                .terminal_tabs
                .get(index)
                .is_some_and(|t| t.is_disconnected()))
        .then(|| {
            let terminal_colors = Appearance::as_ref(app).theme().terminal_colors().normal;
            ColorU::from(AnsiColorIdentifier::Red.to_ansi_color(&terminal_colors))
        });

        // warp tab.rs:1458 — SavePosition ID for the draggable tab.
        let tab_position_id = format!("nexshell_tab_position_{index}");
        let tab_position_id_for_close = tab_position_id.clone();
        // warp tab.rs:922 — tooltip 定位锚点 ID
        let tab_text_position_id = format!("nexshell_tab_text_{index}");
        let tab_text_position_id_for_tooltip = tab_text_position_id.clone();

        let tab_bg_active = uc.tab_bg_active;
        let tab_bg_hover = uc.tab_bg_hover;
        let title_bar_bg = uc.title_bar_bg;
        let tab_border_active = uc.tab_border_active;
        let tab_border_inactive = uc.tab_border_inactive;
        let close_bg_default = uc.tab_close_bg_default;
        let close_bg_hover_color = uc.tab_close_bg_hover;
        let close_icon_active = uc.icon_color_active;
        let mut hover_tab = Hoverable::new(tab_state, move |hover| {
            let is_hovered = hover.is_hovered();
            let bg_color = if let Some(color) = selected_tab_background {
                let opacity = if is_active || is_hovered {
                    WARP_2_HOVERED_TAB_COLOR_OPACITY
                } else {
                    WARP_2_TAB_COLOR_OPACITY
                };
                coloru_with_opacity(color, opacity)
            } else if is_active {
                tab_bg_active
            } else if is_hovered {
                tab_bg_hover
            } else {
                title_bar_bg
            };
            let border_color = if is_active {
                tab_border_active
            } else {
                tab_border_inactive
            };

            // warp tab.rs:1272-1275 — SavePosition 包裹文字用于 tooltip 定位
            let tab_content = if is_tab_being_renamed {
                Align::new(
                    TextInput::new(
                        tab_rename_editor.clone(),
                        UiComponentStyles::default()
                            .set_background(Fill::None)
                            .set_border_radius(CornerRadius::with_all(Radius::Pixels(0.)))
                            .set_border_width(0.),
                    )
                    .with_style(UiComponentStyles {
                        margin: Some(Coords::default().top(tab_rename_editor_margin_top)),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
                )
                .finish()
            } else {
                Text::new_inline(label.clone(), ui_font, UI_FONT_SIZE)
                    .with_color(label_color)
                    .finish()
            };
            let saved_text = SavePosition::new(tab_content, &tab_text_position_id).finish();

            let mut content_row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if let Some(red) = offline_red {
                content_row = content_row.with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(TAB_COLOR_ICON_PATH, red).finish())
                            .with_max_width(8.0)
                            .with_max_height(8.0)
                            .finish(),
                    )
                    .with_horizontal_padding(3.0)
                    .finish(),
                );
            }
            if let Some(red) = recording_red {
                let dot_color = if recording_phase_on {
                    red
                } else {
                    coloru_with_opacity(red, 0)
                };
                content_row = content_row.with_child(
                    Container::new(
                        ConstrainedBox::new(Icon::new(TAB_COLOR_ICON_PATH, dot_color).finish())
                            .with_max_width(8.0)
                            .with_max_height(8.0)
                            .finish(),
                    )
                    .with_horizontal_padding(3.0)
                    .finish(),
                );
            }
            let full_tab_content = Container::new(
                content_row
                    .with_child(Shrinkable::new(1.0, saved_text).finish())
                    .finish(),
            )
            .with_horizontal_padding(TAB_CONTENT_HORIZONTAL_PADDING)
            .finish();

            // warp tab.rs:1295-1313 — compact tab switches to icon-only
            // below COMPACT_TAB_WIDTH_THRESHOLD.
            // 紧凑模式没地方放红点，改为按相位把终端图标染红。
            let compact_icon_color = if let Some(red) = offline_red {
                red
            } else {
                match recording_red {
                    Some(red) if recording_phase_on => red,
                    _ => label_color,
                }
            };
            let compact_icon =
                ConstrainedBox::new(Icon::new(ICON_PATH_TERMINAL, compact_icon_color).finish())
                    .with_max_width(TAB_INDICATOR_HEIGHT)
                    .with_max_height(TAB_INDICATOR_HEIGHT)
                    .finish();
            let compact_tab_content = Clipped::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(compact_icon)
                    .finish(),
            )
            .finish();

            // warp tab.rs:1371-1407 — close button is overlaid on full
            // tabs, and only on the active tab in compact mode.
            let close_action = if is_settings {
                TerminalGridAction::CloseSettingsTab
            } else {
                TerminalGridAction::CloseTab(index)
            };
            let build_close_button_overlay = |is_hovered: bool| {
                Container::new(
                    ConstrainedBox::new(close_button_element(
                        close_state_for_render.clone(),
                        index,
                        tab_position_id_for_close.clone(),
                        is_hovered,
                        close_action.clone(),
                        close_bg_default,
                        close_bg_hover_color,
                        close_icon_active,
                    ))
                    .with_width(TAB_CLOSE_BUTTON_WIDTH)
                    .with_height(TAB_CLOSE_BUTTON_WIDTH)
                    .finish(),
                )
                .finish()
            };

            let mut full_stack = Stack::new().with_child(full_tab_content);
            full_stack.add_positioned_child(
                build_close_button_overlay(is_hovered),
                OffsetPositioning::offset_from_parent(
                    vec2f(-(TAB_CLOSE_BUTTON_HORIZONTAL_INSET + 4.0), 0.0),
                    ParentOffsetBounds::ParentByPosition,
                    ParentAnchor::MiddleRight,
                    ChildAnchor::MiddleRight,
                ),
            );

            let mut compact_stack = Stack::new().with_child(compact_tab_content);
            if is_active {
                compact_stack.add_positioned_child(
                    build_close_button_overlay(is_hovered),
                    OffsetPositioning::offset_from_parent(
                        vec2f(-(TAB_CLOSE_BUTTON_HORIZONTAL_INSET + 4.0), 0.0),
                        ParentOffsetBounds::ParentByPosition,
                        ParentAnchor::MiddleRight,
                        ChildAnchor::MiddleRight,
                    ),
                );
            }

            let stack = SizeConstraintSwitch::new(
                full_stack.finish(),
                vec![(
                    SizeConstraintCondition::WidthLessThan(COMPACT_TAB_WIDTH_THRESHOLD),
                    compact_stack.finish(),
                )],
            )
            .finish();

            Container::new(stack)
                .with_vertical_padding(TAB_VERTICAL_PADDING)
                .with_background_color(bg_color)
                .with_border(
                    Border::new(1.0)
                        .with_sides(false, index == 0, false, true)
                        .with_border_color(border_color),
                )
                .finish()
        })
        .on_middle_click(move |ctx, _, _| {
            if is_settings {
                ctx.dispatch_typed_action(TerminalGridAction::CloseSettingsTab);
            } else {
                ctx.dispatch_typed_action(TerminalGridAction::CloseTab(index));
            }
        });
        if !is_settings {
            hover_tab = hover_tab.on_right_click(move |ctx, _, position| {
                ctx.dispatch_typed_action(TerminalGridAction::ToggleTabRightClickMenu {
                    tab_index: index,
                    anchor: TabContextMenuAnchor::Pointer(position),
                });
            });
        }

        if !is_tab_being_renamed {
            hover_tab = hover_tab.on_mouse_down(move |ctx, _, _| {
                let close_hovered = close_state_for_mouse_down
                    .lock()
                    .map(|s| s.is_hovered())
                    .unwrap_or(false);
                if !close_hovered {
                    if is_settings {
                        ctx.dispatch_typed_action(TerminalGridAction::ShowSettings);
                    } else {
                        ctx.dispatch_typed_action(TerminalGridAction::SelectTab(index));
                    }
                }
            });
            if !is_settings {
                hover_tab = hover_tab.on_double_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::RenameTab(index));
                });
            }
        }

        let tooltip_text = uc.tooltip_text;
        let tooltip_bg = uc.tooltip_bg;
        let tab_with_tooltip = Hoverable::new(tooltip_state, move |tt_state| {
            let base = hover_tab.finish();
            if tt_state.is_hovered() && !is_tab_being_renamed && !tab_drag_in_progress {
                let tooltip = Container::new(
                    Text::new(tooltip_label.clone(), ui_font, UI_FONT_SIZE)
                        .with_color(tooltip_text)
                        .finish(),
                )
                .with_background_color(tooltip_bg)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .with_padding(Padding::uniform(6.))
                .finish();

                let mut stack = Stack::new().with_child(base);
                // warp tab.rs:1594-1603
                stack.add_positioned_overlay_child(
                    tooltip,
                    OffsetPositioning::offset_from_save_position_element(
                        tab_text_position_id_for_tooltip.clone(),
                        vec2f(0., 8.),
                        PositionedElementOffsetBounds::WindowByPosition,
                        PositionedElementAnchor::BottomLeft,
                        ChildAnchor::TopLeft,
                    ),
                );
                return stack.finish();
            }
            base
        })
        .with_hover_in_delay(Duration::from_millis(500));

        let constrained_tab = match TabWidthConstraint::from_hover_width(hover_fixed_width) {
            // warp tab.rs:1647-1656 — fixed width while close hover is
            // active; otherwise each tab can shrink from max width 200.
            TabWidthConstraint::Fixed(width) => ConstrainedBox::new(tab_with_tooltip.finish())
                .with_width(width)
                .finish(),
            TabWidthConstraint::Max(width) => ConstrainedBox::new(tab_with_tooltip.finish())
                .with_max_width(width)
                .finish(),
        };

        let tab_with_drag = Draggable::new(draggable_state, constrained_tab)
            .on_drag_start(|ctx, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::StartTabDrag);
            })
            .on_drag(move |ctx, _, tab_position, _| {
                ctx.dispatch_typed_action(TerminalGridAction::DragTab {
                    tab_index: index,
                    tab_position,
                });
            })
            .on_drop(|ctx, _, _, _| {
                ctx.dispatch_typed_action(TerminalGridAction::DropTab);
            })
            .with_drag_axis(DragAxis::HorizontalOnly)
            .finish();
        // warp tab.rs:1673-1678 — save the draggable tab position and
        // make the whole tab participate in flex shrinking.
        let full_tab = SavePosition::new(tab_with_drag, &tab_position_id).finish();
        Shrinkable::new(1.0, full_tab).finish()
    }
}
