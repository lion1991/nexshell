// RootView 多文件 impl 容器。
// 详细决策见 docs/adr/0001-root-view-multi-file-impl.md。
//
// step 1-9 抽出 9 个 section（git_panel / file_panel / host_library / host_monitor /
// settings / tab_bar / terminal / find / context_menus）；step 11 收尾把
// struct RootView / new() / impl Entity / impl View / impl TypedActionView 整体迁入本文件。
// main.rs 仅留启动装配与伴生类型（TabModel / TerminalSessionTab / 常量等），经 crate:: 互引。

mod code_viewer_section;
mod context_menus_section;
mod file_panel_section;
mod find_section;
mod git_panel_section;
mod host_library_section;
mod host_monitor_section;
mod settings_section;
mod tab_bar_section;
mod terminal_section;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pathfinder_geometry::vector::{vec2f, Vector2F};
use warp_core::ui::appearance::Appearance;
use nexshell::text_editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use warp_core::ui::theme::AnsiColorIdentifier;
use warpui::{
    clipboard::ClipboardContent,
    elements::{
        Border, CacheOption, ChildAnchor, ClippedScrollStateHandle, CrossAxisAlignment,
        DispatchEventResult, DraggableState, EventHandler, Expanded, Flex, Image as ImageElement,
        MainAxisSize, MouseState, MouseStateHandle, OffsetPositioning, ParentAnchor, ParentElement,
        ParentOffsetBounds, PositionedElementAnchor, PositionedElementOffsetBounds, Shrinkable,
        Stack,
    },
    fonts,
    r#async::Timer,
    AppContext, BlurContext, CursorInfo, Element, Entity, FocusContext, SingletonEntity as _,
    TypedActionView, View, ViewContext,
};

use nexshell::file_panel::{
    apply_sftp_event, spawn_sftp_worker, FilePanelState, FilePanelWorkerHandle, SftpRequest,
};
use nexshell::git_panel::{
    apply_git_event, spawn_git_worker, GitEvent, GitPanelState, GitRequest,
};
use nexshell::host_management::{
    default_database_path, load_or_initialize_host_management_snapshot_from_db_path,
    unavailable_host_management_snapshot, HostConnectionConfig, HostConnectionPlan,
    HostManagementState, RecentHostSnapshot,
};
use nexshell::host_overview::{
    should_run_host_overview_monitor, should_show_host_overview_sidebar,
    spawn_host_overview_monitor, HostOverviewEvent, HostOverviewSnapshot, HostOverviewStatus,
    HostOverviewUiState,
};
use nexshell::host_overview_fleet::HostOverviewFleet;
use nexshell::pane_state::NexPaneId;
use nexshell::pane_tree::{Direction, DraggedBorder, PaneData};
use nexshell::pty_event_loop::PtyEvent;
use nexshell::ssh_key_store::SshKeyRecord;
use nexshell::terminal_runtime::{
    terminal_focus_report_bytes, LocalTerminalRuntime, RemoteSshConfig, SerialPortRuntimeConfig,
    TerminalInputEditor, TerminalPalette,
};
use nexshell::warp_horizontal_tabs::{new_tab_insert_index, NewTabPlacement};
use nexshell::warp_tab_context_menu::TabContextMenuAnchor;

use crate::external_editor::EditorChoice;
use crate::file_panel_view_helpers::file_panel_leaf_name;
use crate::font_enumeration::{
    enumerate_all_system_fonts, enumerate_monospace_fonts, load_nexshell_monospace_font,
    load_nexshell_ui_font,
};
use crate::git_panel_view_helpers::{git_commit_editor_options, GIT_COMMIT_DETAIL_CLEAR_DELAY};
use crate::group_tag_manage_window::GroupTagManageModel;
use crate::host_edit_window::HostEditModel;
use crate::host_management_view::HostManagementViewStates;
#[cfg(target_os = "macos")]
use crate::macos_window_util;
use crate::terminal_grid_element::{
    CursorStyleChoice, FindPanelState, LanguageChoice, ScrollbarDrag, TerminalGridAction,
    TerminalImeLayout, TerminalShapedLineCache, ThemeChoice, TERMINAL_CURSOR_POSITION_ID,
};
use crate::terminal_view_helpers::{
    connected_serial_tab_port, inactive_terminal_runtime, occupied_serial_port_index,
    root_debug_key_log, root_overlay_event_dispatch_mode, serial_port_from_host_config,
    terminal_context_menu_offset_bounds, terminal_overlay_event_dispatch_mode,
    terminal_window_title, update_cursor_blink,
};
use crate::throttle::throttle;
use crate::ui_colors::UiColors;
use crate::ui_settings::{
    load_ui_settings, resolve_locale, save_ui_settings_to_disk, UiSettings, TERMINAL_FONT_SIZE_MAX,
    TERMINAL_FONT_SIZE_MIN,
};
use crate::{settings_view, warp_dropdown_view, warp_filterable_dropdown};

// crate root 保留的伴生类型（helper/section 也经 crate:: 引用）。
use crate::{
    AppPage, CursorBlinkState, FilePanelInputIntent, HostPasswordIntent, TabModel,
    TerminalSessionKind, TerminalSessionTab,
};
// crate root 保留的布局/资源常量。
use crate::{
    DEFAULT_COLS, DEFAULT_ROWS, DEFAULT_WINDOW_TITLE, FILE_PANEL_WIDTH_DEFAULT,
    GIT_PANEL_WIDTH_DEFAULT, IDLE_REFRESH_INTERVAL, NEW_TAB_BUTTON_POSITION_ID,
    SETTINGS_BUTTON_POSITION_ID, TITLE_BAR_BORDER_HEIGHT, TITLE_BAR_HEIGHT, WAKEUP_THROTTLE_PERIOD,
};

pub(crate) struct RootView {
    // === 路由 ===
    window_id: warpui::WindowId,
    app_page: AppPage,

    // === 主机库（host_library_section）：状态 / 内联编辑器 / 密码栏 ===
    host_state: HostManagementState,
    host_view_states: RefCell<HostManagementViewStates>,
    host_status_fleet: HostOverviewFleet,
    // 密钥库缓存：(记录, 关联主机数)，进入密钥页 / 导入 / 删除时刷新
    host_keys: Vec<(SshKeyRecord, usize)>,
    // 选中密钥推导出的 openssh 公钥缓存（选中时算一次，避免每帧解密私钥）
    host_selected_key_public: Option<String>,
    // 最近访问主机缓存，连接 / 进入主机库时刷新
    host_recent: Vec<RecentHostSnapshot>,
    host_search_editor: warpui::ViewHandle<EditorView>,
    tab_rename_editor: warpui::ViewHandle<EditorView>,
    file_panel_input_editor: warpui::ViewHandle<EditorView>,
    file_panel_input_intent: Option<FilePanelInputIntent>,
    // 右键菜单：卡片内联重命名的 editor 与目标 host_id
    host_rename_editor: warpui::ViewHandle<EditorView>,
    host_rename_target: Option<String>,
    // 密钥页：内联编辑（名称 / 口令）的两个 editor 与编辑目标 id
    host_key_name_editor: warpui::ViewHandle<EditorView>,
    host_key_passphrase_editor: warpui::ViewHandle<EditorView>,
    host_key_edit_target: Option<String>,
    /// 主机库密码栏：editor 为 Some 时可见。intent 决定按钮文案与提交逻辑。
    host_password_editor: Option<warpui::ViewHandle<EditorView>>,
    host_password_intent: Option<HostPasswordIntent>,
    host_password_confirm_state: MouseStateHandle,
    host_password_cancel_state: MouseStateHandle,
    /// 后台正在跑 PBKDF2/AES 时为 true：禁用输入栏，显示进度文案。
    host_password_busy: bool,

    // === 文件面板（file_panel_section）：宽度 / 分隔条 ===
    /// (起始 mouse 屏幕 X, 起始面板宽度)；拖拽中 None 表示未在拖拽。
    file_panel_resize_anchor: Option<(f32, f32)>,
    file_panel_divider_state: MouseStateHandle,
    file_panel_divider_drag_state: DraggableState,

    // === Git 面板（git_panel_section）：宽度 / 分隔条 ===
    /// git 面板：仿 file_panel 同套，宽度全局共用，但 open 状态下放到 TerminalSessionTab。
    git_panel_width: f32,
    git_history_height: f32,
    git_panel_button_state: MouseStateHandle,
    git_panel_resize_anchor: Option<(f32, f32)>,
    git_history_resize_anchor: Option<(String, f32, f32)>,
    git_panel_divider_state: MouseStateHandle,
    git_panel_divider_drag_state: DraggableState,

    // === 主机库：编辑 / 分组管理子窗口 ===
    active_edit_model: Option<warpui::ModelHandle<HostEditModel>>,
    edit_window_id: Option<warpui::WindowId>,
    active_manage_model: Option<warpui::ModelHandle<GroupTagManageModel>>,
    manage_window_id: Option<warpui::WindowId>,

    // === 字体 ===
    monospace_font: fonts::FamilyId,
    ui_font: fonts::FamilyId,

    // === 标题栏 / Chrome（tab_bar_section）===
    sidebar_open: bool,
    sidebar_button_state: MouseStateHandle,
    host_tab_state: MouseStateHandle,
    new_tab_combo_state: MouseStateHandle,
    new_tab_plus_state: MouseStateHandle,
    new_tab_chevron_state: MouseStateHandle,
    settings_button_state: MouseStateHandle,
    window_control_minimize_state: MouseStateHandle,
    window_control_maximize_state: MouseStateHandle,
    window_control_close_state: MouseStateHandle,
    settings_menu_open: bool,
    file_panel_button_state: MouseStateHandle,

    // === 菜单 / 右键菜单（tab_bar / context_menus_section）===
    settings_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    new_session_menu_open: bool,
    new_session_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    tab_right_click_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_tab_right_click_menu: Option<(usize, TabContextMenuAnchor)>,
    terminal_context_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_terminal_context_menu: Option<Vector2F>,
    file_panel_context_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_file_panel_context_menu: Option<Vector2F>,
    git_panel_context_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_git_panel_context_menu: Option<Vector2F>,
    process_list_context_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_process_list_context_menu: Option<Vector2F>,
    host_card_context_menu: warpui::ViewHandle<nexshell::menu::Menu<TerminalGridAction>>,
    show_host_card_context_menu: Option<Vector2F>,

    // === 标签页（tab_bar_section）===
    tab_bar_hover_state: MouseStateHandle,
    tab_states: Vec<MouseStateHandle>,
    tab_tooltip_states: Vec<MouseStateHandle>,
    tab_close_states: Vec<MouseStateHandle>,
    tab_draggable_states: Vec<DraggableState>,
    tab_selected_colors: Vec<Option<AnsiColorIdentifier>>,
    tab_being_renamed: Option<usize>,
    terminal_tabs: Vec<TerminalSessionTab>,
    // main.rs 的 nexshell:close_tab 全局 action 需读取当前 tab，故 crate 可见。
    pub(crate) active_tab_index: usize,
    next_terminal_tab_seq: usize,
    tab_fixed_width: Option<f32>,
    tab_drag_in_progress: bool,

    // === 终端运行时 / 交互（terminal_section）===
    terminal: Arc<Mutex<LocalTerminalRuntime>>,
    input_editor: Arc<Mutex<TerminalInputEditor>>,
    selection_drag: Arc<Mutex<bool>>,
    last_resize_cells: Arc<Mutex<(u16, u16)>>,
    scrollbar_drag: Arc<Mutex<Option<ScrollbarDrag>>>,
    cursor_over_terminal: Arc<Mutex<bool>>,
    scrollbar_thumb_hovered: Arc<Mutex<bool>>,

    // === 查找栏（find_section）===
    find_state: Arc<Mutex<FindPanelState>>,
    find_editor: warpui::ViewHandle<EditorView>,
    find_btn_next: MouseStateHandle,
    find_btn_prev: MouseStateHandle,
    find_btn_close: MouseStateHandle,

    // === 终端渲染 / 光标 ===
    smooth_scroll_px: Arc<Mutex<f64>>,
    shaped_line_cache: Arc<Mutex<TerminalShapedLineCache>>,
    terminal_ime_layout: Arc<Mutex<Option<TerminalImeLayout>>>,
    terminal_font_size: f32,
    line_height_ratio: f32,
    /// 录制中标签红点的闪烁相位，由 idle tick 驱动。
    recording_blink: CursorBlinkState,

    // === 其它运行时状态（推送动画 / 窗口 / 分屏）===
    git_push_animation_tick: u64,
    last_window_title: String,
    /// 各 tab 的前台进程 flag，与 on_should_close_window 回调共享
    foreground_flags: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    dragged_border: Option<DraggedBorder>,
    maximized_pane: Option<NexPaneId>,

    // === 设置页（settings_section）===
    settings_tab_open: bool,
    settings_tab_state: MouseStateHandle,
    settings_tab_close_state: MouseStateHandle,
    settings_view_state: RefCell<settings_view::NexSettingsViewState>,

    // === 外观设置（settings_section）===
    current_theme: ThemeChoice,
    // warp: 缓存当前 WarpTheme，避免每帧 WarpThemeConfig::new()
    cached_warp_theme: warp_core::ui::theme::WarpTheme,
    window_opacity: u8,
    cursor_style: CursorStyleChoice,
    monospace_font_name: String,
    monospace_font_weight: warpui::fonts::Weight,
    language: LanguageChoice,
    /// 文件面板「编辑」用的编辑器（探测到的候选只在下拉创建时用，不存字段）。
    open_file_editor: EditorChoice,
    /// 「diff / 查看器复用单标签」开关（ADR 0002，默认开启）。
    reuse_view_tab: bool,
    available_monospace_fonts: Vec<String>,
    available_all_fonts: Vec<String>,
    // warp: appearance_page.rs:499-500
    font_family_dropdown:
        warpui::ViewHandle<warp_filterable_dropdown::FilterableDropdown<TerminalGridAction>>,
    font_weight_dropdown: warpui::ViewHandle<warp_dropdown_view::Dropdown<TerminalGridAction>>,
    // 文件面板「编辑」编辑器选择下拉
    open_file_editor_dropdown: warpui::ViewHandle<warp_dropdown_view::Dropdown<TerminalGridAction>>,
    // 首帧预热标志：在首帧把 settings 页面隐藏渲染，预热框架 layout/paint 缓存
    settings_prewarmed: std::cell::Cell<bool>,
    last_host_swap_time: Option<std::time::Instant>,
    /// 远程保存在途时，关闭/换文件确认的「保存」续作暂存于此，待写成功后补执行（review C）。按 tab_id 键。
    code_viewer_pending_post: std::collections::HashMap<String, code_viewer_section::PostSave>,
}

impl RootView {
    // 由 main.rs 的 open_main_window 调用，故需对父模块（crate root）可见。
    pub(super) fn new(
        ctx: &mut ViewContext<Self>,
        foreground_flags: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    ) -> Self {
        let ui_settings = load_ui_settings();
        rust_i18n::set_locale(resolve_locale(ui_settings.language));
        let monospace_font = fonts::Cache::handle(ctx).update(ctx, |cache, _| {
            load_nexshell_monospace_font(cache, Some(&ui_settings.font_family))
        });
        // UI font for chrome (tabs / titlebar). Falls back to monospace if Helvetica Neue 不可用。
        let ui_font = fonts::Cache::handle(ctx).update(ctx, |cache, _| {
            load_nexshell_ui_font(cache).unwrap_or(monospace_font)
        });

        // Needed for KeyDown delivery.
        ctx.focus_self();
        Self::sync_titlebar_height(ctx);
        Self::apply_window_opacity(ctx, ui_settings.opacity);

        // 不为占位 spawn 真 shell（Warp 无此物，PTY 仅真会话时才建）；首个真终端按需 spawn。
        let terminal = LocalTerminalRuntime::failed("placeholder", "");
        let last_resize_cells = Arc::new(Mutex::new((DEFAULT_COLS, DEFAULT_ROWS)));
        let terminal = Arc::new(Mutex::new(terminal));
        let terminal_tabs: Vec<TerminalSessionTab> = Vec::new();

        // Only drives blink and bell decay while PTY is idle.
        Self::schedule_idle_refresh(ctx);

        let (host_snapshot, host_notice) = match default_database_path() {
            Some(db_path) => {
                match load_or_initialize_host_management_snapshot_from_db_path(&db_path) {
                    Ok(snapshot) => (snapshot, None),
                    Err(error) => (
                        unavailable_host_management_snapshot(),
                        Some(
                            rust_i18n::t!("toast_db_read_failed", error = error.to_string())
                                .to_string(),
                        ),
                    ),
                }
            }
            None => (
                unavailable_host_management_snapshot(),
                Some(rust_i18n::t!("toast_db_path_unavailable").to_string()),
            ),
        };
        let mut host_state = HostManagementState::new(host_snapshot);
        host_state.notice = host_notice;
        let host_search_editor = Self::create_host_search_editor(ctx);
        ctx.subscribe_to_view(&host_search_editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_host_search_editor_event(event, ctx);
        });
        let tab_rename_editor = Self::create_tab_rename_editor(ctx);
        let file_panel_input_editor = Self::create_file_panel_input_editor(ctx);
        let host_rename_editor = Self::create_host_rename_editor(ctx);
        let host_key_name_editor = Self::create_host_key_name_editor(ctx);
        let host_key_passphrase_editor = Self::create_host_key_passphrase_editor(ctx);

        if !ui_settings.font_weight.is_normal() {
            Appearance::handle(ctx).update(ctx, |a, mctx| {
                a.set_monospace_font_weight(ui_settings.font_weight, mctx);
            });
        }

        // warp: appearance_page.rs:995-1015 — FilterableDropdown for font family
        let monospace_fonts = enumerate_monospace_fonts();
        let all_fonts = enumerate_all_system_fonts();
        let font_family_dropdown = {
            let font_name = ui_settings.font_family.clone();
            let fonts_for_init: Vec<String> = monospace_fonts.clone();
            ctx.add_typed_action_view(move |ctx| {
                let mut dropdown = warp_filterable_dropdown::FilterableDropdown::new(ctx);
                dropdown.set_top_bar_max_width(225.0);
                dropdown.set_menu_width(225.0, ctx);
                let items: Vec<warp_dropdown_view::DropdownItem<TerminalGridAction>> =
                    fonts_for_init
                        .iter()
                        .map(|name| {
                            warp_dropdown_view::DropdownItem::new(
                                name.as_str(),
                                TerminalGridAction::SetFontFamily(name.clone()),
                            )
                        })
                        .collect();
                dropdown.add_items(items, ctx);
                dropdown.set_selected_by_name(font_name, ctx);
                dropdown
            })
        };

        // warp: appearance_page.rs:1017-1035 — Dropdown for font weight
        let open_file_editor_dropdown = {
            let current = ui_settings.open_file_editor;
            let editors = crate::external_editor::detect_installed_editors();
            ctx.add_typed_action_view(move |ctx| {
                let mut dropdown = warp_dropdown_view::Dropdown::new(ctx);
                dropdown.set_top_bar_max_width(180.0);
                dropdown.set_menu_width(180.0, ctx);
                let mut items = vec![warp_dropdown_view::DropdownItem::new(
                    Self::open_file_editor_label(EditorChoice::SystemDefault),
                    TerminalGridAction::SetOpenFileEditor(EditorChoice::SystemDefault),
                )];
                for editor in editors {
                    items.push(warp_dropdown_view::DropdownItem::new(
                        Self::open_file_editor_label(EditorChoice::External(editor)),
                        TerminalGridAction::SetOpenFileEditor(EditorChoice::External(editor)),
                    ));
                }
                dropdown.add_items(items, ctx);
                dropdown.set_selected_by_name(Self::open_file_editor_label(current), ctx);
                dropdown
            })
        };

        let font_weight_dropdown = {
            let weight = ui_settings.font_weight;
            ctx.add_typed_action_view(move |ctx| {
                let mut dropdown = warp_dropdown_view::Dropdown::new(ctx);
                dropdown.set_top_bar_max_width(120.0);
                dropdown.set_menu_width(120.0, ctx);
                let items = vec![
                    warp_dropdown_view::DropdownItem::new(
                        "Normal",
                        TerminalGridAction::SetFontWeight(warpui::fonts::Weight::Normal),
                    ),
                    warp_dropdown_view::DropdownItem::new(
                        "Bold",
                        TerminalGridAction::SetFontWeight(warpui::fonts::Weight::Bold),
                    ),
                ];
                dropdown.add_items(items, ctx);
                dropdown.set_selected_by_name(weight.to_string(), ctx);
                dropdown
            })
        };

        let cached_warp_theme = ui_settings.theme.to_warp_theme();
        if ui_settings.theme != ThemeChoice::Dark {
            let palette = TerminalPalette::from_theme(&cached_warp_theme);
            let theme_for_appearance = cached_warp_theme.clone();
            Appearance::handle(ctx).update(ctx, |a, mctx| a.set_theme(theme_for_appearance, mctx));
            if let Ok(rt) = terminal.lock() {
                rt.set_palette(palette);
            }
        }

        let mut view = Self {
            window_id: ctx.window_id(),
            app_page: AppPage::HostManagement,
            host_state,
            host_view_states: RefCell::new(HostManagementViewStates::new()),
            host_status_fleet: HostOverviewFleet::new(),
            host_keys: Vec::new(),
            host_selected_key_public: None,
            host_recent: Vec::new(),
            host_search_editor,
            tab_rename_editor,
            file_panel_input_editor,
            file_panel_input_intent: None,
            host_rename_editor,
            host_rename_target: None,
            host_key_name_editor,
            host_key_passphrase_editor,
            host_key_edit_target: None,
            host_password_editor: None,
            host_password_intent: None,
            host_password_confirm_state: Arc::new(Mutex::new(MouseState::default())),
            host_password_cancel_state: Arc::new(Mutex::new(MouseState::default())),
            host_password_busy: false,
            file_panel_resize_anchor: None,
            file_panel_divider_state: Arc::new(Mutex::new(MouseState::default())),
            file_panel_divider_drag_state: {
                let s = DraggableState::default();
                s.set_suppress_overlay_paint(true);
                s
            },
            git_panel_width: GIT_PANEL_WIDTH_DEFAULT,
            git_history_height: ui_settings.git_history_height,
            git_panel_button_state: Arc::new(Mutex::new(MouseState::default())),
            git_panel_resize_anchor: None,
            git_history_resize_anchor: None,
            git_panel_divider_state: Arc::new(Mutex::new(MouseState::default())),
            git_panel_divider_drag_state: {
                let s = DraggableState::default();
                s.set_suppress_overlay_paint(true);
                s
            },
            active_edit_model: None,
            edit_window_id: None,
            active_manage_model: None,
            manage_window_id: None,
            monospace_font,
            ui_font,
            sidebar_open: ui_settings.sidebar_open,
            sidebar_button_state: Arc::new(Mutex::new(MouseState::default())),
            host_tab_state: Arc::new(Mutex::new(MouseState::default())),
            new_tab_combo_state: Arc::new(Mutex::new(MouseState::default())),
            new_tab_plus_state: Arc::new(Mutex::new(MouseState::default())),
            new_tab_chevron_state: Arc::new(Mutex::new(MouseState::default())),
            settings_button_state: Arc::new(Mutex::new(MouseState::default())),
            window_control_minimize_state: Arc::new(Mutex::new(MouseState::default())),
            window_control_maximize_state: Arc::new(Mutex::new(MouseState::default())),
            window_control_close_state: Arc::new(Mutex::new(MouseState::default())),
            settings_menu_open: false,
            file_panel_button_state: Arc::new(Mutex::new(MouseState::default())),
            settings_menu: {
                let menu = ctx.add_typed_action_view(|ctx| {
                    let theme = warp_core::ui::appearance::Appearance::as_ref(ctx).theme();
                    nexshell::menu::Menu::new()
                        .with_width(220.)
                        .with_border(Border::all(1.).with_border_color(theme.outline().into()))
                        .with_drop_shadow()
                        .prevent_interaction_with_other_elements()
                });
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.settings_menu_open = false;
                        ctx.notify();
                    }
                });
                menu
            },
            new_session_menu_open: false,
            new_session_menu: {
                let menu = ctx.add_typed_action_view(|ctx| {
                    let theme = warp_core::ui::appearance::Appearance::as_ref(ctx).theme();
                    nexshell::menu::Menu::new()
                        .with_width(200.)
                        .with_border(Border::all(1.).with_border_color(theme.outline().into()))
                        .with_drop_shadow()
                        .prevent_interaction_with_other_elements()
                });
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.new_session_menu_open = false;
                        ctx.notify();
                    }
                });
                menu
            },
            tab_right_click_menu: {
                // warp/app/src/workspace/view.rs:1744-1750.
                let menu = ctx.add_typed_action_view(|_| nexshell::menu::Menu::new());
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_tab_right_click_menu = None;
                        ctx.notify();
                    }
                });
                menu
            },
            show_tab_right_click_menu: None,
            terminal_context_menu: {
                // warp: view.rs:3696-3702
                let menu = ctx.add_typed_action_view(|_| {
                    nexshell::menu::Menu::new()
                        .with_drop_shadow()
                        .prevent_interaction_with_other_elements()
                });
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_terminal_context_menu = None;
                        ctx.notify();
                    }
                });
                menu
            },
            show_terminal_context_menu: None,
            file_panel_context_menu: {
                // 不加 prevent_interaction_with_other_elements：菜单打开时，对其他 entry
                // 右键仍能冒泡到 entry handler → 重新 dispatch ShowContextMenu 替换菜单
                let menu =
                    ctx.add_typed_action_view(|_| nexshell::menu::Menu::new().with_drop_shadow());
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_file_panel_context_menu = None;
                        ctx.notify();
                    }
                });
                menu
            },
            show_file_panel_context_menu: None,
            git_panel_context_menu: {
                let menu =
                    ctx.add_typed_action_view(|_| nexshell::menu::Menu::new().with_drop_shadow());
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_git_panel_context_menu = None;
                        ctx.notify();
                    }
                });
                menu
            },
            show_git_panel_context_menu: None,
            process_list_context_menu: {
                let menu =
                    ctx.add_typed_action_view(|_| nexshell::menu::Menu::new().with_drop_shadow());
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_process_list_context_menu = None;
                        if let Some(tab) = me.terminal_tabs.get_mut(me.active_tab_index) {
                            tab.process_list_selected_pid = None;
                        }
                        ctx.notify();
                    }
                });
                menu
            },
            show_process_list_context_menu: None,
            host_card_context_menu: {
                let menu =
                    ctx.add_typed_action_view(|_| nexshell::menu::Menu::new().with_drop_shadow());
                ctx.subscribe_to_view(&menu, |me, _, event: &nexshell::menu::Event, ctx| {
                    if matches!(event, nexshell::menu::Event::Close { .. }) {
                        me.show_host_card_context_menu = None;
                        me.host_state.context_menu_target = None;
                        ctx.notify();
                    }
                });
                menu
            },
            show_host_card_context_menu: None,
            tab_bar_hover_state: Arc::new(Mutex::new(MouseState::default())),
            tab_states: Vec::new(),
            tab_tooltip_states: Vec::new(),
            tab_close_states: Vec::new(),
            tab_draggable_states: Vec::new(),
            tab_selected_colors: Vec::new(),
            tab_being_renamed: None,
            terminal_tabs,
            active_tab_index: 0,
            next_terminal_tab_seq: 1,
            tab_fixed_width: None,
            tab_drag_in_progress: false,
            terminal,
            input_editor: Arc::new(Mutex::new(TerminalInputEditor::default())),
            selection_drag: Arc::new(Mutex::new(false)),
            last_resize_cells,
            scrollbar_drag: Arc::new(Mutex::new(None)),
            cursor_over_terminal: Arc::new(Mutex::new(false)),
            scrollbar_thumb_hovered: Arc::new(Mutex::new(false)),
            find_state: Arc::new(Mutex::new(FindPanelState::default())),
            find_editor: Self::create_find_editor(ctx),
            find_btn_next: Default::default(),
            find_btn_prev: Default::default(),
            find_btn_close: Default::default(),
            smooth_scroll_px: Arc::new(Mutex::new(0.0)),
            shaped_line_cache: Arc::new(Mutex::new(TerminalShapedLineCache::default())),
            terminal_ime_layout: Arc::new(Mutex::new(None)),
            terminal_font_size: ui_settings.font_size,
            line_height_ratio: ui_settings.line_height_ratio,
            recording_blink: CursorBlinkState::default(),
            git_push_animation_tick: 0,
            last_window_title: DEFAULT_WINDOW_TITLE.to_string(),
            foreground_flags,
            dragged_border: None,
            maximized_pane: None,
            settings_tab_open: false,
            settings_tab_state: Arc::new(Mutex::new(MouseState::default())),
            settings_tab_close_state: Arc::new(Mutex::new(MouseState::default())),
            settings_view_state: RefCell::new(settings_view::NexSettingsViewState::default()),
            current_theme: ui_settings.theme,
            cached_warp_theme,
            window_opacity: ui_settings.opacity,
            cursor_style: ui_settings.cursor_style,
            monospace_font_name: ui_settings.font_family.clone(),
            monospace_font_weight: ui_settings.font_weight,
            language: ui_settings.language,
            open_file_editor: ui_settings.open_file_editor,
            reuse_view_tab: ui_settings.reuse_view_tab,
            available_monospace_fonts: monospace_fonts,
            available_all_fonts: all_fonts,
            font_family_dropdown,
            font_weight_dropdown,
            open_file_editor_dropdown,
            settings_prewarmed: std::cell::Cell::new(false),
            last_host_swap_time: None,
            code_viewer_pending_post: std::collections::HashMap::new(),
        };
        view.reload_host_recent(); // 启动首屏即填充最近访问
        view
    }

    fn create_tab_rename_editor(ctx: &mut ViewContext<Self>) -> warpui::ViewHandle<EditorView> {
        let editor = ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                ..Default::default()
            };
            EditorView::single_line(options, ctx)
        });
        ctx.subscribe_to_view(&editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_tab_rename_editor_event(event, ctx);
        });
        editor
    }

    fn attach_terminal_streams(
        terminal: &mut LocalTerminalRuntime,
        tab_id: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let terminal_session_id = terminal.snapshot().session_id.clone();
        if let Some(wakeup_rx) = terminal.take_wakeup_rx() {
            ctx.spawn_stream_local(
                throttle(WAKEUP_THROTTLE_PERIOD, wakeup_rx),
                Self::handle_terminal_wakeup,
                |_, _| {},
            );
        }

        if let Some(event_rx) = terminal.take_event_rx() {
            if let Some(tab_id) = tab_id {
                ctx.spawn_stream_local(
                    event_rx,
                    move |view, event, ctx| {
                        view.handle_terminal_event_for_tab(
                            &tab_id,
                            &terminal_session_id,
                            event,
                            ctx,
                        );
                    },
                    |_, _| {},
                );
            } else {
                ctx.spawn_stream_local(event_rx, Self::handle_terminal_event, |_, _| {});
            }
        }
    }

    /// 接 SSH handle 流（remote SSH session 才有）。认证成功后 channel 会发一个
    /// SshHandle，存到对应 tab 上供文件面板用。
    fn attach_ssh_handle_stream(
        terminal: &mut LocalTerminalRuntime,
        tab_id: String,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(rx) = terminal.take_ssh_handle_rx() {
            ctx.spawn_stream_local(
                rx,
                move |view, handle, ctx| {
                    let tab_id_for_worker = tab_id.clone();
                    if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.ssh_handle = Some(handle.clone());
                        ctx.notify();
                    }
                    // 重连/首连拿到新 handle：同步刷新该终端派生的远程编辑标签 handle，
                    // 否则旧 handle 随断开失效后，已打开的编辑标签永远存不回（缺陷3）。
                    for t in view.terminal_tabs.iter_mut() {
                        if matches!(t.kind, TerminalSessionKind::CodeViewer)
                            && t.host_id.as_deref() == Some(tab_id.as_str())
                            && t.code_viewer_ssh_handle.is_some()
                        {
                            t.code_viewer_ssh_handle = Some(handle.clone());
                        }
                    }
                    // 如果面板此刻已开但 worker 还没起，handle 一就绪就 spawn。
                    let need_spawn = view
                        .terminal_tabs
                        .iter()
                        .find(|t| t.id == tab_id_for_worker)
                        .map(|t| t.file_panel_open && t.sftp_worker.is_none())
                        .unwrap_or(false);
                    if need_spawn {
                        Self::start_sftp_worker_for_tab(view, &tab_id_for_worker, ctx);
                    }
                },
                |_, _| {},
            );
        }
    }

    /// 启动指定 tab 的 SFTP worker，并把事件流接到 view 上。
    /// 调用方需保证 tab.ssh_handle 已就绪。
    pub(crate) fn start_sftp_worker_for_tab(
        view: &mut Self,
        tab_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let (handle, label, init_path) = {
            let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            let Some(handle) = tab.ssh_handle.clone() else {
                return;
            };
            (handle, tab.id.clone(), tab.file_panel_state.cwd.clone())
        };
        match spawn_sftp_worker(handle, &label) {
            Ok((worker, evt_rx)) => {
                worker.send(SftpRequest::List(init_path));
                if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.sftp_worker = Some(FilePanelWorkerHandle::Sftp(worker));
                    tab.file_panel_state.loading = true;
                    tab.file_panel_state.error = None;
                }
                let owner = tab_id.to_string();
                ctx.spawn_stream_local(
                    evt_rx,
                    move |view, evt, ctx| {
                        if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == owner) {
                            apply_sftp_event(&mut tab.file_panel_state, evt);
                            ctx.notify();
                        }
                    },
                    |_, _| {},
                );
            }
            Err(error) => {
                if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.file_panel_state.error = Some(error);
                    tab.file_panel_state.loading = false;
                }
            }
        }
    }

    /// 启动指定本地 tab 的文件 worker，并把事件流接到 view 上。
    fn terminal_tab_label(&self, index: usize) -> String {
        self.terminal_tabs
            .get(index)
            .map(TerminalSessionTab::label)
            .unwrap_or_else(|| format!("session {}", index + 1))
    }

    fn open_serial_tab_index(&self, port: &str, skip_index: Option<usize>) -> Option<usize> {
        occupied_serial_port_index(
            self.terminal_tabs.iter().map(connected_serial_tab_port),
            port,
            skip_index,
        )
    }

    fn release_terminal_runtime_for_tab(&mut self, index: usize) {
        let Some(tab) = self.terminal_tabs.get_mut(index) else {
            return;
        };
        let replacement = inactive_terminal_runtime();
        tab.pane_terminals
            .insert(tab.focused_pane_id, Arc::clone(&replacement));
        tab.terminal = Arc::clone(&replacement);
        if self.active_tab_index == index {
            self.terminal = replacement;
        }
    }

    fn active_terminal_window_title(&self) -> String {
        self.terminal_tabs
            .get(self.active_tab_index)
            .map(TerminalSessionTab::window_title)
            .unwrap_or_else(|| DEFAULT_WINDOW_TITLE.to_string())
    }

    fn handle_terminal_wakeup(&mut self, _: (), ctx: &mut ViewContext<Self>) {
        let (title, should_clear_editor) = if let Ok(rt) = self.terminal.lock() {
            rt.refresh_foreground_status();
            let snap = rt.snapshot();
            let clear = !rt.shell_is_foreground()
                || snap.grid.input_modes.alt_screen
                || snap.grid.mouse_app_active();
            (rt.title(), clear)
        } else {
            return;
        };
        if should_clear_editor {
            if let Ok(mut editor) = self.input_editor.lock() {
                editor.clear();
            }
        }
        let title = self
            .terminal_tabs
            .get(self.active_tab_index)
            .and_then(|tab| tab.custom_label.clone())
            .or(title)
            .unwrap_or_else(|| self.active_terminal_window_title());
        self.sync_terminal_window_title(Some(&title), ctx);
        self.flush_terminal_clipboard_requests(ctx);
        Self::dispatch_git_cwd_updates(self, ctx);
        Self::dispatch_file_panel_cwd_updates(self, ctx);
        ctx.notify();
    }

    /// 扫描所有本地 tab 的 snapshot.local_cwd，与上次派发对比；变化即 lazy spawn
    /// git worker 并发 SetCwd。远程 / 串口 tab 跳过。
    fn dispatch_git_cwd_updates(view: &mut Self, ctx: &mut ViewContext<Self>) {
        let pending: Vec<(String, PathBuf)> = view
            .terminal_tabs
            .iter()
            .filter(|t| matches!(t.kind, TerminalSessionKind::Local))
            .filter_map(|t| {
                let snap_cwd = t.terminal.lock().ok()?.snapshot().local_cwd.clone()?;
                if t.git_last_dispatched_cwd.as_ref() == Some(&snap_cwd) {
                    None
                } else {
                    Some((t.id.clone(), snap_cwd))
                }
            })
            .collect();
        for (tab_id, cwd) in pending {
            Self::ensure_git_worker_for_tab(view, &tab_id, ctx);
            if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                if let Some(worker) = tab.git_worker.as_ref() {
                    if worker.send(GitRequest::SetCwd(cwd.clone())) {
                        tab.git_last_dispatched_cwd = Some(cwd);
                    }
                }
            }
        }
    }

    /// 本地文件面板默认跟随终端 OSC 7 cwd；用户手动进入目录后会关闭 follow_cwd。
    /// 若 tab 还没有 git worker，spawn 一个并把 event 流接到 view。
    fn ensure_git_worker_for_tab(view: &mut Self, tab_id: &str, ctx: &mut ViewContext<Self>) {
        let need = view
            .terminal_tabs
            .iter()
            .any(|t| t.id == tab_id && t.git_worker.is_none());
        if !need {
            return;
        }
        match spawn_git_worker(tab_id) {
            Ok((worker, evt_rx)) => {
                if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.git_worker = Some(worker);
                }
                let owner = tab_id.to_string();
                ctx.spawn_stream_local(
                    evt_rx,
                    move |view, evt: GitEvent, ctx| {
                        let evt_for_diff_tabs = evt.clone();
                        let commit_finished = match &evt {
                            GitEvent::CommitFinished { success } => Some(*success),
                            _ => None,
                        };
                        let push_finished = match &evt {
                            GitEvent::PushFinished { success } => Some(*success),
                            _ => None,
                        };
                        let ssh_host_key_prompt = match &evt {
                            GitEvent::SshHostKeyPrompt { prompt } => Some(prompt.clone()),
                            _ => None,
                        };
                        let mut clear_commit_editor = None;
                        let mut changed = false;
                        let mut show_ssh_host_key_prompt = None;
                        if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == owner) {
                            apply_git_event(&mut tab.git_panel_state, evt);
                            let hovered_commit_is_stale = tab
                                .git_panel_hovered_commit
                                .as_ref()
                                .map(|active_sha| {
                                    !tab.git_panel_state
                                        .recent_commits
                                        .iter()
                                        .any(|commit| &commit.sha == active_sha)
                                })
                                .unwrap_or(false);
                            if hovered_commit_is_stale {
                                tab.git_panel_hovered_commit = None;
                                tab.git_panel_hover_clear_after = None;
                            }
                            let selected_commit_is_stale = tab
                                .git_panel_selected_commit
                                .as_ref()
                                .map(|active_sha| {
                                    !tab.git_panel_state
                                        .recent_commits
                                        .iter()
                                        .any(|commit| &commit.sha == active_sha)
                                })
                                .unwrap_or(false);
                            if selected_commit_is_stale {
                                tab.git_panel_selected_commit = None;
                            }
                            if let Some(success) = commit_finished {
                                tab.git_commit_busy = false;
                                if success {
                                    clear_commit_editor = Some(tab.git_commit_editor.clone());
                                }
                            }
                            if push_finished.is_some() {
                                tab.git_push_busy = false;
                            }
                            if let Some(prompt) = ssh_host_key_prompt {
                                tab.git_push_busy = false;
                                show_ssh_host_key_prompt = Some((owner.clone(), prompt));
                            }
                            changed = true;
                        }
                        if view.apply_git_event_to_diff_tabs(&owner, evt_for_diff_tabs) {
                            changed = true;
                        }
                        if let Some(editor) = clear_commit_editor {
                            editor.update(ctx, |editor, ctx| {
                                editor.clear_buffer_and_reset_undo_stack(ctx);
                            });
                        }
                        if let Some((tab_id, prompt)) = show_ssh_host_key_prompt {
                            view.show_git_ssh_host_key_prompt(tab_id, prompt, ctx);
                        }
                        if changed {
                            ctx.notify();
                        }
                    },
                    |_, _| {},
                );
            }
            Err(error) => {
                if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.git_panel_state.error = Some(error);
                }
            }
        }
    }

    /// Lifecycle events from the PTY thread (child exit, write error).
    /// We just notify — the snapshot already reflects the new
    /// `connected = false` / `status` state because `mark_disconnected`
    /// flipped them inside the FairMutex.
    fn handle_terminal_event(&mut self, event: PtyEvent, ctx: &mut ViewContext<Self>) {
        match event {
            PtyEvent::ChildExited | PtyEvent::Disconnected(_) => {
                ctx.notify();
            }
        }
    }

    fn handle_terminal_event_for_tab(
        &mut self,
        tab_id: &str,
        terminal_session_id: &str,
        event: PtyEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            PtyEvent::ChildExited | PtyEvent::Disconnected(_) => {
                if self.terminal_tab_runtime_session_matches(tab_id, terminal_session_id) {
                    self.stop_host_overview_monitor_for_tab(tab_id, "终端已断开");
                    // 断线：清死 SFTP worker 并标记文件区，避免死 session 被静默复用（缺陷1）。
                    self.mark_remote_file_panel_disconnected(tab_id);
                }
                ctx.notify();
            }
        }
    }

    fn terminal_tab_runtime_session_matches(
        &self,
        tab_id: &str,
        terminal_session_id: &str,
    ) -> bool {
        self.terminal_tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .and_then(|tab| tab.terminal.lock().ok())
            .map(|runtime| runtime.snapshot().session_id == terminal_session_id)
            .unwrap_or(false)
    }

    pub(crate) fn terminal_tab_is_connected(tab: &TerminalSessionTab) -> bool {
        tab.terminal
            .lock()
            .map(|runtime| runtime.snapshot().connected)
            .unwrap_or(false)
    }

    /// 详情页 tab（进程/网络/系统信息）的终端是占位 failed runtime（connected 恒 false），
    /// 概览数据走 monitor 自身的 SSH 连接，断开与否由 monitor 的 Error 事件反映，
    /// 不能用 terminal_tab_is_connected 判断。
    fn tab_is_host_overview_page(tab: &TerminalSessionTab) -> bool {
        matches!(
            tab.kind,
            TerminalSessionKind::ProcessList
                | TerminalSessionKind::NetworkList
                | TerminalSessionKind::SystemInfo
        )
    }

    fn stop_host_overview_monitor_for_tab(&mut self, tab_id: &str, reason: &str) {
        let Some(index) = self.terminal_tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let supports_host_overview = self.terminal_tabs[index]
            .host_id
            .as_deref()
            .and_then(|host_id| self.host_state.connection_plan_for(host_id))
            .is_some_and(|plan| matches!(plan, HostConnectionPlan::SavedSsh { .. }));
        if !supports_host_overview {
            return;
        }

        let tab = &mut self.terminal_tabs[index];
        tab.host_overview_monitor = None;
        let host_label = tab.host_overview.snapshot.host.trim();
        let host_label = if host_label.is_empty() || host_label == "未连接" {
            tab.label()
        } else {
            host_label.to_string()
        };
        let hostname = tab.host_overview.snapshot.hostname.clone();
        let mut snapshot = HostOverviewSnapshot::waiting(host_label);
        snapshot.hostname = hostname.or_else(|| Some(tab.label()));
        snapshot.status = HostOverviewStatus::Error(reason.to_string());
        tab.host_overview.set_waiting_snapshot(snapshot);
        tab.host_overview_network_item_states.borrow_mut().clear();
    }

    fn handle_host_overview_event_for_tab(
        &mut self,
        tab_id: &str,
        event: HostOverviewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(index) = self.terminal_tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let tab = &self.terminal_tabs[index];
        if !Self::tab_is_host_overview_page(tab) && !Self::terminal_tab_is_connected(tab) {
            self.stop_host_overview_monitor_for_tab(tab_id, "终端已断开");
            ctx.notify();
            return;
        }
        self.terminal_tabs[index].host_overview.apply_event(event);
        ctx.notify();
    }

    fn sync_host_overview_monitor(&mut self, ctx: &mut ViewContext<Self>) {
        let active_tab_supports_host_overview = self.active_tab_supports_host_overview();
        if self.app_page != AppPage::Terminal || !active_tab_supports_host_overview {
            return;
        }
        let Some(tab) = self.terminal_tabs.get(self.active_tab_index) else {
            return;
        };
        // 详情页：终端是占位 runtime 且不依赖侧栏开关，跳过这两项门控
        let is_overview_page = Self::tab_is_host_overview_page(tab);
        let terminal_connected = is_overview_page || Self::terminal_tab_is_connected(tab);
        if !terminal_connected {
            let tab_id = tab.id.clone();
            self.stop_host_overview_monitor_for_tab(&tab_id, "终端已断开");
            return;
        }
        if !is_overview_page
            && !should_run_host_overview_monitor(
                self.sidebar_open,
                active_tab_supports_host_overview,
                terminal_connected,
            )
        {
            return;
        }
        if tab.host_overview_monitor.is_some() {
            return;
        }

        let tab_id = tab.id.clone();
        let host_id = tab.host_id.clone();
        let Some(host_id) = host_id else {
            if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                tab.host_overview
                    .set_waiting_snapshot(HostOverviewSnapshot::waiting("未连接"));
            }
            return;
        };
        let Some(plan) = self.host_state.connection_plan_for(&host_id) else {
            if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                tab.host_overview
                    .set_waiting_snapshot(HostOverviewSnapshot::error(
                        "未知主机",
                        "无法读取主机连接配置",
                    ));
            }
            return;
        };

        let HostConnectionPlan::SavedSsh { config, title, .. } = plan else {
            if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                tab.host_overview
                    .set_waiting_snapshot(HostOverviewSnapshot::error(
                        "非 SSH 主机",
                        "当前连接暂不支持主机概览",
                    ));
            }
            return;
        };
        let host_label = format!(
            "{}@{}:{}",
            config.username.trim(),
            config.host.trim(),
            config.port
        );
        if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
            if !tab.host_overview.snapshot.has_collected_data() {
                let mut waiting = HostOverviewSnapshot::waiting(host_label.clone());
                waiting.hostname = Some(title);
                tab.host_overview.set_waiting_snapshot(waiting);
                tab.host_overview_network_item_states.borrow_mut().clear();
            }
        }

        match spawn_host_overview_monitor(
            Self::remote_ssh_config_from_host_config(&config),
            Duration::from_secs(3),
        ) {
            Ok((handle, receiver)) => {
                if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                    tab.host_overview_monitor = Some(handle);
                }
                ctx.spawn_stream_local(
                    receiver,
                    move |view, event, ctx| {
                        view.handle_host_overview_event_for_tab(&tab_id, event, ctx);
                    },
                    |_, _| {},
                );
            }
            Err(error) => {
                if let Some(tab) = self.terminal_tabs.get_mut(self.active_tab_index) {
                    tab.host_overview
                        .set_waiting_snapshot(HostOverviewSnapshot::error(host_label, error));
                }
            }
        }
    }

    fn should_render_host_overview_sidebar(&self) -> bool {
        should_show_host_overview_sidebar(
            self.sidebar_open,
            self.active_tab_supports_host_overview(),
        )
    }

    /// 查看伪 tab（CodeViewer / GitDiff）回溯到 host_id 指向的源终端 tab index；普通 tab 即 active 自身。
    /// 源终端标签可被单独关闭（close_terminal_tab 不级联删派生伪 tab），孤儿场景返回 None ——
    /// 文件面板 / git 面板据此统一惰性禁用，避免在孤儿伪 tab 上 toggle 状态或渲染破损空面板。
    pub(in crate::root_view) fn source_terminal_tab_index(&self) -> Option<usize> {
        let active_tab = self.terminal_tabs.get(self.active_tab_index)?;
        if matches!(
            active_tab.kind,
            TerminalSessionKind::GitDiff | TerminalSessionKind::CodeViewer
        ) {
            let source_tab_id = active_tab.host_id.as_deref()?;
            return self
                .terminal_tabs
                .iter()
                .position(|tab| tab.id == source_tab_id);
        }
        Some(self.active_tab_index)
    }

    // git 面板沿用同一查看伪 tab→源终端代理（语义聚焦 git，调用点可读）。
    fn active_git_panel_tab_index(&self) -> Option<usize> {
        self.source_terminal_tab_index()
    }

    // git 仅支持 Local；GitDiff tab 回溯到来源 tab 再判定
    fn active_tab_supports_git_panel(&self) -> bool {
        let Some(panel_index) = self.active_git_panel_tab_index() else {
            return false;
        };
        self.terminal_tabs
            .get(panel_index)
            .map(|tab| matches!(tab.kind, TerminalSessionKind::Local))
            .unwrap_or(false)
    }

    // 当前 git 面板（按 active_git_panel_tab_index 解析）是否处于展开态
    fn active_git_panel_open(&self) -> bool {
        self.active_git_panel_tab_index()
            .and_then(|idx| self.terminal_tabs.get(idx))
            .map(|tab| tab.git_panel_open)
            .unwrap_or(false)
    }

    fn should_render_git_panel(&self) -> bool {
        if matches!(
            self.terminal_tabs
                .get(self.active_tab_index)
                .map(|tab| tab.kind),
            Some(TerminalSessionKind::GitDiff) | Some(TerminalSessionKind::CodeViewer)
        ) && self.active_git_panel_tab_index().is_none()
        {
            return false;
        }
        // 仅 Local tab 允许展开 git 面板；open 现在按 tab 独立存
        self.active_tab_supports_git_panel() && self.active_git_panel_open()
    }

    fn schedule_idle_refresh(ctx: &mut ViewContext<Self>) {
        ctx.spawn(Timer::after(IDLE_REFRESH_INTERVAL), |me, _, ctx| {
            let now = Instant::now();
            let any_recording = me.terminal_tabs.iter().any(|tab| tab.is_recording());
            let recording_dirty = update_cursor_blink(&mut me.recording_blink, any_recording, now);
            let push_animating = me.terminal_tabs.iter().any(|tab| tab.git_push_busy);
            if push_animating {
                me.git_push_animation_tick = me.git_push_animation_tick.wrapping_add(1);
            }
            if recording_dirty || push_animating {
                ctx.notify();
            }
            Self::schedule_idle_refresh(ctx);
        });
    }

    fn sync_terminal_window_title(&mut self, title: Option<&str>, ctx: &mut ViewContext<Self>) {
        let next_title = terminal_window_title(title);
        if self.last_window_title == next_title {
            return;
        }

        ctx.windows().set_window_title(ctx.window_id(), next_title);
        self.last_window_title = next_title.to_string();
    }

    fn sync_titlebar_height(ctx: &mut ViewContext<Self>) {
        if let Some(platform_window) = ctx.windows().platform_window(ctx.window_id()) {
            platform_window
                .as_ref()
                .set_titlebar_height((TITLE_BAR_HEIGHT + TITLE_BAR_BORDER_HEIGHT) as f64);
        }
    }

    fn apply_window_opacity(_ctx: &mut ViewContext<Self>, opacity: u8) {
        #[cfg(target_os = "macos")]
        {
            let alpha = opacity as f64 / 100.0;
            macos_window_util::set_window_alpha(alpha);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = opacity;
    }

    fn report_terminal_focus(&self, focused: bool) {
        if let Ok(rt) = self.terminal.lock() {
            if let Some(bytes) =
                terminal_focus_report_bytes(focused, rt.snapshot().grid.input_modes)
            {
                rt.send_input(bytes);
            }
        }
    }

    fn flush_terminal_clipboard_requests(&self, ctx: &mut ViewContext<Self>) {
        let terminals: Vec<Arc<Mutex<LocalTerminalRuntime>>> =
            if let Some(tab) = self.terminal_tabs.get(self.active_tab_index) {
                tab.pane_terminals.values().cloned().collect()
            } else {
                vec![Arc::clone(&self.terminal)]
            };

        for terminal in &terminals {
            let store_requests = terminal
                .lock()
                .map(|rt| rt.take_clipboard_store_requests())
                .unwrap_or_default();
            for request in store_requests {
                ctx.clipboard()
                    .write(ClipboardContent::plain_text(request.text));
            }

            let load_requests = terminal
                .lock()
                .map(|rt| rt.take_clipboard_load_requests())
                .unwrap_or_default();
            if !load_requests.is_empty() {
                let clipboard_text = ctx.clipboard().read().plain_text;
                if let Ok(rt) = terminal.lock() {
                    for request in load_requests {
                        rt.send_input(request.response_bytes(&clipboard_text));
                    }
                }
            }
        }
    }
}

impl Entity for RootView {
    type Event = ();
}

impl View for RootView {
    fn ui_name() -> &'static str {
        "NexShellRootView"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        root_debug_key_log(format_args!(
            "root focus self_focused={} page={:?} active_tab={} focused_view={:?}",
            focus_ctx.is_self_focused(),
            self.app_page,
            self.active_tab_index,
            ctx.focused_view_id(ctx.window_id())
        ));
        if focus_ctx.is_self_focused() {
            self.report_terminal_focus(true);
            Self::sync_titlebar_height(ctx);
        }
    }

    fn on_blur(&mut self, blur_ctx: &BlurContext, _ctx: &mut ViewContext<Self>) {
        root_debug_key_log(format_args!(
            "root blur self_blurred={} page={:?} active_tab={}",
            blur_ctx.is_self_blurred(),
            self.app_page,
            self.active_tab_index
        ));
        if blur_ctx.is_self_blurred() {
            self.report_terminal_focus(false);
        }
    }

    fn active_cursor_position(&self, ctx: &ViewContext<Self>) -> Option<CursorInfo> {
        if self.app_page == AppPage::HostManagement {
            let focused = ctx.focused_view_id(ctx.window_id());
            if focused == Some(self.host_search_editor.id()) {
                let cursor_id = nexshell::text_editor::position_id_for_cursor(self.host_search_editor.id());
                let font_size = Appearance::as_ref(ctx).ui_font_size();
                return ctx
                    .element_position_by_id(cursor_id)
                    .map(|position| CursorInfo {
                        position,
                        font_size,
                    });
            }
            return None;
        }
        if let Some(cursor_info) = self.live_terminal_cursor_position() {
            return Some(cursor_info);
        }
        ctx.element_position_by_id(TERMINAL_CURSOR_POSITION_ID)
            .map(|position| CursorInfo {
                position,
                font_size: self.terminal_font_size,
            })
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let on_terminal_page = self.app_page == AppPage::Terminal;
        let mut tabs: Vec<TabModel> = self
            .terminal_tabs
            .iter()
            .enumerate()
            .map(|(index, _)| TabModel {
                label: self.terminal_tab_label(index),
                active: on_terminal_page && index == self.active_tab_index,
                is_settings: false,
            })
            .collect();
        if self.settings_tab_open {
            tabs.push(TabModel {
                label: rust_i18n::t!("menu_settings").to_string(),
                active: self.app_page == AppPage::Settings,
                is_settings: true,
            });
        }
        let title_bar = self.render_title_bar(&tabs, app);

        let body: Box<dyn Element> = if self.app_page == AppPage::HostManagement {
            self.render_host_management_page(app)
        } else if self.app_page == AppPage::Settings {
            self.render_settings_page(app)
        } else if self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| {
                matches!(t.kind, TerminalSessionKind::ProcessList)
            })
        {
            self.render_process_list_page(app)
        } else if self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| {
                matches!(t.kind, TerminalSessionKind::NetworkList)
            })
        {
            self.render_network_list_page(app)
        } else if self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| matches!(t.kind, TerminalSessionKind::SystemInfo))
        {
            self.render_system_info_page(app)
        } else if self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| matches!(t.kind, TerminalSessionKind::GitDiff))
        {
            let diff_page = self.render_git_diff_page(app);
            self.render_active_tab_body_with_side_panels(diff_page, app)
        } else if self
            .terminal_tabs
            .get(self.active_tab_index)
            .map_or(false, |t| matches!(t.kind, TerminalSessionKind::CodeViewer))
        {
            // 与 GitDiff 同属「内容 + 可选侧栏」类：整页查看器外仍保留文件/git/host 侧栏，
            // 否则打开文件后切到本 tab，文件面板按钮失效（toggle 了状态却无处渲染）。
            let code_viewer_page = self.render_code_viewer_page(app);
            self.render_active_tab_body_with_side_panels(code_viewer_page, app)
        } else {
            let active_tab = self.terminal_tabs.get(self.active_tab_index);
            let in_split = active_tab.map_or(false, |t| t.pane_tree.len() > 1);
            let maybe_maximized = self.maximized_pane;

            let terminal_content: Box<dyn Element> = if in_split && maybe_maximized.is_none() {
                self.render_split_terminal_body(app)
            } else {
                let render_terminal = if let Some(max_id) = maybe_maximized {
                    active_tab
                        .and_then(|t| t.pane_terminals.get(&max_id))
                        .cloned()
                        .unwrap_or_else(|| Arc::clone(&self.terminal))
                } else {
                    Arc::clone(&self.terminal)
                };
                let render_kind = active_tab
                    .map(|tab| tab.kind)
                    .unwrap_or(TerminalSessionKind::Local);
                self.render_single_terminal_body(&render_terminal, render_kind, app)
            };

            let mut terminal_stack =
                Stack::new().with_event_dispatch_mode(terminal_overlay_event_dispatch_mode());
            terminal_stack.add_child(terminal_content);
            if let Some(find_overlay) = self.render_find_bar() {
                terminal_stack.add_child(find_overlay);
            }

            self.render_active_tab_body_with_side_panels(terminal_stack.finish(), app)
        };

        let main_layout = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(title_bar)
            .with_child(Expanded::new(1.0, body).finish())
            .finish();

        // warp: workspace/view.rs:22848-22875 — 背景图片渲染
        let theme_for_bg = &self.cached_warp_theme;
        let mut root = Stack::new().with_event_dispatch_mode(root_overlay_event_dispatch_mode());
        if let Some(img) = theme_for_bg.background_image() {
            let opacity_ratio = img.opacity as f32 / 100.0;
            root.add_child(
                Shrinkable::new(
                    1.,
                    ImageElement::new(img.source(), CacheOption::Original)
                        .cover()
                        .with_opacity(opacity_ratio)
                        .finish(),
                )
                .finish(),
            );
        }
        // warp: 预热 settings + theme_chooser 的 layout/paint 缓存
        if !self.settings_prewarmed.get() && self.app_page != AppPage::Settings {
            self.settings_view_state.borrow_mut().theme_chooser_open = true;
            let warm = self.render_settings_page(app);
            self.settings_view_state.borrow_mut().theme_chooser_open = false;
            root.add_child(warm);
            self.settings_prewarmed.set(true);
        }
        let root_key_page = self.app_page;
        let root_key_active_tab_index = self.active_tab_index;
        let root_key_tab_count = self.terminal_tabs.len();
        let root_key_focused_pane = self
            .terminal_tabs
            .get(self.active_tab_index)
            .map(|tab| tab.focused_pane_id);
        let root_key_pane_count = self
            .terminal_tabs
            .get(self.active_tab_index)
            .map(|tab| tab.pane_tree.len())
            .unwrap_or(0);
        let root_key_maximized_pane = self.maximized_pane;
        let sweep_git_commit_hover = self
            .active_git_panel_tab_index()
            .and_then(|idx| self.terminal_tabs.get(idx))
            .map(|tab| tab.git_panel_open && tab.git_panel_hovered_commit.is_some())
            .unwrap_or(false);
        let main_layout = EventHandler::new(main_layout)
            .on_keydown(move |_, _, keystroke| {
                root_debug_key_log(format_args!(
                    "root keydown key={:?} mods(cmd={}, ctrl={}, alt={}, shift={}) page={:?} active_tab={}/{} focused_pane={:?} pane_count={} maximized_pane={:?}",
                    keystroke.key,
                    keystroke.cmd,
                    keystroke.ctrl,
                    keystroke.alt,
                    keystroke.shift,
                    root_key_page,
                    root_key_active_tab_index,
                    root_key_tab_count,
                    root_key_focused_pane,
                    root_key_pane_count,
                    root_key_maximized_pane
                ));
                DispatchEventResult::PropagateToParent
            })
            .finish();
        root.add_child(
            EventHandler::new(main_layout)
                .with_always_handle()
                .on_mouse_in(
                    move |ctx, _, _| {
                        if sweep_git_commit_hover {
                            ctx.notify_after(GIT_COMMIT_DETAIL_CLEAR_DELAY);
                            ctx.dispatch_typed_action(TerminalGridAction::GitCommitHoverSweep);
                        }
                        DispatchEventResult::PropagateToParent
                    },
                    None,
                )
                .finish(),
        );

        if self.new_session_menu_open {
            root.add_positioned_overlay_child(
                self.render_new_session_menu(),
                OffsetPositioning::offset_from_save_position_element(
                    NEW_TAB_BUTTON_POSITION_ID,
                    vec2f(0.0, 2.0),
                    PositionedElementOffsetBounds::WindowByPosition,
                    PositionedElementAnchor::BottomLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if self.settings_menu_open {
            root.add_positioned_overlay_child(
                self.render_settings_menu(),
                OffsetPositioning::offset_from_save_position_element(
                    SETTINGS_BUTTON_POSITION_ID,
                    vec2f(0.0, 2.0),
                    PositionedElementOffsetBounds::WindowByPosition,
                    PositionedElementAnchor::BottomRight,
                    ChildAnchor::TopRight,
                ),
            );
        }

        if let Some((_tab_index, TabContextMenuAnchor::Pointer(position))) =
            self.show_tab_right_click_menu
        {
            // warp/app/src/workspace/view.rs:22172-22187.
            root.add_positioned_overlay_child(
                self.render_tab_right_click_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    ParentOffsetBounds::Unbounded,
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(position) = self.show_terminal_context_menu {
            // warp: view.rs:16543-16576 (show_context_menu)
            root.add_positioned_overlay_child(
                self.render_terminal_context_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    terminal_context_menu_offset_bounds(),
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(position) = self.show_file_panel_context_menu {
            root.add_positioned_overlay_child(
                self.render_file_panel_context_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    terminal_context_menu_offset_bounds(),
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(position) = self.show_git_panel_context_menu {
            root.add_positioned_overlay_child(
                self.render_git_panel_context_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    terminal_context_menu_offset_bounds(),
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(position) = self.show_process_list_context_menu {
            root.add_positioned_overlay_child(
                self.render_process_list_context_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    terminal_context_menu_offset_bounds(),
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(position) = self.show_host_card_context_menu {
            root.add_positioned_overlay_child(
                self.render_host_card_context_menu(),
                OffsetPositioning::offset_from_parent(
                    position,
                    terminal_context_menu_offset_bounds(),
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        // git 提交详情卡：挂 root Stack（waterfall）才能遮挡下层终端，避免滚动/拖动穿透。
        if let Some((detail, position_id)) = self.render_git_commit_detail_overlay() {
            root.add_positioned_overlay_child(
                detail,
                OffsetPositioning::offset_from_save_position_element(
                    position_id,
                    vec2f(-6.0, 0.0),
                    PositionedElementOffsetBounds::WindowByPosition,
                    PositionedElementAnchor::TopLeft,
                    ChildAnchor::TopRight,
                ),
            );
        }

        root.finish()
    }
}

impl RootView {
    fn ui_colors(&self) -> UiColors {
        UiColors::from_theme(&self.cached_warp_theme)
    }
}

impl TypedActionView for RootView {
    type Action = TerminalGridAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        if self.edit_window_id.is_some() || self.manage_window_id.is_some() {
            return;
        }
        match action {
            // === 终端 / 查找 / 字体 ===
            TerminalGridAction::CopySelection => self.handle_copy_selection(ctx),
            TerminalGridAction::PasteClipboard => self.handle_paste_clipboard(ctx),
            TerminalGridAction::ClearVisibleScreen => self.handle_clear_visible_screen(ctx),
            TerminalGridAction::OpenFindBar => self.handle_open_find_bar(ctx),
            TerminalGridAction::CloseFindBar => self.close_find_bar(ctx),
            TerminalGridAction::FindStep(step) => self.handle_find_step(*step, ctx),
            TerminalGridAction::IncreaseFontSize => self.handle_increase_font_size(ctx),
            TerminalGridAction::DecreaseFontSize => self.handle_decrease_font_size(ctx),
            TerminalGridAction::ResetFontSize => self.handle_reset_font_size(ctx),

            // === 标题栏 Chrome（tab_bar_section）===
            TerminalGridAction::ToggleSidebar => self.handle_toggle_sidebar(ctx),
            TerminalGridAction::WindowMinimize => self.handle_window_minimize(ctx),
            TerminalGridAction::WindowToggleMaximize => self.handle_window_toggle_maximize(ctx),
            TerminalGridAction::WindowClose => self.handle_window_close(ctx),

            // === 主机监控（host_monitor_section）===
            TerminalGridAction::ToggleHostNetworkDropdown => {
                self.handle_toggle_host_network_dropdown(ctx)
            }
            TerminalGridAction::SelectHostNetwork(interface) => {
                self.handle_select_host_network(interface.clone(), ctx)
            }
            TerminalGridAction::SortHostProcesses(key) => {
                self.handle_sort_host_processes(*key, ctx)
            }
            TerminalGridAction::SortHostNetwork(key) => self.handle_sort_host_network(*key, ctx),
            TerminalGridAction::CopyHostAddress(text) => self.handle_copy_host_address(text, ctx),

            // === 新建标签 / 面板开关 ===
            TerminalGridAction::NewTab => self.handle_new_tab(ctx),
            TerminalGridAction::ToggleNewSessionMenu => self.handle_toggle_new_session_menu(ctx),
            TerminalGridAction::ToggleFilePanel => self.handle_toggle_file_panel(ctx),
            TerminalGridAction::ToggleGitPanel => self.handle_toggle_git_panel(ctx),

            // === Git 面板（git_panel_section）===
            TerminalGridAction::GitPanelRefresh => self.handle_git_panel_refresh(),
            TerminalGridAction::GitPanelSelectEntry { path, kind, mode } => {
                self.handle_git_panel_select_entry(path.clone(), *kind, *mode, ctx)
            }
            TerminalGridAction::GitPanelStage(path) => self.handle_git_panel_stage(path.clone()),
            TerminalGridAction::GitPanelStageAll(paths) => {
                self.handle_git_panel_stage_all(paths.clone())
            }
            TerminalGridAction::GitPanelStagePaths { tab_id, paths } => {
                self.show_git_panel_context_menu_close(ctx);
                if !paths.is_empty() {
                    self.send_git_request_to_tab(tab_id, GitRequest::Stage(paths.clone()));
                }
            }
            TerminalGridAction::GitPanelUnstage(path) => {
                self.handle_git_panel_unstage(path.clone())
            }
            TerminalGridAction::GitPanelUnstagePaths { tab_id, paths } => {
                self.show_git_panel_context_menu_close(ctx);
                if !paths.is_empty() {
                    self.send_git_request_to_tab(tab_id, GitRequest::Unstage(paths.clone()));
                }
            }
            TerminalGridAction::GitPanelAddToGitignore { tab_id, paths } => {
                self.show_git_panel_context_menu_close(ctx);
                if !paths.is_empty() {
                    self.send_git_request_to_tab(tab_id, GitRequest::AddToGitignore(paths.clone()));
                }
            }
            TerminalGridAction::GitPanelShowContextMenu {
                tab_id,
                path,
                kind,
                discard_enabled,
                position,
            } => {
                self.show_git_panel_context_menu(
                    tab_id.clone(),
                    path.clone(),
                    *kind,
                    *discard_enabled,
                    *position,
                    ctx,
                );
            }
            TerminalGridAction::GitPanelDiscardWorktreeChanges { tab_id, path } => {
                self.show_git_panel_context_menu_close(ctx);
                self.confirm_git_discard_worktree_change(tab_id.clone(), path.clone(), ctx);
            }
            TerminalGridAction::GitPanelDeleteUntracked { tab_id, path } => {
                self.show_git_panel_context_menu_close(ctx);
                self.confirm_git_delete_untracked(tab_id.clone(), path.clone(), ctx);
            }
            TerminalGridAction::GitPanelResizeStart(start_x) => {
                self.handle_git_panel_resize_start(*start_x)
            }
            TerminalGridAction::GitPanelResizeMove(current_x) => {
                self.handle_git_panel_resize_move(*current_x, ctx)
            }
            TerminalGridAction::GitPanelResizeEnd => self.handle_git_panel_resize_end(),
            TerminalGridAction::GitHistoryResizeStart(start_y) => {
                self.handle_git_history_resize_start(*start_y)
            }
            TerminalGridAction::GitHistoryResizeMove(current_y) => {
                self.handle_git_history_resize_move(*current_y, ctx)
            }
            TerminalGridAction::GitHistoryResizeEnd => self.handle_git_history_resize_end(),
            TerminalGridAction::GitHistoryScrolled {
                tab_id,
                scroll_start,
                delta_y,
            } => {
                self.handle_git_history_scrolled(tab_id, *scroll_start, *delta_y, ctx);
            }
            TerminalGridAction::GitCommitRowHover {
                tab_id,
                sha,
                hovered,
            } => {
                self.handle_git_commit_row_hover(tab_id, sha, *hovered, ctx);
            }
            TerminalGridAction::GitCommitDetailHover {
                tab_id,
                sha,
                hovered,
            } => {
                self.handle_git_commit_detail_hover(tab_id, sha, *hovered, ctx);
            }
            TerminalGridAction::GitCommitSelect { tab_id, sha } => {
                self.handle_git_commit_select(tab_id.clone(), sha.clone(), ctx)
            }
            TerminalGridAction::GitCommitHoverSweep => {
                self.sweep_git_commit_hover(ctx);
            }
            TerminalGridAction::GitCommitCopySha(sha) => {
                self.handle_git_commit_copy_sha(sha.clone(), ctx)
            }
            TerminalGridAction::GitCommitEditorFocus => self.handle_git_commit_editor_focus(ctx),
            TerminalGridAction::GitCommitConfirm => {
                self.run_git_commit(ctx);
            }
            TerminalGridAction::GitPushConfirm => {
                self.run_git_push(ctx);
            }

            // === 文件面板（file_panel_section）===
            TerminalGridAction::FilePanelRefresh => self.handle_file_panel_refresh(ctx),
            TerminalGridAction::FilePanelGoUp => self.handle_file_panel_go_up(),
            TerminalGridAction::FilePanelEnterDir(name) => {
                self.handle_file_panel_enter_dir(name.clone())
            }
            TerminalGridAction::FilePanelSelect { name, mode } => {
                self.handle_file_panel_select(name.clone(), *mode, ctx)
            }
            TerminalGridAction::FilePanelTreeItemClicked { path, is_dir, mode } => {
                self.handle_file_panel_tree_item_clicked(path.clone(), *is_dir, *mode, ctx)
            }
            TerminalGridAction::FilePanelDropFiles(paths) => {
                self.handle_file_panel_drop_files(paths.clone())
            }
            TerminalGridAction::FilePanelShowContextMenu {
                name,
                is_dir,
                position,
            } => {
                self.show_file_panel_context_menu(name.clone(), *is_dir, *position, ctx);
            }

            // === 主机库：右键 / 剪贴板 / 重命名（host_library_section）===
            TerminalGridAction::HostShowContextMenu { host_id, position } => {
                self.show_host_card_context_menu(host_id.clone(), *position, ctx);
            }
            TerminalGridAction::HostClipboardCopy(host_id) => {
                self.handle_host_clipboard_copy(host_id.clone(), ctx)
            }
            TerminalGridAction::HostClipboardCut(host_id) => {
                self.handle_host_clipboard_cut(host_id.clone(), ctx)
            }
            TerminalGridAction::HostClipboardPaste => self.handle_host_clipboard_paste(ctx),
            TerminalGridAction::HostRestoreDeleted => self.handle_host_restore_deleted(ctx),
            TerminalGridAction::HostRenameInline(host_id) => {
                self.handle_host_rename_inline(host_id.clone(), ctx)
            }

            // === 文件面板：传输 / 输入 / 缩放（file_panel_section）===
            TerminalGridAction::FilePanelDownload { name, is_dir } => {
                self.show_file_panel_context_menu_close(ctx);
                self.start_file_panel_download(name.clone(), *is_dir, ctx);
            }
            TerminalGridAction::FilePanelOpenUploadDialog => {
                self.start_file_panel_upload_dialog(ctx);
            }
            TerminalGridAction::FilePanelCancelTransfer(id) => {
                self.handle_file_panel_cancel_transfer(*id)
            }
            TerminalGridAction::FilePanelDelete { name, is_dir } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_delete(name.clone(), *is_dir);
            }
            TerminalGridAction::FilePanelStartRename { name } => {
                self.show_file_panel_context_menu_close(ctx);
                self.start_file_panel_input(
                    FilePanelInputIntent::Rename {
                        old_name: name.clone(),
                    },
                    file_panel_leaf_name(name),
                    ctx,
                );
            }
            TerminalGridAction::FilePanelStartNewDir => {
                self.show_file_panel_context_menu_close(ctx);
                self.start_file_panel_input(FilePanelInputIntent::NewDir, String::new(), ctx);
            }
            TerminalGridAction::FilePanelStartNewFile => {
                self.show_file_panel_context_menu_close(ctx);
                self.start_file_panel_input(FilePanelInputIntent::NewFile, String::new(), ctx);
            }
            TerminalGridAction::FilePanelStartNewFileIn { parent } => {
                self.show_file_panel_context_menu_close(ctx);
                self.start_file_panel_input(
                    FilePanelInputIntent::NewFileIn {
                        parent: parent.clone(),
                    },
                    String::new(),
                    ctx,
                );
            }
            TerminalGridAction::FilePanelSyncToTerminalCwd => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_sync_to_terminal_cwd(ctx);
            }
            TerminalGridAction::FilePanelCdToDirectory { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_cd_to_directory(path.clone());
            }
            TerminalGridAction::FilePanelOpenDirectoryInNewTab { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.open_local_terminal_tab_in_dir(std::path::PathBuf::from(path), ctx);
            }
            TerminalGridAction::FilePanelRevealInFileManager { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.reveal_local_file_panel_path(path, ctx);
            }
            TerminalGridAction::FilePanelOpenWithDefault { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_open_with_default(path, ctx);
            }
            TerminalGridAction::FilePanelOpenInEditor { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_open_in_editor(path, ctx);
            }
            TerminalGridAction::FilePanelOpenInCodeViewer { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_open_in_code_viewer(path.clone(), ctx);
            }
            TerminalGridAction::CodeViewerSave => self.handle_code_viewer_save(ctx),
            TerminalGridAction::FilePanelCopyPath { name } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_copy_path(name.clone(), ctx);
            }
            TerminalGridAction::FilePanelCopyRelativePath { path } => {
                self.show_file_panel_context_menu_close(ctx);
                self.handle_file_panel_copy_relative_path(path.clone(), ctx);
            }
            TerminalGridAction::FilePanelResizeStart(start_x) => {
                self.handle_file_panel_resize_start(*start_x)
            }
            TerminalGridAction::FilePanelResizeMove(current_x) => {
                self.handle_file_panel_resize_move(*current_x, ctx)
            }
            TerminalGridAction::FilePanelResizeEnd => self.handle_file_panel_resize_end(),

            // === 设置 / 外观（settings_section）===
            TerminalGridAction::ToggleSettingsMenu => self.handle_toggle_settings_menu(ctx),
            TerminalGridAction::ShowSettingsKeybindings => {
                self.handle_show_settings_keybindings(ctx)
            }
            TerminalGridAction::SettingsMenuWhatsNew
            | TerminalGridAction::SettingsMenuDocumentation
            | TerminalGridAction::SettingsMenuFeedback
            | TerminalGridAction::SettingsMenuViewLogs => self.handle_settings_menu_dismiss(ctx),
            TerminalGridAction::ShowSettings => self.handle_show_settings(ctx),
            TerminalGridAction::CloseSettingsTab => self.handle_close_settings_tab(ctx),
            TerminalGridAction::SettingsSelectPage(section) => {
                self.handle_settings_select_page(*section, ctx)
            }
            TerminalGridAction::SetTheme(choice) => self.handle_set_theme(*choice, ctx),
            TerminalGridAction::ShowThemeChooser => self.handle_show_theme_chooser(ctx),
            TerminalGridAction::CloseThemeChooser => self.handle_close_theme_chooser(ctx),
            TerminalGridAction::SetLanguage(choice) => self.handle_set_language(*choice, ctx),
            TerminalGridAction::SetTerminalFontSize(size) => {
                self.handle_set_terminal_font_size(*size, ctx)
            }
            TerminalGridAction::SetOpacity(value) => self.handle_set_opacity(*value, ctx),
            TerminalGridAction::SetCursorStyle(style) => self.handle_set_cursor_style(*style, ctx),
            TerminalGridAction::SetFontFamily(name) => {
                self.handle_set_font_family(name.clone(), ctx)
            }
            TerminalGridAction::SetFontWeight(weight) => self.handle_set_font_weight(*weight, ctx),
            TerminalGridAction::SetOpenFileEditor(choice) => {
                self.handle_set_open_file_editor(*choice, ctx)
            }
            TerminalGridAction::SetReuseViewTab(enabled) => {
                self.handle_set_reuse_view_tab(*enabled, ctx)
            }
            TerminalGridAction::SetLineHeight(ratio) => self.handle_set_line_height(*ratio, ctx),
            TerminalGridAction::ResetLineHeight => self.handle_reset_line_height(ctx),
            TerminalGridAction::ToggleViewAllFonts => self.handle_toggle_view_all_fonts(ctx),

            // === 标签页（tab_bar_section）===
            TerminalGridAction::SelectTab(index) => self.handle_select_tab(*index, ctx),
            TerminalGridAction::MoveTabLeft(index) => self.handle_move_tab_left(*index, ctx),
            TerminalGridAction::MoveTabRight(index) => self.handle_move_tab_right(*index, ctx),
            TerminalGridAction::RenameTab(index) => self.rename_terminal_tab(*index, ctx),
            TerminalGridAction::ResetTabName(index) => self.clear_terminal_tab_name(*index, ctx),
            TerminalGridAction::CloseTab(index) => self.handle_close_tab(*index, ctx),
            TerminalGridAction::CloseOtherTabs(index) => self.handle_close_other_tabs(*index, ctx),
            TerminalGridAction::CloseTabsRight(index) => self.handle_close_tabs_right(*index, ctx),
            TerminalGridAction::ReconnectTab(index) => self.handle_reconnect_tab(*index, ctx),
            TerminalGridAction::DisconnectTab(index) => self.handle_disconnect_tab(*index, ctx),
            TerminalGridAction::ToggleTabRecording(index) => {
                self.handle_toggle_tab_recording(*index, ctx)
            }
            TerminalGridAction::DuplicateTab(index) => self.handle_duplicate_tab(*index, ctx),
            TerminalGridAction::ToggleTabColor { color, tab_index } => {
                self.handle_toggle_tab_color(*color, *tab_index, ctx)
            }
            TerminalGridAction::ActivatePrevTab => self.handle_activate_prev_tab(ctx),
            TerminalGridAction::ActivateNextTab => self.handle_activate_next_tab(ctx),
            TerminalGridAction::ToggleTabRightClickMenu { tab_index, anchor } => {
                self.toggle_tab_right_click_menu(*tab_index, *anchor, ctx);
            }
            TerminalGridAction::TabHoverWidthStart { width } => {
                self.handle_tab_hover_width_start(*width, ctx)
            }
            TerminalGridAction::TabHoverWidthEnd => self.handle_tab_hover_width_end(ctx),
            TerminalGridAction::StartTabDrag => self.handle_start_tab_drag(ctx),
            TerminalGridAction::DragTab {
                tab_index,
                tab_position,
            } => self.on_tab_drag(*tab_index, *tab_position, ctx),
            TerminalGridAction::DropTab => self.handle_drop_tab(ctx),

            // === 终端鼠标 / 右键菜单（terminal_section）===
            TerminalGridAction::TerminalMouseDown => self.handle_terminal_mouse_down(ctx),
            TerminalGridAction::ShowTerminalContextMenu {
                position,
                has_selection,
            } => {
                self.show_terminal_context_menu(*position, *has_selection, ctx);
            }

            // === 主机库（host_library_section）===
            TerminalGridAction::HostQuickConnect(host_id) => {
                self.handle_host_quick_connect(host_id.clone(), ctx)
            }
            TerminalGridAction::HostToggleSelect(host_id) => {
                self.handle_host_toggle_select(host_id.clone(), ctx)
            }
            TerminalGridAction::HostSelectSingle(host_id) => {
                self.handle_host_select_single(host_id.clone(), ctx)
            }
            TerminalGridAction::HostToggleSelectAll => self.handle_host_toggle_select_all(ctx),
            TerminalGridAction::HostSelectGroup(group_id) => {
                self.handle_host_select_group(group_id.clone(), ctx)
            }
            TerminalGridAction::HostToggleTag(tag) => self.handle_host_toggle_tag(tag.clone(), ctx),
            TerminalGridAction::HostToggleProtocolDropdown => {
                self.handle_host_toggle_protocol_dropdown(ctx)
            }
            TerminalGridAction::HostSetProtocolFilter(filter) => {
                self.handle_host_set_protocol_filter(*filter, ctx)
            }
            TerminalGridAction::HostSetViewMode(mode) => self.handle_host_set_view_mode(*mode, ctx),
            TerminalGridAction::HostTogglePrivacy => self.handle_host_toggle_privacy(ctx),
            TerminalGridAction::HostRefresh => self.handle_host_refresh(ctx),
            TerminalGridAction::HostNewHost => self.handle_host_new_host(ctx),
            TerminalGridAction::HostEditSelected => self.handle_host_edit_selected(ctx),
            TerminalGridAction::HostEditOne(host_id) => {
                self.handle_host_edit_one(host_id.clone(), ctx)
            }
            TerminalGridAction::HostDeleteOne(host_id) => {
                self.handle_host_delete_one(host_id.clone(), ctx)
            }
            TerminalGridAction::HostDeleteSelected => self.handle_host_delete_selected(ctx),
            TerminalGridAction::HostConnectSelected => self.handle_host_connect_selected(ctx),
            TerminalGridAction::HostClearSelection => self.handle_host_clear_selection(ctx),
            TerminalGridAction::HostEnterReorderMode => self.handle_host_enter_reorder_mode(ctx),
            TerminalGridAction::HostExitReorderMode => self.handle_host_exit_reorder_mode(ctx),
            TerminalGridAction::HostStartCardDrag => self.handle_host_start_card_drag(ctx),
            TerminalGridAction::HostDragCard {
                host_id,
                card_position,
            } => self.handle_host_drag_card(host_id.clone(), *card_position, ctx),
            TerminalGridAction::HostDropCard => self.handle_host_drop_card(ctx),
            TerminalGridAction::HostManageGroupsTags => self.handle_host_manage_groups_tags(ctx),
            TerminalGridAction::HostImportKeyFile(path) => {
                self.handle_host_import_key_file(path.clone(), ctx)
            }
            TerminalGridAction::HostDeleteKey(key_id) => {
                self.handle_host_delete_key(key_id.clone(), ctx)
            }
            TerminalGridAction::HostSelectKey(key_id) => {
                self.handle_host_select_key(key_id.clone(), ctx)
            }
            TerminalGridAction::HostCopyKeyToServer => self.handle_host_copy_key_to_server(ctx),
            TerminalGridAction::HostEditKey => self.handle_host_edit_key(ctx),
            TerminalGridAction::HostKeyEditSave => self.handle_host_key_edit_save(ctx),
            TerminalGridAction::HostKeyEditCancel => self.handle_host_key_edit_cancel(ctx),
            TerminalGridAction::HostDeleteKeyPrompt => self.handle_host_delete_key_prompt(ctx),
            TerminalGridAction::HostDeleteKeyCancel => self.handle_host_delete_key_cancel(ctx),
            TerminalGridAction::HostCloudSync => self.handle_host_cloud_sync(ctx),
            TerminalGridAction::HostImport => self.handle_host_import(ctx),
            TerminalGridAction::HostExport => self.handle_host_export(ctx),
            TerminalGridAction::HostPasswordConfirm => self.handle_host_password_confirm(ctx),
            TerminalGridAction::HostPasswordCancel => self.handle_host_password_cancel(ctx),
            TerminalGridAction::ShowHostManagement => self.handle_show_host_management(ctx),

            // === 主机监控：进程 / 网络 / 系统（host_monitor_section）===
            TerminalGridAction::OpenProcessList => self.handle_open_process_list(ctx),
            TerminalGridAction::OpenNetworkList => self.handle_open_network_list(ctx),
            TerminalGridAction::OpenSystemInfo => self.handle_open_system_info(ctx),
            TerminalGridAction::ProcessListShowContextMenu {
                pid,
                command,
                args,
                exe_path,
                position,
            } => {
                self.show_process_list_context_menu(
                    *pid,
                    command.clone(),
                    args.clone(),
                    exe_path.clone(),
                    *position,
                    ctx,
                );
            }
            TerminalGridAction::KillRemoteProcess { pid, label } => {
                self.kill_remote_process(*pid, label.clone(), ctx);
            }

            // === 分屏（terminal_section）===
            TerminalGridAction::SplitRight => {
                self.split_active_pane(Direction::Right, ctx);
            }
            TerminalGridAction::SplitDown => {
                self.split_active_pane(Direction::Down, ctx);
            }
            TerminalGridAction::SplitLeft => {
                self.split_active_pane(Direction::Left, ctx);
            }
            TerminalGridAction::SplitUp => {
                self.split_active_pane(Direction::Up, ctx);
            }
            TerminalGridAction::ClosePane => {
                self.close_focused_pane(ctx);
            }
            TerminalGridAction::FocusPane(pane_id) => self.handle_focus_pane(*pane_id, ctx),
            TerminalGridAction::NavigatePaneLeft => {
                self.navigate_pane(Direction::Left, ctx);
            }
            TerminalGridAction::NavigatePaneRight => {
                self.navigate_pane(Direction::Right, ctx);
            }
            TerminalGridAction::NavigatePaneUp => {
                self.navigate_pane(Direction::Up, ctx);
            }
            TerminalGridAction::NavigatePaneDown => {
                self.navigate_pane(Direction::Down, ctx);
            }
            TerminalGridAction::StartPaneResizing(border) => {
                self.handle_start_pane_resizing(*border)
            }
            TerminalGridAction::PaneResizeMove(position) => {
                self.handle_pane_resize_move(*position, ctx)
            }
            TerminalGridAction::EndPaneResizing => self.handle_end_pane_resizing(),
            TerminalGridAction::ToggleMaximizePane => self.handle_toggle_maximize_pane(ctx),
        }
    }
}

impl RootView {
    fn reset_active_terminal_view_state(&mut self) {
        if let Ok(mut editor) = self.input_editor.lock() {
            editor.clear();
        }
        if let Ok(mut smooth_scroll_px) = self.smooth_scroll_px.lock() {
            *smooth_scroll_px = 0.0;
        }
        if let Ok(mut find_state) = self.find_state.lock() {
            *find_state = FindPanelState::default();
        }
        if let Ok(mut shaped_line_cache) = self.shaped_line_cache.lock() {
            *shaped_line_cache = TerminalShapedLineCache::default();
        }
        if let Ok(mut selection_drag) = self.selection_drag.lock() {
            *selection_drag = false;
        }
        if let Ok(rt) = self.terminal.lock() {
            rt.clear_marked_text();
            rt.set_find_query(None);
        }
    }

    fn next_local_terminal_session_id(&mut self) -> String {
        let session_id = format!("local-{}", self.next_terminal_tab_seq);
        self.next_terminal_tab_seq += 1;
        session_id
    }

    fn unique_terminal_tab_id(&self, preferred: &str) -> String {
        let preferred = preferred.trim();
        let base = if preferred.is_empty() {
            "terminal"
        } else {
            preferred
        };
        if !self.terminal_tabs.iter().any(|tab| tab.id == base) {
            return base.to_string();
        }

        for suffix in 2.. {
            let candidate = format!("{base}-{suffix}");
            if !self.terminal_tabs.iter().any(|tab| tab.id == candidate) {
                return candidate;
            }
        }
        unreachable!("unbounded suffix search always returns")
    }

    fn push_terminal_tab(
        &mut self,
        mut terminal: LocalTerminalRuntime,
        preferred_id: &str,
        fallback_label: String,
        kind: TerminalSessionKind,
        host_id: Option<String>,
        serial_port: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let id = self.unique_terminal_tab_id(preferred_id);
        Self::attach_terminal_streams(&mut terminal, Some(id.clone()), ctx);
        Self::attach_ssh_handle_stream(&mut terminal, id.clone(), ctx);
        let fallback_label = if fallback_label.trim().is_empty() {
            kind.default_label().to_string()
        } else {
            fallback_label
        };
        let terminal = Arc::new(Mutex::new(terminal));
        let fg_handle = terminal
            .lock()
            .ok()
            .map(|rt| rt.shell_is_foreground_handle())
            .unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
        let insert_index = new_tab_insert_index(
            self.terminal_tabs.len(),
            self.active_tab_index,
            NewTabPlacement::default(),
        );
        if let Ok(mut flags) = self.foreground_flags.lock() {
            if insert_index <= flags.len() {
                flags.insert(insert_index, Arc::clone(&fg_handle));
            } else {
                flags.push(Arc::clone(&fg_handle));
            }
        }
        let pane_id = NexPaneId::new();
        let mut pane_terminals = HashMap::new();
        pane_terminals.insert(pane_id, Arc::clone(&terminal));
        let git_commit_editor = ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let mut editor = EditorView::new(git_commit_editor_options(font_size), ctx);
            editor.set_placeholder_text(rust_i18n::t!("git_panel_commit_placeholder"), ctx);
            editor
        });
        ctx.subscribe_to_view(&git_commit_editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_git_commit_editor_event(event, ctx);
        });
        self.terminal_tabs.insert(
            insert_index,
            TerminalSessionTab {
                id,
                fallback_label: fallback_label.clone(),
                custom_label: None,
                kind,
                host_id,
                host_overview: HostOverviewUiState::waiting(
                    rust_i18n::t!("host_overview_not_connected").to_string(),
                ),
                host_overview_monitor: None,
                host_overview_network_dropdown_state: Arc::new(Mutex::new(MouseState::default())),
                host_overview_network_item_states: RefCell::new(HashMap::new()),
                host_overview_process_row_states: RefCell::new(HashMap::new()),
                host_overview_process_header_states: [
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                ],
                host_overview_copy_button_state: Arc::new(Mutex::new(MouseState::default())),
                host_overview_process_expand_state: Arc::new(Mutex::new(MouseState::default())),
                host_overview_network_expand_state: Arc::new(Mutex::new(MouseState::default())),
                host_overview_system_expand_state: Arc::new(Mutex::new(MouseState::default())),
                system_info_scroll_state: ClippedScrollStateHandle::default(),
                host_overview_disk_scroll_state: ClippedScrollStateHandle::default(),
                process_list_scroll_state: ClippedScrollStateHandle::default(),
                process_list_selected_pid: None,
                network_list_header_states: [
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                    Arc::new(Mutex::new(MouseState::default())),
                ],
                network_list_scroll_state: ClippedScrollStateHandle::default(),
                terminal: Arc::clone(&terminal),
                pane_tree: PaneData::new(pane_id),
                pane_terminals,
                focused_pane_id: pane_id,
                file_panel_open: false,
                file_panel_width: FILE_PANEL_WIDTH_DEFAULT,
                file_panel_state: FilePanelState::new(),
                sftp_worker: None,
                file_panel_entry_states: RefCell::new(HashMap::new()),
                file_panel_refresh_state: Arc::new(Mutex::new(MouseState::default())),
                file_panel_up_state: Arc::new(Mutex::new(MouseState::default())),
                file_panel_upload_state: Arc::new(Mutex::new(MouseState::default())),
                file_panel_scroll_state: ClippedScrollStateHandle::default(),
                file_panel_transfer_cancel_states: RefCell::new(BTreeMap::new()),
                ssh_handle: None,
                serial_port,
                git_panel_open: false,
                git_panel_state: GitPanelState::new(),
                git_worker: None,
                git_last_dispatched_cwd: None,
                git_panel_refresh_state: Arc::new(Mutex::new(MouseState::default())),
                git_panel_commit_state: Arc::new(Mutex::new(MouseState::default())),
                git_commit_editor_shell_state: Arc::new(Mutex::new(MouseState::default())),
                git_panel_push_state: Arc::new(Mutex::new(MouseState::default())),
                git_panel_stage_all_state: Arc::new(Mutex::new(MouseState::default())),
                git_panel_scroll_state: ClippedScrollStateHandle::default(),
                git_panel_diff_scroll_state: ClippedScrollStateHandle::default(),
                git_panel_history_scroll_state: ClippedScrollStateHandle::default(),
                git_panel_history_last_scroll_start: 0.0,
                git_panel_history_divider_state: Arc::new(Mutex::new(MouseState::default())),
                git_panel_history_divider_drag_state: {
                    let s = DraggableState::default();
                    s.set_suppress_overlay_paint(true);
                    s
                },
                git_panel_history_height: self.git_history_height,
                git_panel_entry_states: RefCell::new(HashMap::new()),
                git_panel_entry_action_states: RefCell::new(HashMap::new()),
                git_panel_commit_states: RefCell::new(HashMap::new()),
                git_panel_commit_detail_states: RefCell::new(HashMap::new()),
                git_panel_commit_copy_states: RefCell::new(HashMap::new()),
                git_panel_commit_detail_files_scroll_states: RefCell::new(HashMap::new()),
                git_panel_commit_detail_body_scroll_states: RefCell::new(HashMap::new()),
                git_panel_selected_commit: None,
                git_panel_hovered_commit: None,
                git_panel_hover_clear_after: None,
                git_commit_editor,
                git_commit_busy: false,
                git_push_busy: false,
                code_viewer: None,
                code_viewer_path: None,
                code_viewer_dirty: false,
                code_viewer_saved_content: None,
                code_viewer_ssh_handle: None,
                code_viewer_saving: false,
                code_viewer_remote_meta: None,
            },
        );
        self.tab_states
            .insert(insert_index, Arc::new(Mutex::new(MouseState::default())));
        self.tab_tooltip_states
            .insert(insert_index, Arc::new(Mutex::new(MouseState::default())));
        self.tab_close_states
            .insert(insert_index, Arc::new(Mutex::new(MouseState::default())));
        self.tab_draggable_states
            .insert(insert_index, DraggableState::default());
        self.tab_selected_colors.insert(insert_index, None);
        self.active_tab_index = insert_index;
        self.terminal = terminal;
        self.new_session_menu_open = false;
        self.settings_menu_open = false;
        self.app_page = AppPage::Terminal;
        ctx.focus_self();
        self.reset_active_terminal_view_state();
        self.sync_terminal_window_title(Some(&fallback_label), ctx);
        self.sync_host_overview_monitor(ctx);
    }

    fn open_local_terminal_tab(&mut self, ctx: &mut ViewContext<Self>) {
        let session_id = self.next_local_terminal_session_id();
        let (cols, rows) = self
            .last_resize_cells
            .lock()
            .map(|cells| *cells)
            .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
        let terminal = LocalTerminalRuntime::spawn_local_or_failed(&session_id, cols, rows);
        self.push_terminal_tab(
            terminal,
            &session_id,
            TerminalSessionKind::Local.default_label().to_string(),
            TerminalSessionKind::Local,
            None,
            None,
            ctx,
        );
    }

    fn open_local_terminal_tab_in_dir(
        &mut self,
        cwd: std::path::PathBuf,
        ctx: &mut ViewContext<Self>,
    ) {
        let session_id = self.next_local_terminal_session_id();
        let (cols, rows) = self
            .last_resize_cells
            .lock()
            .map(|cells| *cells)
            .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
        let terminal =
            LocalTerminalRuntime::spawn_local_in_dir_or_failed(&session_id, &cwd, cols, rows);
        self.push_terminal_tab(
            terminal,
            &session_id,
            TerminalSessionKind::Local.default_label().to_string(),
            TerminalSessionKind::Local,
            None,
            None,
            ctx,
        );
    }

    fn activate_terminal_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.terminal_tabs.get(index) else {
            return;
        };
        let terminal = tab
            .pane_terminals
            .get(&tab.focused_pane_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&tab.terminal));
        let title = tab.window_title();
        self.active_tab_index = index;
        self.terminal = terminal;
        self.app_page = AppPage::Terminal;
        ctx.focus_self();
        if let Ok(mut layout) = self.terminal_ime_layout.lock() {
            *layout = None;
        }
        self.reset_active_terminal_view_state();
        self.sync_terminal_window_title(Some(&title), ctx);
        self.sync_host_overview_monitor(ctx);
    }

    fn sync_active_terminal_after_tab_list_change(&mut self, ctx: &mut ViewContext<Self>) {
        if self.terminal_tabs.is_empty() {
            self.active_tab_index = 0;
            self.terminal = inactive_terminal_runtime();
            self.app_page = AppPage::HostManagement;
            self.reload_host_recent();
            self.sync_terminal_window_title(None, ctx);
            self.sync_host_overview_monitor(ctx);
            return;
        }

        self.active_tab_index = self.active_tab_index.min(self.terminal_tabs.len() - 1);
        let Some(active_tab) = self.terminal_tabs.get(self.active_tab_index) else {
            return;
        };
        let terminal = active_tab
            .pane_terminals
            .get(&active_tab.focused_pane_id)
            .cloned()
            .unwrap_or_else(|| Arc::clone(&active_tab.terminal));
        let title = active_tab.window_title();
        self.terminal = terminal;
        self.app_page = AppPage::Terminal;
        ctx.focus_self();
        self.reset_active_terminal_view_state();
        self.sync_terminal_window_title(Some(&title), ctx);
        self.sync_host_overview_monitor(ctx);
    }

    fn remove_terminal_tab_at(&mut self, index: usize) {
        self.terminal_tabs.remove(index);
        if let Ok(mut flags) = self.foreground_flags.lock() {
            if index < flags.len() {
                flags.remove(index);
            }
        }
        self.tab_states.remove(index);
        self.tab_tooltip_states.remove(index);
        self.tab_close_states.remove(index);
        self.tab_draggable_states.remove(index);
        self.tab_selected_colors.remove(index);
        if let Some(rename_index) = self.tab_being_renamed {
            self.tab_being_renamed = if rename_index == index {
                None
            } else if index < rename_index {
                Some(rename_index - 1)
            } else {
                Some(rename_index)
            };
        }
    }

    // warp: workspace/view.rs:11886-11950
    fn close_terminal_tab(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        // 未保存保护（ADR 0003）：dirty 的 CodeViewer 关闭前先弹确认，否则直接关。
        let needs_confirm = self.terminal_tabs.get(index).map_or(false, |t| {
            matches!(t.kind, TerminalSessionKind::CodeViewer) && t.code_viewer_dirty
        });
        if needs_confirm {
            let tab_id = self.terminal_tabs[index].id.clone();
            self.confirm_discard_code_viewer_close(tab_id, ctx);
            return;
        }
        self.close_terminal_tab_inner(index, ctx);
    }

    pub(in crate::root_view) fn close_terminal_tab_inner(
        &mut self,
        index: usize,
        ctx: &mut ViewContext<Self>,
    ) {
        if index >= self.terminal_tabs.len() {
            return;
        }

        self.tab_fixed_width = None;
        self.show_tab_right_click_menu = None;
        self.remove_terminal_tab_at(index);

        if self.terminal_tabs.is_empty() {
            self.active_tab_index = 0;
            self.terminal = inactive_terminal_runtime();
            self.app_page = AppPage::HostManagement;
            self.reload_host_recent();
            self.sync_terminal_window_title(None, ctx);
            return;
        }

        if self.active_tab_index >= self.terminal_tabs.len() {
            self.active_tab_index = self.terminal_tabs.len() - 1;
        } else if index < self.active_tab_index {
            self.active_tab_index -= 1;
        }

        self.sync_active_terminal_after_tab_list_change(ctx);
    }

    pub(crate) fn connect_host(&mut self, host_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(plan) = self.host_state.connection_plan_for(host_id) else {
            return;
        };
        self.record_host_access(host_id);

        match plan {
            HostConnectionPlan::SavedSsh {
                session_id,
                title,
                config,
            } => {
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
                let (cols, rows) = self
                    .last_resize_cells
                    .lock()
                    .map(|cells| *cells)
                    .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
                let status = format!(
                    "connecting SSH: {}@{}:{}",
                    config.username.trim(),
                    config.host.trim(),
                    config.port
                );
                let terminal = LocalTerminalRuntime::spawn_remote_ssh_or_failed(
                    &tab_session_id,
                    Self::remote_ssh_config_from_host_config(&config),
                    status,
                    cols,
                    rows,
                );
                self.push_terminal_tab(
                    terminal,
                    &tab_session_id,
                    title.clone(),
                    TerminalSessionKind::Remote,
                    Some(host_id.to_string()),
                    None,
                    ctx,
                );
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_connecting", title = title).to_string());
            }
            HostConnectionPlan::DirectPty {
                session_id,
                title,
                command,
            } => {
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
                let args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
                let (cols, rows) = self
                    .last_resize_cells
                    .lock()
                    .map(|cells| *cells)
                    .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
                let terminal = LocalTerminalRuntime::spawn_command_or_failed(
                    &tab_session_id,
                    &command.program,
                    &args,
                    command.status,
                    cols,
                    rows,
                );
                self.push_terminal_tab(
                    terminal,
                    &tab_session_id,
                    title.clone(),
                    TerminalSessionKind::Direct,
                    Some(host_id.to_string()),
                    None,
                    ctx,
                );
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_connecting", title = title).to_string());
            }
            HostConnectionPlan::Serial {
                session_id,
                title,
                config,
            } => {
                let Some(serial_port) = serial_port_from_host_config(&config) else {
                    self.host_state.notice = Some(
                        rust_i18n::t!("toast_connect_failed", title = title, reason = "串口为空")
                            .to_string(),
                    );
                    ctx.notify();
                    return;
                };
                if let Some(open_index) = self.open_serial_tab_index(&serial_port, None) {
                    self.activate_terminal_tab(open_index, ctx);
                    self.host_state.notice = Some(format!(
                        "串口 {serial_port} 已在标签页「{}」中打开",
                        self.terminal_tab_label(open_index)
                    ));
                    ctx.notify();
                    return;
                }
                let tab_session_id = self.unique_terminal_tab_id(&session_id);
                let (cols, rows) = self
                    .last_resize_cells
                    .lock()
                    .map(|cells| *cells)
                    .unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
                let terminal = LocalTerminalRuntime::spawn_serial_or_failed(
                    &tab_session_id,
                    Self::serial_config_from_host_config(&config),
                    Self::serial_status_from_host_config(&config),
                    cols,
                    rows,
                );
                self.push_terminal_tab(
                    terminal,
                    &tab_session_id,
                    title.clone(),
                    TerminalSessionKind::Serial,
                    Some(host_id.to_string()),
                    Some(serial_port),
                    ctx,
                );
                self.host_state.notice =
                    Some(rust_i18n::t!("toast_connecting", title = title).to_string());
            }
            HostConnectionPlan::Unsupported { title, reason } => {
                self.host_state.notice = Some(
                    rust_i18n::t!("toast_connect_failed", title = title, reason = reason)
                        .to_string(),
                );
            }
        }

        ctx.notify();
    }

    fn remote_ssh_config_from_host_config(config: &HostConnectionConfig) -> RemoteSshConfig {
        nexshell::host_overview::remote_ssh_config_from_host_config(config)
    }

    fn serial_config_from_host_config(config: &HostConnectionConfig) -> SerialPortRuntimeConfig {
        SerialPortRuntimeConfig {
            port: Self::serial_port_from_host_config(config),
            baud_rate: config.serial_baud_rate,
            data_bits: config.serial_data_bits,
            stop_bits: config.serial_stop_bits,
            parity: config.serial_parity.clone(),
            flow_control: config.serial_flow_control.clone(),
            dtr: config.serial_dtr,
            rts: config.serial_rts,
        }
    }

    fn serial_port_from_host_config(config: &HostConnectionConfig) -> String {
        config
            .serial_port
            .as_deref()
            .unwrap_or(config.host.as_str())
            .trim()
            .to_string()
    }

    fn serial_status_from_host_config(config: &HostConnectionConfig) -> String {
        format!(
            "opening serial: {} @ {}",
            Self::serial_port_from_host_config(config),
            config.serial_baud_rate
        )
    }

    fn set_terminal_font_size(&mut self, size: f32) {
        self.terminal_font_size = size.clamp(TERMINAL_FONT_SIZE_MIN, TERMINAL_FONT_SIZE_MAX);
    }

    fn save_ui_settings(&self) {
        save_ui_settings_to_disk(&UiSettings {
            sidebar_open: self.sidebar_open,
            theme: self.current_theme,
            font_size: self.terminal_font_size,
            line_height_ratio: self.line_height_ratio,
            git_history_height: self.git_history_height,
            opacity: self.window_opacity,
            cursor_style: self.cursor_style,
            font_family: self.monospace_font_name.clone(),
            font_weight: self.monospace_font_weight,
            language: self.language,
            open_file_editor: self.open_file_editor,
            reuse_view_tab: self.reuse_view_tab,
        });
    }
}
