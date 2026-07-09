// settings_section — 设置页 render 入口 + 设置项 action handler（主题/字体/光标/语言/帮助菜单）。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。
// 每个 handle_* 由 root_view/mod.rs handle_action match arm 单行分发；handle_action 按引用匹配，
// 故各 handler 取 owned 形参（String / Copy enum），调用方传 *choice / name.clone() 等。
// 设置页主体渲染在独立 crate::settings_view 模块，render_settings_page 仅薄委托。
// 横切/infra（save_ui_settings 持久化、set_terminal_font_size 字号、apply_window_opacity 透明度）
// 留 main.rs，本文件跨文件 self.xxx() / Self::xxx() 调用。

use crate::external_editor::EditorChoice;
use crate::settings_view;
use crate::terminal_grid_element::{
    CursorStyleChoice, GlassQualityChoice, LanguageChoice, NexSettingsSection, TerminalGridAction,
    TerminalShapedLineCache, ThemeChoice,
};
use crate::ui_settings::{
    resolve_locale, TERMINAL_LINE_HEIGHT_RATIO_DEFAULT, TERMINAL_LINE_HEIGHT_RATIO_MAX,
    TERMINAL_LINE_HEIGHT_RATIO_MIN,
};
use crate::{warp_dropdown_view, AppPage, RootView};
use nexshell::terminal_runtime::TerminalPalette;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::appearance::Appearance;
use warpui::{fonts, AppContext, Element, SingletonEntity, ViewContext};

impl RootView {
    pub(in crate::root_view) fn render_settings_page(&self, app: &AppContext) -> Box<dyn Element> {
        let state = self.settings_view_state.borrow();
        let fonts = if state.appearance_state.view_all_fonts {
            &self.available_all_fonts
        } else {
            &self.available_monospace_fonts
        };
        settings_view::render_settings_view(
            &state,
            self.current_theme,
            self.terminal_font_size,
            self.line_height_ratio,
            self.window_opacity,
            self.glass_quality,
            self.cursor_style,
            self.monospace_font_weight,
            &self.monospace_font_name,
            fonts,
            self.monospace_font,
            &self.font_family_dropdown,
            &self.font_weight_dropdown,
            &self.open_file_editor_dropdown,
            self.language,
            self.reuse_view_tab,
            app,
        )
    }

    pub(in crate::root_view) fn handle_toggle_settings_menu(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.settings_menu_open {
            self.settings_menu_open = false;
        } else {
            use nexshell::menu::{MenuItem, MenuItemFields};
            // warp/app/src/workspace/view.rs:8282-8413 user_menu_items
            let items = vec![
                MenuItemFields::new(rust_i18n::t!("menu_whats_new"))
                    .with_on_select_action(TerminalGridAction::SettingsMenuWhatsNew)
                    .into_item(),
                MenuItemFields::new(rust_i18n::t!("menu_settings"))
                    .with_on_select_action(TerminalGridAction::ShowSettings)
                    .into_item(),
                MenuItemFields::new(rust_i18n::t!("menu_keyboard_shortcuts"))
                    .with_on_select_action(TerminalGridAction::ShowSettingsKeybindings)
                    .into_item(),
                MenuItem::Separator,
                MenuItemFields::new(rust_i18n::t!("menu_documentation"))
                    .with_on_select_action(TerminalGridAction::SettingsMenuDocumentation)
                    .into_item(),
                MenuItemFields::new(rust_i18n::t!("menu_feedback"))
                    .with_on_select_action(TerminalGridAction::SettingsMenuFeedback)
                    .into_item(),
                MenuItemFields::new(rust_i18n::t!("menu_view_logs"))
                    .with_on_select_action(TerminalGridAction::SettingsMenuViewLogs)
                    .into_item(),
            ];
            let origin = ctx
                .element_position_by_id(crate::SETTINGS_BUTTON_POSITION_ID)
                .map(|rect| vec2f(rect.max_x(), rect.max_y() + 2.0));
            self.settings_menu.update(ctx, |menu, view_ctx| {
                menu.set_items(items, view_ctx);
                menu.set_origin(origin);
            });
            ctx.focus(&self.settings_menu);
            self.settings_menu_open = true;
            self.new_session_menu_open = false;
            self.show_terminal_context_menu = None;
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_show_settings_keybindings(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.set_app_page(AppPage::Settings, ctx);
        self.settings_tab_open = true;
        self.settings_view_state.borrow_mut().current_page = NexSettingsSection::Keybindings;
        self.settings_menu_open = false;
        ctx.notify();
    }

    // What's New / Documentation / Feedback / View Logs 暂仅关闭菜单（占位，后续接外链）。
    pub(in crate::root_view) fn handle_settings_menu_dismiss(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.settings_menu_open = false;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_show_settings(&mut self, ctx: &mut ViewContext<Self>) {
        self.set_app_page(AppPage::Settings, ctx);
        self.settings_tab_open = true;
        self.settings_menu_open = false;
        self.settings_prewarmed.set(true);
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_close_settings_tab(&mut self, ctx: &mut ViewContext<Self>) {
        self.settings_tab_open = false;
        if self.app_page == AppPage::Settings {
            self.set_app_page(AppPage::Terminal, ctx);
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_settings_select_page(
        &mut self,
        section: NexSettingsSection,
        ctx: &mut ViewContext<Self>,
    ) {
        self.settings_view_state.borrow_mut().current_page = section;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_theme(
        &mut self,
        choice: ThemeChoice,
        ctx: &mut ViewContext<Self>,
    ) {
        let theme_data = choice.to_warp_theme();
        self.cached_warp_theme = theme_data.clone();
        self.design_tokens =
            nexshell::design_tokens::DesignTokens::from_theme(&self.cached_warp_theme);
        let palette = TerminalPalette::from_theme(&theme_data);
        Appearance::handle(ctx).update(ctx, |a, mctx| a.set_theme(theme_data, mctx));
        self.current_theme = choice;
        if let Ok(rt) = self.terminal.lock() {
            rt.set_palette(palette.clone());
        }
        for tab in &self.terminal_tabs {
            for rt in tab.pane_terminals.values() {
                if let Ok(rt) = rt.lock() {
                    rt.set_palette(palette.clone());
                }
            }
        }
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_show_theme_chooser(&mut self, ctx: &mut ViewContext<Self>) {
        self.settings_view_state.borrow_mut().theme_chooser_open = true;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_close_theme_chooser(&mut self, ctx: &mut ViewContext<Self>) {
        self.settings_view_state.borrow_mut().theme_chooser_open = false;
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_language(
        &mut self,
        choice: LanguageChoice,
        ctx: &mut ViewContext<Self>,
    ) {
        self.language = choice;
        rust_i18n::set_locale(resolve_locale(choice));
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_terminal_font_size(
        &mut self,
        size: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        self.set_terminal_font_size(size);
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_reuse_view_tab(
        &mut self,
        enabled: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        self.reuse_view_tab = enabled;
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_opacity(
        &mut self,
        value: u8,
        ctx: &mut ViewContext<Self>,
    ) {
        self.window_opacity = value.clamp(1, 100);
        Self::apply_window_opacity(ctx, self.window_opacity);
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_glass_quality(
        &mut self,
        quality: GlassQualityChoice,
        ctx: &mut ViewContext<Self>,
    ) {
        self.glass_quality = quality;
        nexshell::glass_backdrop::set_glass_quality(quality);
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_cursor_style(
        &mut self,
        style: CursorStyleChoice,
        ctx: &mut ViewContext<Self>,
    ) {
        self.cursor_style = style;
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_font_family(
        &mut self,
        name: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let loaded =
            fonts::Cache::handle(ctx).update(ctx, |cache, _| cache.load_system_font(&name));
        if let Ok(fid) = loaded {
            self.monospace_font = fid;
            self.monospace_font_name = name.clone();
            if let Ok(mut cache) = self.shaped_line_cache.lock() {
                *cache = TerminalShapedLineCache::default();
            }
            // warp: appearance_page.rs:1928 — 更新 FilterableDropdown 选中项
            let label = name;
            self.font_family_dropdown.update(ctx, |d, ctx| {
                d.set_selected_by_name(label, ctx);
            });
            self.save_ui_settings();
        }
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_font_weight(
        &mut self,
        weight: warpui::fonts::Weight,
        ctx: &mut ViewContext<Self>,
    ) {
        self.monospace_font_weight = weight;
        warp_core::ui::appearance::Appearance::handle(ctx).update(ctx, |appearance, ctx| {
            appearance.set_monospace_font_weight(weight, ctx);
        });
        // warp: 更新 Dropdown 选中项
        let label = weight.to_string();
        self.font_weight_dropdown.update(ctx, |d, ctx| {
            d.set_selected_by_name(label, ctx);
        });
        self.save_ui_settings();
        ctx.notify();
    }

    /// EditorChoice → 下拉显示标签（创建/选中/切换共用，保证一致）
    pub(in crate::root_view) fn open_file_editor_label(choice: EditorChoice) -> String {
        match choice {
            EditorChoice::SystemDefault => {
                rust_i18n::t!("settings_editor_system_default").to_string()
            }
            EditorChoice::External(editor) => editor.display_name().to_string(),
        }
    }

    pub(in crate::root_view) fn handle_set_open_file_editor(
        &mut self,
        choice: EditorChoice,
        ctx: &mut ViewContext<Self>,
    ) {
        self.open_file_editor = choice;
        let label = Self::open_file_editor_label(choice);
        self.open_file_editor_dropdown.update(ctx, |d, ctx| {
            d.set_selected_by_name(label, ctx);
        });
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_set_line_height(
        &mut self,
        ratio: f32,
        ctx: &mut ViewContext<Self>,
    ) {
        self.line_height_ratio = ratio.clamp(
            TERMINAL_LINE_HEIGHT_RATIO_MIN,
            TERMINAL_LINE_HEIGHT_RATIO_MAX,
        );
        self.save_ui_settings();
        ctx.notify();
    }

    pub(in crate::root_view) fn handle_reset_line_height(&mut self, ctx: &mut ViewContext<Self>) {
        self.line_height_ratio = TERMINAL_LINE_HEIGHT_RATIO_DEFAULT;
        self.save_ui_settings();
        ctx.notify();
    }

    // warp: appearance_page.rs:2268-2271
    pub(in crate::root_view) fn handle_toggle_view_all_fonts(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let new_val = {
            let mut state = self.settings_view_state.borrow_mut();
            state.appearance_state.view_all_fonts = !state.appearance_state.view_all_fonts;
            state.appearance_state.view_all_fonts
        };
        let fonts = if new_val {
            &self.available_all_fonts
        } else {
            &self.available_monospace_fonts
        };
        let current_name = self.monospace_font_name.clone();
        let items: Vec<warp_dropdown_view::DropdownItem<TerminalGridAction>> = fonts
            .iter()
            .map(|name| {
                warp_dropdown_view::DropdownItem::new(
                    name.as_str(),
                    TerminalGridAction::SetFontFamily(name.clone()),
                )
            })
            .collect();
        self.font_family_dropdown.update(ctx, |d, ctx| {
            d.set_items(items, ctx);
            d.set_selected_by_name(current_name, ctx);
        });
        ctx.notify();
    }
}
