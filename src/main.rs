//! NexShell WarpUI entrypoint.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

mod cursor_smear;
mod external_editor;
mod file_panel_view_helpers;
mod font_enumeration;
mod font_fallback;
mod git_commit_detail_helpers;
mod git_panel_row_helpers;
mod git_panel_view_helpers;
mod group_tag_manage_window;
mod host_edit_window;
mod host_export;
mod host_management_view;
mod host_monitor_view_helpers;
mod input_cursor;
#[cfg(target_os = "macos")]
mod macos_window_util;
mod settings_view;
mod terminal_grid_glass_dirty;
mod terminal_view_helpers;
mod throttle;
mod title_bar_chrome;
mod ui_colors;
mod ui_settings;
mod underline_decor;
mod warp_dropdown;
mod warp_dropdown_view;
mod warp_filterable_dropdown;

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;

use nexshell::text_editor::EditorView;
use pathfinder_geometry::vector::vec2f;
use warpui::actions::StandardAction;
use warpui::modals::{AlertDialogWithCallbacks, ModalButton};
use warpui::platform::app::{ApproveTerminateResult, TerminationRequestSource};
use warpui::platform::menu::{CustomMenuItem, Menu, MenuBar, MenuItem, MenuItemPropertyChanges};
use warpui::platform::TerminationMode;
use warpui::{
    elements::{
        ClippedScrollStateHandle, DraggableState, DropTargetData, MouseStateHandle,
        ScrollStateHandle, UniformListState,
    },
    fonts,
    keymap::{macros::id, FixedBinding},
    platform,
    platform::WindowBounds,
    AddWindowOptions, AppContext, AssetProvider, SingletonEntity as _, TypedActionView, View,
    ViewContext,
};

#[cfg(target_os = "macos")]
use warpui::platform::current::AppExt;

use nexshell::file_panel::{FilePanelState, FilePanelWorkerHandle};
use nexshell::generation::{accepts_generation, Generation, GenerationAllocator};
use nexshell::git_ops::GitStatusSnapshot;
use nexshell::git_panel::{GitPanelState, GitWorkerHandle};
use nexshell::host_overview::{HostOverviewMonitorHandle, HostOverviewUiState};
use nexshell::pane_state::NexPaneId;
use nexshell::pane_tree::PaneData;
use nexshell::ssh_session::SshHandle;
use nexshell::terminal_runtime::LocalTerminalRuntime;
use nexshell::ui_anim::FloatTransitionMap;
use nexshell::warp_tab_context_menu::{TAB_COLOR_ICON_PATH, TAB_NO_COLOR_ICON_PATH};

mod rdp_view;
mod root_view;
mod terminal_grid_element;
// RootView 定义已于 step 11 迁入 root_view/mod.rs；重导出以保持全库 `crate::RootView` 路径不变。
pub(crate) use root_view::RootView;
// main.rs 残留只剩启动装配与伴生类型，下列为这部分仍需的少量符号。
use font_enumeration::{
    default_monospace_font_family_name, load_nexshell_monospace_font, load_nexshell_ui_font,
};
use terminal_grid_element::TerminalGridAction;
use terminal_view_helpers::{terminal_clear_key_binding, terminal_tab_original_label};
use ui_settings::load_ui_settings;

const TERMINAL_CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const DEFAULT_WINDOW_TITLE: &str = "NexShell";
const DEFAULT_WINDOW_WIDTH: f32 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 768.0;
const DEFAULT_COLS: u16 = 100;
const DEFAULT_ROWS: u16 = 30;

// warp tab.rs:529 TAB_BAR_HEIGHT = 34。
const TITLE_BAR_HEIGHT: f32 = 34.0;
const TITLE_BAR_BORDER_HEIGHT: f32 = 1.0;
const TRAFFIC_LIGHT_RESERVED_WIDTH: f32 = 80.0;
const TAB_BAR_PADDING_LEFT: f32 = 4.0;
const TAB_BAR_PADDING_RIGHT: f32 = 8.0;
const WINDOWS_WINDOW_CONTROL_BUTTON_WIDTH: f32 = 46.0;
// warp buttons.rs:73-74 ICON_DIMENSIONS=24, padding=4 → icon 区 16x16。
const ICON_BUTTON_SIZE: f32 = 24.0;
const ICON_BUTTON_PADDING: f32 = 4.0;
// warp view.rs:17146 最后一个 left toolbar button margin_right(8.)。
const SIDEBAR_TOGGLE_MARGIN_RIGHT: f32 = 8.0;
// warp view.rs:17619-17622: BUTTON_HEIGHT=24, SIDE_MENU_WIDTH=16, BUTTON_WIDTH=24+16, BUTTON_LEFT_MARGIN=4。
const NEW_TAB_BUTTON_HEIGHT: f32 = 24.0;
const NEW_TAB_PLUS_WIDTH: f32 = 24.0;
const NEW_TAB_CHEVRON_WIDTH: f32 = 16.0;
const NEW_TAB_BUTTON_LEFT_MARGIN: f32 = 4.0;
// Warp tab.rs:1280 horizontal_padding(8.)。
const TAB_CONTENT_HORIZONTAL_PADDING: f32 = 8.0;
// Warp tab.rs:1419 vertical_padding(2.)。
const TAB_VERTICAL_PADDING: f32 = 2.0;
// Warp tab.rs:67 / 75。
const TAB_CLOSE_BUTTON_WIDTH: f32 = 20.0;
const TAB_CLOSE_BUTTON_HORIZONTAL_INSET: f32 = 2.0;
// warp_core/src/ui/appearance.rs:12 DEFAULT_UI_FONT_SIZE = 12.0。
const UI_FONT_SIZE: f32 = 12.0;
const WARP_2_TAB_COLOR_OPACITY: u8 = 25;
const WARP_2_HOVERED_TAB_COLOR_OPACITY: u8 = 50;

const WAKEUP_THROTTLE_PERIOD: Duration = Duration::from_micros(1_000_000 / 60);

const IDLE_REFRESH_INTERVAL: Duration = Duration::from_millis(100);

// Asset paths used by chrome icons. Logical, no directory prefix —
// EmbeddedAssets maps these to bytes baked in via `include_bytes!`.
const ICON_PATH_SIDEBAR_OPEN: &str = "icons/left-panel-open.svg";
const ICON_PATH_SIDEBAR_CLOSE: &str = "icons/left-panel-close.svg";
const ICON_PATH_PLUS: &str = "icons/plus.svg";
const ICON_PATH_CHEVRON_DOWN: &str = "icons/chevron-down.svg";
const ICON_PATH_CHEVRON_RIGHT: &str = "icons/chevron-right.svg";
const ICON_PATH_CLOSE: &str = "icons/close.svg";
const ICON_PATH_TERMINAL: &str = "icons/terminal.svg";
const ICON_PATH_GEAR: &str = "icons/gear.svg";
const ICON_PATH_SEARCH: &str = "icons/search.svg";
const ICON_PATH_REFRESH: &str = "icons/refresh.svg";
const ICON_PATH_GRID_VIEW: &str = "icons/grid-view.svg";
const ICON_PATH_LIST_VIEW: &str = "icons/list-view.svg";
const ICON_PATH_EYE: &str = "icons/eye.svg";
const ICON_PATH_EYE_OFF: &str = "icons/eye-off.svg";
const ICON_PATH_DOWNLOAD: &str = "icons/download.svg";
const ICON_PATH_UPLOAD: &str = "icons/upload.svg";
const ICON_PATH_CLOUD: &str = "icons/cloud.svg";
const ICON_PATH_LINUX: &str = "icons/linux.svg";
const ICON_PATH_SERIAL: &str = "icons/serial.svg";
const ICON_PATH_LINK: &str = "icons/link.svg";
const ICON_PATH_KEY: &str = "icons/key.svg";
const ICON_PATH_PLAY: &str = "icons/play.svg";
const ICON_PATH_SWAP: &str = "icons/swap.svg";
const ICON_PATH_ACTIVITY: &str = "icons/activity.svg";
const ICON_PATH_TRASH: &str = "icons/trash.svg";
const ICON_PATH_PENCIL: &str = "icons/pencil.svg";
const ICON_PATH_HOME: &str = "icons/home.svg";
const ICON_PATH_X_CIRCLE: &str = "icons/x-circle.svg";
const ICON_PATH_GIT_BRANCH: &str = "icons/git-branch.svg";
const ICON_PATH_GIT_LOCAL_REF: &str = "icons/git-local-ref.svg";
const ICON_PATH_ARROW_UP: &str = "icons/arrow-up.svg";
const ICON_PATH_ARROW_DOWN: &str = "icons/arrow-down.svg";
const ICON_PATH_FOLDER: &str = "icons/folder.svg";
const ICON_PATH_COPY: &str = "icons/copy.svg";
const ICON_PATH_EXPAND: &str = "icons/expand.svg";
const GIT_REF_BADGE_MAX_WIDTH: f32 = 150.0;
const GIT_COMMIT_DETAIL_CARD_WIDTH: f32 = 360.0;
/// 详情卡文件列表最大高度；超出走 ClippedScrollable 滚动，避免卡片无限拉高被窗口裁掉。
const GIT_COMMIT_DETAIL_FILES_MAX_HEIGHT: f32 = 220.0;
/// 详情卡提交正文最大高度；长正文同样走滚动，避免整卡超窗被裁。
const GIT_COMMIT_DETAIL_BODY_MAX_HEIGHT: f32 = 160.0;
const TAB_BAR_POSITION_ID: &str = "nexshell:tab_bar";
const NEW_TAB_BUTTON_POSITION_ID: &str = "nexshell:new_tab_btn";
const SETTINGS_BUTTON_POSITION_ID: &str = "nexshell:settings_btn";
const FILE_PANEL_BUTTON_POSITION_ID: &str = "nexshell:file_panel_btn";
const FILE_PANEL_WIDTH_DEFAULT: f32 = 260.0;
const FILE_PANEL_WIDTH_MIN: f32 = 200.0;
const FILE_PANEL_WIDTH_MAX: f32 = 640.0;
/// 左缘拖拽条命中区域宽度。Warp 的 pane divider 也是几像素的隐形 hit area。
const FILE_PANEL_DIVIDER_WIDTH: f32 = 6.0;
const SPLIT_PANE_HEADER_HEIGHT: f32 = 26.0;
const GIT_PANEL_BUTTON_POSITION_ID: &str = "nexshell:git_panel_btn";
const GIT_PANEL_WIDTH_DEFAULT: f32 = 280.0;
const GIT_PANEL_WIDTH_MIN: f32 = 220.0;
const GIT_PANEL_WIDTH_MAX: f32 = 640.0;
const GIT_PANEL_DIVIDER_WIDTH: f32 = 6.0;
const GIT_HISTORY_DIVIDER_HEIGHT: f32 = 6.0;
const GIT_COMMIT_EDITOR_MIN_HEIGHT: f32 = 30.0; // 默认一行（12pt×1.2 + 8+8 padding ≈ 30）
const GIT_COMMIT_EDITOR_MAX_HEIGHT: f32 = 120.0;

#[derive(Clone, Copy, Debug)]
enum TabBarLocation {
    AfterTabIndex(usize),
}

#[derive(Debug)]
struct TabBarDropTargetData {
    tab_bar_location: TabBarLocation,
}

impl DropTargetData for TabBarDropTargetData {
    fn as_any(&self) -> &dyn Any {
        match self.tab_bar_location {
            TabBarLocation::AfterTabIndex(index) => {
                let _ = index;
            }
        }
        self
    }
}

struct EmbeddedAssets;

impl AssetProvider for EmbeddedAssets {
    fn get(&self, path: &str) -> Result<Cow<'_, [u8]>> {
        let bytes: &'static [u8] = match path {
            ICON_PATH_SIDEBAR_OPEN => include_bytes!("../assets/svg/left-panel-open.svg"),
            ICON_PATH_SIDEBAR_CLOSE => include_bytes!("../assets/svg/left-panel-close.svg"),
            ICON_PATH_PLUS => include_bytes!("../assets/svg/plus.svg"),
            ICON_PATH_CHEVRON_DOWN => include_bytes!("../assets/svg/chevron-down.svg"),
            ICON_PATH_CHEVRON_RIGHT => include_bytes!("../assets/svg/chevron-right.svg"),
            ICON_PATH_CLOSE => include_bytes!("../assets/svg/close.svg"),
            ICON_PATH_TERMINAL => include_bytes!("../assets/svg/terminal.svg"),
            ICON_PATH_GEAR => include_bytes!("../assets/svg/gear.svg"),
            ICON_PATH_SEARCH => include_bytes!("../assets/svg/search.svg"),
            ICON_PATH_REFRESH => include_bytes!("../assets/svg/refresh.svg"),
            ICON_PATH_GRID_VIEW => include_bytes!("../assets/svg/grid-view.svg"),
            ICON_PATH_LIST_VIEW => include_bytes!("../assets/svg/list-view.svg"),
            ICON_PATH_EYE => include_bytes!("../assets/svg/eye.svg"),
            ICON_PATH_EYE_OFF => include_bytes!("../assets/svg/eye-off.svg"),
            ICON_PATH_DOWNLOAD => include_bytes!("../assets/svg/download.svg"),
            ICON_PATH_UPLOAD => include_bytes!("../assets/svg/upload.svg"),
            ICON_PATH_CLOUD => include_bytes!("../assets/svg/cloud.svg"),
            ICON_PATH_LINUX => include_bytes!("../assets/svg/linux.svg"),
            ICON_PATH_SERIAL => include_bytes!("../assets/svg/serial.svg"),
            ICON_PATH_LINK => include_bytes!("../assets/svg/link.svg"),
            ICON_PATH_KEY => include_bytes!("../assets/svg/key.svg"),
            ICON_PATH_PLAY => include_bytes!("../assets/svg/play.svg"),
            ICON_PATH_SWAP => include_bytes!("../assets/svg/swap.svg"),
            ICON_PATH_ACTIVITY => include_bytes!("../assets/svg/activity.svg"),
            ICON_PATH_TRASH => include_bytes!("../assets/svg/trash.svg"),
            ICON_PATH_PENCIL => include_bytes!("../assets/svg/pencil.svg"),
            ICON_PATH_X_CIRCLE => include_bytes!("../assets/svg/x-circle.svg"),
            ICON_PATH_ARROW_UP => include_bytes!("../assets/svg/arrow-up.svg"),
            ICON_PATH_ARROW_DOWN => include_bytes!("../assets/svg/arrow-down.svg"),
            ICON_PATH_FOLDER => include_bytes!("../assets/svg/folder.svg"),
            ICON_PATH_COPY => include_bytes!("../assets/svg/copy.svg"),
            ICON_PATH_EXPAND => include_bytes!("../assets/svg/expand.svg"),
            ICON_PATH_HOME => include_bytes!("../assets/svg/home.svg"),
            ICON_PATH_GIT_BRANCH => include_bytes!("../assets/svg/git-branch.svg"),
            ICON_PATH_GIT_LOCAL_REF => include_bytes!("../assets/svg/git-local-ref.svg"),
            "bundled/svg/check-thick.svg" => {
                include_bytes!("../assets/bundled/svg/check-thick.svg")
            }
            "bundled/svg/search-small.svg" => {
                include_bytes!("../assets/bundled/svg/search-small.svg")
            }
            "bundled/svg/chevron-down.svg" => {
                include_bytes!("../assets/bundled/svg/chevron-down.svg")
            }
            "bundled/svg/file.svg" => include_bytes!("../assets/bundled/svg/file.svg"),
            "bundled/svg/terminal.svg" => include_bytes!("../assets/bundled/svg/terminal.svg"),
            "bundled/svg/play-white.svg" => {
                include_bytes!("../assets/bundled/svg/play-white.svg")
            }
            "bundled/svg/refresh-cw-04.svg" => {
                include_bytes!("../assets/bundled/svg/refresh-cw-04.svg")
            }
            "bundled/svg/stop.svg" => include_bytes!("../assets/bundled/svg/stop.svg"),
            TAB_COLOR_ICON_PATH => {
                include_bytes!("../assets/bundled/svg/ellipse.svg")
            }
            TAB_NO_COLOR_ICON_PATH => {
                include_bytes!("../assets/bundled/svg/no_color_ellipse.svg")
            }
            "async/jpg/phenomenon_bg.jpg" => {
                include_bytes!("../assets/async/jpg/phenomenon_bg.jpg")
            }
            "async/jpg/jellyfish_bg.jpg" => {
                include_bytes!("../assets/async/jpg/jellyfish_bg.jpg")
            }
            "async/jpg/koi_bg.jpg" => {
                include_bytes!("../assets/async/jpg/koi_bg.jpg")
            }
            "async/jpg/leafy_bg.jpg" => {
                include_bytes!("../assets/async/jpg/leafy_bg.jpg")
            }
            "async/jpg/marble_bg.jpg" => {
                include_bytes!("../assets/async/jpg/marble_bg.jpg")
            }
            "async/jpg/pink_city_bg.jpg" => {
                include_bytes!("../assets/async/jpg/pink_city_bg.jpg")
            }
            "async/jpg/snowy_bg.jpg" => {
                include_bytes!("../assets/async/jpg/snowy_bg.jpg")
            }
            "async/jpg/red_rock_bg.jpg" => {
                include_bytes!("../assets/async/jpg/red_rock_bg.jpg")
            }
            "async/jpg/dark_city_bg.jpg" => {
                include_bytes!("../assets/async/jpg/dark_city_bg.jpg")
            }
            "async/jpg/sent_referral_reward_bg.jpg" => {
                include_bytes!("../assets/async/jpg/sent_referral_reward_bg.jpg")
            }
            "async/jpg/solarflare_bg.jpg" => {
                include_bytes!("../assets/async/jpg/solarflare_bg.jpg")
            }
            "async/jpg/received_referral_reward_bg.jpg" => {
                include_bytes!("../assets/async/jpg/received_referral_reward_bg.jpg")
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "EmbeddedAssets: no entry for path {}",
                    path
                ))
            }
        };
        Ok(Cow::Borrowed(bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AppPage {
    HostManagement,
    Terminal,
    Settings,
}

/// 文件面板 inline 输入栏的"意图"：决定 confirm 时往 worker 发什么请求。
#[derive(Clone, Debug)]
enum FilePanelInputIntent {
    Rename { old_name: String },
    NewDir,
    NewFile,
    NewFileIn { parent: String },
}

/// 主机库密码栏的两种用途：加密导出 vs 解密导入。
enum HostPasswordIntent {
    Export,
    Import { encrypted_bytes: Vec<u8> },
}

#[derive(Clone, Debug)]
struct TabModel {
    label: String,
    active: bool,
    is_settings: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalSessionKind {
    Local,
    Remote,
    Serial,
    Direct,
    ProcessList,
    NetworkList,
    SystemInfo,
    GitDiff,
    CodeViewer,
    Rdp,
}

impl TerminalSessionKind {
    fn default_label(self) -> String {
        match self {
            Self::Local => rust_i18n::t!("tab_local").to_string(),
            Self::Remote => rust_i18n::t!("tab_remote").to_string(),
            Self::Serial => rust_i18n::t!("tab_serial").to_string(),
            Self::Direct => rust_i18n::t!("tab_direct").to_string(),
            Self::ProcessList => rust_i18n::t!("tab_process_list").to_string(),
            Self::NetworkList => rust_i18n::t!("tab_network_list").to_string(),
            Self::SystemInfo => rust_i18n::t!("tab_system_info").to_string(),
            Self::GitDiff => rust_i18n::t!("tab_git_diff").to_string(),
            Self::CodeViewer => rust_i18n::t!("tab_code_viewer").to_string(),
            Self::Rdp => rust_i18n::t!("tab_rdp").to_string(),
        }
    }

    fn supports_terminal_recording(self) -> bool {
        matches!(
            self,
            Self::Local | Self::Remote | Self::Serial | Self::Direct
        )
    }
}

#[derive(Default)]
struct PanePresentationState {
    maximized_pane: Option<NexPaneId>,
}

impl PanePresentationState {
    fn maximized_pane(&self) -> Option<NexPaneId> {
        self.maximized_pane
    }

    fn clear_maximized(&mut self) {
        self.maximized_pane = None;
    }

    fn toggle_maximize(&mut self, pane_count: usize, focused_pane: NexPaneId) {
        if self.maximized_pane.is_some() {
            self.maximized_pane = None;
        } else if pane_count > 1 {
            self.maximized_pane = Some(focused_pane);
        }
    }
}

#[derive(Default)]
struct HostFleetSyncDebounce {
    pending: Option<Generation>,
}

impl HostFleetSyncDebounce {
    fn schedule(&mut self, allocator: &mut GenerationAllocator) -> Generation {
        let generation = allocator.allocate();
        self.pending = Some(generation);
        generation
    }

    fn accept(&mut self, generation: Generation) -> bool {
        if !accepts_generation(self.pending, generation) {
            return false;
        }
        self.pending = None;
        true
    }

    fn cancel(&mut self) {
        self.pending = None;
    }
}

/// RDP 整页 tab 的连接状态三态（连接中 / 已连接渲染帧 / 已断开）。
enum RdpConnectionPhase {
    Connecting,
    Connected,
    Disconnected { reason: String },
}

impl RdpConnectionPhase {
    fn on_transport_connected(&mut self) {
        // 首帧上传前继续显示连接态，避免稳定 asset key 暴露上一会话的缓存画面。
    }

    fn on_frame_uploaded(&mut self) {
        if matches!(self, Self::Connecting) {
            *self = Self::Connected;
        }
    }
}

/// RDP 会话随 tab 生命周期存活的状态。drop = 断开（RdpSessionHandle::Drop 优雅关闭），
/// 关 tab 时随 TerminalSessionTab 一并 drop，不走终端 shutdown 路径。
struct RdpTabState {
    /// 协议层句柄（framebuffer / 帧事件 / 输入通道占位）。
    handle: nexshell::rdp_session::RdpSessionHandle,
    /// 重连用连接参数（分辨率连接时定一次，重连沿用同一分辨率）。
    config: nexshell::rdp_session::RdpSessionConfig,
    session_generation: Generation,
    phase: RdpConnectionPhase,
    /// image cache 稳定键（每会话一个，逐帧覆盖同一条目，不堆积不泄漏）。
    asset_id: String,
    /// 已上传纹理对应的帧代号；新帧代号更大才重传，避免重复上传。
    last_uploaded_generation: u64,
    /// 当前 letterbox 几何（Element 每帧回写），第 ④ 步鼠标→远端坐标反算用。
    viewport: std::sync::Arc<std::sync::Mutex<Option<rdp_view::RdpViewport>>>,
    /// 上次发往远端的鼠标坐标；相同则不重发 MouseMove（Element 每帧重建，故存共享层）。
    /// 拖窗时 AppKit 会持续投递重复/近重复 LeftMouseDragged，去重防窗口在移动循环里抖动。
    last_mouse: std::sync::Arc<std::sync::Mutex<Option<(u16, u16)>>>,
    /// 修饰键持续对账器（与 page_element 共用）：跟踪已发 down 未发 up 的修饰键，
    /// 每个键鼠事件按本地 flags 对账补发丢失的 keyup，防远端「Alt 粘滞」卡键。
    mod_tracker: std::sync::Arc<std::sync::Mutex<rdp_view::keymap::ModifierTracker>>,
    /// 远端光标接管：当前应显示的本地光标（PointerChanged 时更新，画面 hover 时套用）。
    current_pointer: warpui::platform::Cursor,
    /// 远端指针备忘：cache_key → (注册时 scale, Cursor)。同指针反复切换不重复注册。
    pointer_cursor_cache: std::collections::HashMap<u64, (f32, warpui::platform::Cursor)>,
    /// 显示质量档：true=高清(HiDPI)，用于连接信息面板展示。
    hidpi: bool,
    /// 协议层运行时统计（Arc 与协议线程共享），连接信息面板只读差分。
    stats: Arc<nexshell::rdp_session::RdpStats>,
    /// 连接信息浮层开关。
    conn_info_open: bool,
    /// 上次采样 (bytes, frames, 时刻)，与下一 tick 差分算率。
    conn_info_last_sample: Option<(u64, u64, std::time::Instant)>,
    /// 最近算出的接收码率 Mbps / 发布帧率 fps（渲染直接用）。
    conn_info_mbps: f64,
    conn_info_fps: f64,
    /// 动态分辨率防抖：窗口尺寸/全屏变化稳定后才请求远端重设分辨率。
    resize_debounce: rdp_view::ResizeDebounce,
}

struct TerminalSessionTab {
    id: String,
    fallback_label: String,
    custom_label: Option<String>,
    kind: TerminalSessionKind,
    host_id: Option<String>,
    host_overview: HostOverviewUiState,
    host_overview_monitor: Option<HostOverviewMonitorHandle>,
    host_overview_network_dropdown_state: MouseStateHandle,
    host_overview_network_item_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// 概览侧栏进程行 hover 状态，按 pid 索引；每帧按可见 pid 清理。
    host_overview_process_row_states: RefCell<HashMap<u32, MouseStateHandle>>,
    host_overview_process_header_states: [MouseStateHandle; 6],
    host_overview_copy_button_state: MouseStateHandle,
    host_overview_process_expand_state: MouseStateHandle,
    host_overview_network_expand_state: MouseStateHandle,
    host_overview_system_expand_state: MouseStateHandle,
    /// 概览 CPU/Mem/Swap 环 + Load 三环的 sweep 数值过渡，按 key 索引。
    host_overview_gauge_anim: RefCell<FloatTransitionMap<String>>,
    system_info_scroll_state: ClippedScrollStateHandle,
    host_overview_disk_scroll_state: ClippedScrollStateHandle,
    process_list_scroll_state: ClippedScrollStateHandle,
    /// 右键菜单针对的进程 pid；菜单关闭后清空。供进程列表行高亮。
    process_list_selected_pid: Option<u32>,
    network_list_header_states: [MouseStateHandle; 8],
    network_list_scroll_state: ClippedScrollStateHandle,
    terminal: Arc<Mutex<LocalTerminalRuntime>>,
    pane_tree: PaneData,
    pane_terminals: HashMap<NexPaneId, Arc<Mutex<LocalTerminalRuntime>>>,
    focused_pane_id: NexPaneId,
    pane_presentation: PanePresentationState,
    file_panel_open: bool,
    file_panel_width: f32,
    file_panel_state: FilePanelState,
    /// 后台文件 worker。远程 tab 走 SFTP，本地 tab 走本机文件系统。
    sftp_worker: Option<FilePanelWorkerHandle>,
    /// 文件项 hover 状态，按名字索引。
    file_panel_entry_states: RefCell<HashMap<String, MouseStateHandle>>,
    file_panel_refresh_state: MouseStateHandle,
    file_panel_up_state: MouseStateHandle,
    file_panel_upload_state: MouseStateHandle,
    file_panel_scroll_state: ClippedScrollStateHandle,
    /// 每个传输行的取消按钮 hover 状态，按 transfer_id 索引。
    file_panel_transfer_cancel_states: RefCell<BTreeMap<u64, MouseStateHandle>>,
    /// 主 SSH handle clone（认证成功后由 terminal_runtime 推过来）。
    /// 文件面板拿它直接开 SFTP channel，跟 PTY 共享 TCP 连接（C 方案）。
    ssh_handle: Option<SshHandle>,
    /// Native serial sessions are exclusive; store the normalized port so a
    /// second tab cannot claim the same device while this tab is alive.
    serial_port: Option<String>,
    /// git 面板展开开关（每 tab 独立；与 file_panel_open 平行）。
    /// 仅 Local tab 会真正渲染；其它 kind 即使为 true 也不显示按钮/面板。
    git_panel_open: bool,
    /// 本地 git 面板状态。远程 / 串口 tab 也持有但永远不更新（无 OSC 7 上报）。
    git_panel_state: GitPanelState,
    /// 本地 git worker（lazy 启动：首次拿到 local_cwd 时 spawn）。
    git_worker: Option<GitWorkerHandle>,
    /// worker 上一次"已派发"的 cwd，避免重复 SetCwd。
    git_last_dispatched_cwd: Option<PathBuf>,
    git_panel_refresh_state: MouseStateHandle,
    git_panel_commit_state: MouseStateHandle,
    git_commit_editor_shell_state: MouseStateHandle,
    git_panel_push_state: MouseStateHandle,
    git_panel_stage_all_state: MouseStateHandle,
    /// 文件列表虚拟化：UniformList 只按可见 range 构建行；scrollbar 状态单独持有。
    git_panel_list_state: UniformListState,
    git_panel_scrollbar_state: ScrollStateHandle,
    /// 上一帧渲染用过的快照（Arc::ptr_eq 比较）：快照没变就跳过 hover map retain。
    git_panel_pruned_status: RefCell<Option<Arc<GitStatusSnapshot>>>,
    git_panel_diff_scroll_state: ClippedScrollStateHandle,
    git_panel_history_scroll_state: ClippedScrollStateHandle,
    git_panel_history_last_scroll_start: f32,
    git_panel_history_divider_state: MouseStateHandle,
    git_panel_history_divider_drag_state: DraggableState,
    git_panel_history_height: f32,
    /// 文件行 hover 状态，按 staged/worktree 分组 + 路径索引。Rc：UniformList build_items 闭包需 'static 持有。
    git_panel_entry_states: Rc<RefCell<HashMap<String, MouseStateHandle>>>,
    /// 文件行 stage/unstage 按钮 hover 状态。
    git_panel_entry_action_states: Rc<RefCell<HashMap<String, MouseStateHandle>>>,
    /// commit 行 hover 状态，按短 SHA 索引。
    git_panel_commit_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// commit 详情卡 hover 状态，按短 SHA 索引；用于从行移动到卡片时保持显示。
    git_panel_commit_detail_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// commit 详情卡复制按钮状态，按短 SHA 索引；确保 mouse-down/up 之间状态稳定。
    git_panel_commit_copy_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// commit 详情卡文件列表纵向滚动状态，按短 SHA 索引；文件多时滚动查看。
    git_panel_commit_detail_files_scroll_states: RefCell<HashMap<String, ClippedScrollStateHandle>>,
    /// commit 详情卡提交正文纵向滚动状态，按短 SHA 索引；长正文滚动查看。
    git_panel_commit_detail_body_scroll_states: RefCell<HashMap<String, ClippedScrollStateHandle>>,
    /// 点击选中的 commit 短 SHA；只用于行高亮，详情卡生命周期仍跟随 hover。
    git_panel_selected_commit: Option<String>,
    /// 当前显示详情卡的 commit 短 SHA。
    git_panel_hovered_commit: Option<String>,
    /// 鼠标离开 commit 行和详情卡后，延迟到这个时间点再隐藏详情。
    git_panel_hover_clear_after: Option<Instant>,
    /// inline commit message 输入框（每 tab 独立，切 tab 保留草稿）。
    git_commit_editor: warpui::ViewHandle<EditorView>,
    /// 正在提交中：禁用按钮 + editor，避免重复点击。
    git_commit_busy: bool,
    /// 正在推送中：禁用按钮，避免重复点击。
    git_push_busy: bool,
    /// 只读代码查看器视图句柄（仅 CodeViewer tab 持有；本地化 code_editor::CodeEditorView）。
    code_viewer: Option<warpui::ViewHandle<nexshell::code_editor::CodeEditorView>>,
    /// 查看器当前展示的本地文件绝对路径（复用判断 + 重载用）。
    code_viewer_path: Option<String>,
    /// 是否有未保存改动（= 当前 text != 已保存基线）；驱动标签脏圆点 + 关闭/换文件确认。
    code_viewer_dirty: bool,
    /// 已保存基线内容（打开 / 保存时刷新）；脏判定的对比基准。
    code_viewer_saved_content: Option<String>,
    /// 远程编辑器的 SSH handle clone（ADR 0005）；Some=远程文件经 SFTP 读写，None=本地走 fs。
    code_viewer_ssh_handle: Option<SshHandle>,
    /// 当前远程保存操作；completion 必须携带同一 generation 才能回写此 tab。
    code_viewer_save_generation: Option<Generation>,
    /// 远程文件基线元数据 (size, modified)；保存前 re-stat 对比做冲突检测。modified 可能缺失（服务端未报）。
    code_viewer_remote_meta: Option<(u64, Option<std::time::SystemTime>)>,
    /// RDP 整页会话状态（仅 Rdp kind 持有；drop 即断开）。
    rdp: Option<RdpTabState>,
}

impl TerminalSessionTab {
    fn label(&self) -> String {
        if let Some(custom_label) = &self.custom_label {
            return custom_label.clone();
        }
        self.original_label()
    }

    fn original_label(&self) -> String {
        let runtime_title = self
            .terminal
            .lock()
            .ok()
            .and_then(|rt| rt.title())
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty());
        terminal_tab_original_label(self.kind, &self.fallback_label, runtime_title.as_deref())
    }

    fn window_title(&self) -> String {
        self.label()
    }

    fn code_viewer_is_saving(&self) -> bool {
        self.code_viewer_save_generation.is_some()
    }

    /// 任一 pane 在录制即视为录制中（驱动标签红点）。
    fn is_recording(&self) -> bool {
        self.pane_terminals
            .values()
            .chain(std::iter::once(&self.terminal))
            .any(|rt| rt.lock().map_or(false, |rt| rt.is_recording()))
    }

    /// 远程/串口任一会话断开即视为离线（驱动标签红点）。
    fn is_disconnected(&self) -> bool {
        matches!(
            self.kind,
            TerminalSessionKind::Remote | TerminalSessionKind::Serial
        ) && self
            .pane_terminals
            .values()
            .chain(std::iter::once(&self.terminal))
            .any(|rt| rt.lock().map_or(false, |rt| !rt.is_connected()))
    }

    /// 远程/串口且任一会话仍连接：决定是否显示「断开连接」菜单项。
    fn can_disconnect(&self) -> bool {
        matches!(
            self.kind,
            TerminalSessionKind::Remote | TerminalSessionKind::Serial
        ) && self
            .pane_terminals
            .values()
            .chain(std::iter::once(&self.terminal))
            .any(|rt| rt.lock().map_or(false, |rt| rt.is_connected()))
    }
}

#[derive(Clone, Copy, Debug)]
struct CursorBlinkState {
    phase_visible: bool,
    last_toggled_at: Option<Instant>,
}

impl Default for CursorBlinkState {
    fn default() -> Self {
        Self {
            phase_visible: true,
            last_toggled_at: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TabMoveDirection {
    Left,
    Right,
}

fn register_terminal_key_bindings(ctx: &mut AppContext) {
    let view_id = id!(RootView::ui_name());
    ctx.register_fixed_bindings([
        FixedBinding::new(
            terminal_clear_key_binding(),
            TerminalGridAction::ClearVisibleScreen,
            view_id.clone(),
        ),
        FixedBinding::new("cmd-d", TerminalGridAction::SplitRight, view_id.clone()),
        FixedBinding::new(
            "cmd-shift-D",
            TerminalGridAction::SplitDown,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-shift-W",
            TerminalGridAction::ClosePane,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-alt-left",
            TerminalGridAction::NavigatePaneLeft,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-alt-right",
            TerminalGridAction::NavigatePaneRight,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-alt-up",
            TerminalGridAction::NavigatePaneUp,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-alt-down",
            TerminalGridAction::NavigatePaneDown,
            view_id.clone(),
        ),
        FixedBinding::new(
            "cmd-shift-enter",
            TerminalGridAction::ToggleMaximizePane,
            view_id.clone(),
        ),
        // 代码编辑器保存（ADR 0003）：全局绑定，handler 内仅 active 为 CodeViewer 时落盘。
        FixedBinding::new("cmd-s", TerminalGridAction::CodeViewerSave, view_id),
    ]);
}

fn dispatch_to_root_view(
    ctx: &mut AppContext,
    f: impl FnOnce(&mut RootView, &mut ViewContext<RootView>),
) {
    let Some(window_id) = ctx.window_ids().into_iter().next() else {
        return;
    };
    let Some(handle) = ctx.root_view::<RootView>(window_id) else {
        return;
    };
    handle.update(ctx, f);
}

/// 关窗 / 退出 app 回调里读 RootView 是否有未保存的 CodeViewer（审查 #1）。
fn app_has_unsaved_code_viewer(ctx: &mut AppContext) -> bool {
    let mut has_unsaved = false;
    dispatch_to_root_view(ctx, |view, _| {
        has_unsaved = view.has_unsaved_code_viewer();
    });
    has_unsaved
}

fn register_menu_global_actions(ctx: &mut AppContext) {
    ctx.add_global_action("nexshell:find", |_: &(), ctx| {
        dispatch_to_root_view(ctx, |view, ctx| {
            view.handle_action(&TerminalGridAction::OpenFindBar, ctx);
        });
    });
    ctx.add_global_action("nexshell:copy", |_: &(), ctx| {
        dispatch_to_root_view(ctx, |view, ctx| {
            view.handle_action(&TerminalGridAction::CopySelection, ctx);
        });
    });
    ctx.add_global_action("nexshell:new_tab", |_: &(), ctx| {
        dispatch_to_root_view(ctx, |view, ctx| {
            view.handle_action(&TerminalGridAction::NewTab, ctx);
        });
    });
    ctx.add_global_action("nexshell:close_tab", |_: &(), ctx| {
        dispatch_to_root_view(ctx, |view, ctx| {
            view.handle_action(&TerminalGridAction::CloseTab(view.active_tab_index), ctx);
        });
    });
}

fn register_warp_text_input_stack(ctx: &mut AppContext) {
    ctx.add_singleton_model(|_| {
        settings::PublicPreferences::new(Box::<
            warpui_extras::user_preferences::in_memory::InMemoryPreferences,
        >::default())
    });
    ctx.add_singleton_model(|_| {
        settings::PrivatePreferences::new(Box::<
            warpui_extras::user_preferences::in_memory::InMemoryPreferences,
        >::default())
    });
    ctx.add_singleton_model(|_| settings::manager::SettingsManager::default());

    nexshell::text_editor::settings::AppEditorSettings::register(ctx);
    // 代码查看器复用的 CodeEditorView::new 依赖 FontSettings 单例（ADR 0002）。
    nexshell::text_editor::settings::FontSettings::register(ctx);
    nexshell::text_editor::settings::SelectionSettings::register(ctx);
    warp_core::semantic_selection::SemanticSelection::register(ctx);
    register_warp_appearance(ctx);
    ctx.add_singleton_model(|_| nexshell::util::bindings::KeybindingChangedNotifier::new());
    nexshell::menu::init(ctx);
    nexshell::text_editor::init(ctx);
    // CodeEditorView 自己的 action 键绑定（方向键/退格/删除/选择等）；可编辑后必需，
    // 单行 editor::init 不含（ADR 0003）。否则只有 IME 字符输入可用、方向键/功能键全失效。
    nexshell::code_editor::init_code_editor_view(ctx);
    // 查找栏键盘导航（cmd-g / cmd-shift-G 跳下一处/上一处）：warp 在 lib.rs:1707 独立调用
    // find::view::init（与 init_code_editor_view 是两个 init），解耦时漏调，致快捷键失效。
    nexshell::code_editor::find::view::init(ctx);
}

fn register_warp_appearance(ctx: &mut AppContext) {
    let ui_settings = load_ui_settings();
    let (monospace_font, ui_font, password_font) =
        fonts::Cache::handle(ctx).update(ctx, |cache, _| {
            let monospace_font =
                load_nexshell_monospace_font(cache, Some(&ui_settings.font_family));
            let ui_font = load_nexshell_ui_font(cache).unwrap_or(monospace_font);
            let password_font = cache
                .load_family_from_bytes(
                    "PasswordCircle",
                    vec![include_bytes!("../assets/bundled/fonts/password.ttf").to_vec()],
                )
                .expect("password font");
            (monospace_font, ui_font, password_font)
        });

    ctx.add_singleton_model(move |_| {
        warp_core::ui::appearance::Appearance::new(
            nexshell::themes::default_themes::dark_theme(),
            monospace_font,
            ui_settings.font_size,
            ui_settings.font_weight,
            ui_font,
            ui_settings.line_height_ratio,
            monospace_font,
            password_font,
        )
    });

    // CJK fallback：base 字体（Hack/Roboto）缺中文字形时，给 warpui 文本系统（code_viewer）
    // 提供系统中文字体 fallback。解耦删 warp app 时漏搬此注册，致 code_viewer 中文乱码。
    ctx.set_fallback_font_fn(font_fallback::fallback_font_fn);
    ctx.set_fallback_font_source_provider(font_fallback::fallback_source);
}

fn warp_text_input_custom_tag_to_keystroke(
    custom: warpui::keymap::CustomTag,
) -> Option<warpui::keymap::Keystroke> {
    nexshell::util::bindings::custom_tag_to_keystroke(custom)
}

fn configure_warp_text_input_custom_action_key_bindings(app_builder: &mut platform::AppBuilder) {
    app_builder
        .convert_custom_triggers_to_keystroke_triggers(warp_text_input_custom_tag_to_keystroke);
    app_builder.register_default_keystroke_triggers_for_custom_actions(
        warp_text_input_custom_tag_to_keystroke,
    );
}

fn nexshell_menu_bar(_ctx: &mut AppContext) -> MenuBar {
    let app_menu = Menu::new(
        "NexShell",
        vec![
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("about_app"),
                |_ctx| {
                    #[cfg(target_os = "macos")]
                    macos_window_util::show_about_panel();
                },
                |_, _| MenuItemPropertyChanges::default(),
                None,
            )),
            MenuItem::Separator,
            #[cfg(target_os = "macos")]
            MenuItem::Services,
            #[cfg(target_os = "macos")]
            MenuItem::Separator,
            MenuItem::Standard(StandardAction::HideOtherApps),
            MenuItem::Standard(StandardAction::ShowAllApps),
            MenuItem::Separator,
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("quit_app"),
                |ctx| {
                    ctx.terminate_app(TerminationMode::Cancellable, None);
                },
                |_, _| MenuItemPropertyChanges::default(),
                Some(warpui::keymap::Keystroke::parse("cmd-q").unwrap()),
            )),
        ],
    );

    let file_menu = Menu::new(
        "File",
        vec![
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("menu_new_tab"),
                |ctx| {
                    ctx.dispatch_global_action("nexshell:new_tab", &());
                },
                |_, _| MenuItemPropertyChanges::default(),
                Some(warpui::keymap::Keystroke::parse("cmd-t").unwrap()),
            )),
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("menu_close_tab"),
                |ctx| {
                    ctx.dispatch_global_action("nexshell:close_tab", &());
                },
                |_, _| MenuItemPropertyChanges::default(),
                Some(warpui::keymap::Keystroke::parse("cmd-w").unwrap()),
            )),
            MenuItem::Standard(StandardAction::Close),
        ],
    );

    let edit_menu = Menu::new(
        "Edit",
        vec![
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("menu_copy"),
                |ctx| {
                    ctx.dispatch_global_action("nexshell:copy", &());
                },
                |_, _| MenuItemPropertyChanges::default(),
                Some(warpui::keymap::Keystroke::parse("cmd-c").unwrap()),
            )),
            MenuItem::Standard(StandardAction::Paste),
            MenuItem::Separator,
            MenuItem::Custom(CustomMenuItem::new(
                &rust_i18n::t!("menu_find"),
                |ctx| {
                    ctx.dispatch_global_action("nexshell:find", &());
                },
                |_, _| MenuItemPropertyChanges::default(),
                Some(warpui::keymap::Keystroke::parse("cmd-f").unwrap()),
            )),
        ],
    );

    let window_menu = Menu::new(
        "Window",
        vec![
            MenuItem::Standard(StandardAction::Minimize),
            MenuItem::Standard(StandardAction::Zoom),
            MenuItem::Separator,
            MenuItem::Standard(StandardAction::BringAllToFront),
            MenuItem::Standard(StandardAction::ToggleFullScreen),
        ],
    );

    MenuBar::new(vec![app_menu, file_menu, edit_menu, window_menu])
}

fn open_main_window(ctx: &mut AppContext, foreground_flags: Arc<Mutex<Vec<Arc<AtomicBool>>>>) {
    ctx.add_window(
        AddWindowOptions {
            title: Some(DEFAULT_WINDOW_TITLE.to_string()),
            window_bounds: WindowBounds::ExactSize(vec2f(
                DEFAULT_WINDOW_WIDTH,
                DEFAULT_WINDOW_HEIGHT,
            )),
            ..Default::default()
        },
        move |ctx| RootView::new(ctx, foreground_flags),
    );
}

/// 提高进程可打开文件描述符上限：每台主机监控要占 tokio runtime + SSH 连接的 fd，
/// macOS GUI app 默认软上限很低（~256），多主机时会 EMFILE。启动期尽量提到 hard limit。
#[cfg(unix)]
fn raise_open_file_limit() {
    unsafe {
        let mut rl: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) != 0 {
            return;
        }
        // macOS 的 hard 可能是 RLIM_INFINITY，但实际受 kern.maxfilesperproc 限制，
        // 故取一个保守上限；否则用 min(desired, hard)。best-effort，失败忽略。
        const DESIRED: libc::rlim_t = 10240;
        let target = if rl.rlim_max == libc::RLIM_INFINITY {
            DESIRED
        } else {
            DESIRED.min(rl.rlim_max)
        };
        if target > rl.rlim_cur {
            rl.rlim_cur = target;
            let _ = libc::setrlimit(libc::RLIMIT_NOFILE, &rl);
        }
    }
}

fn main() -> Result<()> {
    #[cfg(unix)]
    raise_open_file_limit();
    // 设了 RUST_LOG 才接管 tracing（看 IronRDP 内部日志，如 RUST_LOG=ironrdp_rdpsnd=debug）。
    if std::env::var_os("RUST_LOG").is_some() {
        use tracing_subscriber::EnvFilter;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }
    nexshell::features::init_feature_flags();
    warp_core::features::FeatureFlag::ImeMarkedText.set_enabled(true);
    #[cfg(target_os = "macos")]
    nexshell::platform::macos::install_warp_ime_shims();

    let foreground_flags: Arc<Mutex<Vec<Arc<AtomicBool>>>> = Arc::new(Mutex::new(Vec::new()));

    let flags_for_close = Arc::clone(&foreground_flags);
    let flags_for_quit = Arc::clone(&foreground_flags);
    let flags_for_reopen = Arc::clone(&foreground_flags);

    let mut callbacks = platform::AppCallbacks::default();

    // 必须设置，否则 handle_window_closed 不会被调用，窗口清理不完整
    callbacks.on_window_will_close = Some(Box::new(|_closed_data, _ctx| {}));

    // 点 X → 只关窗口，有进程则弹确认（与 Warp/iTerm 一致）
    callbacks.on_should_close_window = Some(Box::new(move |window_id, ctx| {
        let running_count = flags_for_close
            .lock()
            .map(|flags| flags.iter().filter(|f| !f.load(Ordering::Relaxed)).count())
            .unwrap_or(0);
        // 有未保存的内置编辑器内容也要拦截，否则关窗会静默丢失（审查 #1）。
        let has_unsaved = app_has_unsaved_code_viewer(ctx);

        if running_count == 0 && !has_unsaved {
            return ApproveTerminateResult::Terminate;
        }

        let message = if has_unsaved {
            rust_i18n::t!("dialog_close_window_unsaved").to_string()
        } else {
            rust_i18n::t!("dialog_close_window_msg", count = running_count).to_string()
        };
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_app(
            rust_i18n::t!("dialog_close_window"),
            message,
            vec![
                ModalButton::for_app(rust_i18n::t!("dialog_confirm_close"), move |ctx| {
                    ctx.windows()
                        .close_window(window_id, TerminationMode::ForceTerminate);
                }),
                ModalButton::for_app(rust_i18n::t!("dialog_cancel"), |_ctx| {}),
            ],
            |_ctx| {},
        ));
        ApproveTerminateResult::Cancel
    }));

    // Cmd+Q / 菜单 Quit → 真正退出，有进程则弹确认
    callbacks.on_should_terminate_app = Some(Box::new(move |source, ctx| {
        // 系统发起的退出（注销/重启/关机/系统更新）不可阻塞：下面任一 Cancel 分支
        // 都会被系统解读为拒绝退出，可能中止关机流程或卡在没有可见窗口的确认弹窗上。
        if source == TerminationRequestSource::System {
            return ApproveTerminateResult::Terminate;
        }

        let running_count = flags_for_quit
            .lock()
            .map(|flags| flags.iter().filter(|f| !f.load(Ordering::Relaxed)).count())
            .unwrap_or(0);
        // 有未保存的内置编辑器内容也要拦截，否则 Cmd+Q 会静默丢失（审查 #1）。
        let has_unsaved = app_has_unsaved_code_viewer(ctx);

        if running_count == 0 && !has_unsaved {
            return ApproveTerminateResult::Terminate;
        }

        let message = if has_unsaved {
            rust_i18n::t!("dialog_quit_app_unsaved").to_string()
        } else {
            rust_i18n::t!("dialog_quit_app_msg", count = running_count).to_string()
        };
        ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_app(
            rust_i18n::t!("dialog_quit_app"),
            message,
            vec![
                ModalButton::for_app(rust_i18n::t!("dialog_confirm_quit"), move |ctx| {
                    ctx.terminate_app(TerminationMode::ForceTerminate, None);
                }),
                ModalButton::for_app(rust_i18n::t!("dialog_cancel"), |_ctx| {}),
            ],
            |_ctx| {},
        ));
        ApproveTerminateResult::Cancel
    }));

    // 点 Dock 图标且无可见窗口时，重新打开主窗口
    callbacks.on_new_window_requested = Some(Box::new(move |ctx| {
        open_main_window(ctx, Arc::clone(&flags_for_reopen));
    }));

    let mut app_builder = platform::AppBuilder::new(callbacks, Box::new(EmbeddedAssets), None);
    configure_warp_text_input_custom_action_key_bindings(&mut app_builder);

    #[cfg(target_os = "macos")]
    app_builder.set_menu_bar_builder(nexshell_menu_bar);

    #[cfg(target_os = "windows")]
    {
        // 对齐 warp（lib.rs:1037）：设 AppUserModelID（用 nexshell 自己的 bundle id），
        // Windows 任务栏据此正确分组窗口 / 取应用图标 / 归集跳转列表与通知。
        use warpui::platform::windows::AppBuilderExt;
        app_builder.set_app_user_model_id("com.matt.nexshell".to_string());
    }

    let flags_for_run = Arc::clone(&foreground_flags);
    let _ = app_builder.run(move |ctx| {
        #[cfg(target_os = "macos")]
        macos_window_util::install_reduce_transparency_observer();

        register_warp_text_input_stack(ctx);
        register_terminal_key_bindings(ctx);
        register_menu_global_actions(ctx);
        open_main_window(ctx, flags_for_run);
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    // 仅保留跨模块集成 / i18n / 启动类测试；各 helper 的单元测试已随 fn 迁出（附录 C）。
    use super::{
        accepts_generation, GenerationAllocator, HostFleetSyncDebounce, PanePresentationState,
        RdpConnectionPhase, TerminalSessionKind,
    };
    use crate::terminal_view_helpers::terminal_tab_kind_uses_side_panel_layout;
    use nexshell::pane_state::NexPaneId;

    #[test]
    fn git_diff_tab_uses_side_panel_layout_so_git_panel_stays_visible() {
        assert!(terminal_tab_kind_uses_side_panel_layout(
            TerminalSessionKind::GitDiff
        ));
    }

    #[test]
    fn code_viewer_tab_uses_side_panel_layout_so_file_panel_stays_openable() {
        // 打开文件会切到 CodeViewer tab；若不走侧栏布局，文件面板按钮 toggle 后无处渲染 → 打不开。
        assert!(terminal_tab_kind_uses_side_panel_layout(
            TerminalSessionKind::CodeViewer
        ));
    }

    #[test]
    fn only_terminal_grid_tabs_support_recording() {
        for kind in [
            TerminalSessionKind::Local,
            TerminalSessionKind::Remote,
            TerminalSessionKind::Serial,
            TerminalSessionKind::Direct,
        ] {
            assert!(kind.supports_terminal_recording(), "{kind:?}");
        }

        for kind in [
            TerminalSessionKind::ProcessList,
            TerminalSessionKind::NetworkList,
            TerminalSessionKind::SystemInfo,
            TerminalSessionKind::GitDiff,
            TerminalSessionKind::CodeViewer,
            TerminalSessionKind::Rdp,
        ] {
            assert!(!kind.supports_terminal_recording(), "{kind:?}");
        }
    }

    #[test]
    fn pane_presentation_instances_toggle_independently() {
        let pane_a = NexPaneId::new();
        let pane_b = NexPaneId::new();
        let mut tab_a = PanePresentationState::default();
        let mut tab_b = PanePresentationState::default();

        tab_a.toggle_maximize(2, pane_a);
        assert_eq!(tab_a.maximized_pane(), Some(pane_a));
        assert_eq!(tab_b.maximized_pane(), None);

        tab_b.toggle_maximize(1, pane_b);
        assert_eq!(tab_b.maximized_pane(), None);

        tab_b.toggle_maximize(2, pane_b);
        assert_eq!(tab_b.maximized_pane(), Some(pane_b));
        assert_eq!(tab_a.maximized_pane(), Some(pane_a));

        tab_a.toggle_maximize(2, pane_a);
        assert_eq!(tab_a.maximized_pane(), None);
        assert_eq!(tab_b.maximized_pane(), Some(pane_b));
    }

    #[test]
    fn generation_allocator_issues_distinct_values_and_rejects_stale_ones() {
        let mut allocator = GenerationAllocator::default();
        let old_tab_session = allocator.allocate();
        let reopened_tab_session = allocator.allocate();
        let first_save_after_reopen = allocator.allocate();

        assert_ne!(old_tab_session, reopened_tab_session);
        assert_ne!(old_tab_session, first_save_after_reopen);
        assert_ne!(reopened_tab_session, first_save_after_reopen);
        assert!(accepts_generation(
            Some(reopened_tab_session),
            reopened_tab_session
        ));
        assert!(!accepts_generation(
            Some(reopened_tab_session),
            old_tab_session
        ));
        assert!(!accepts_generation(None, old_tab_session));
    }

    #[test]
    fn host_fleet_search_debounce_only_accepts_the_latest_scheduled_sync() {
        let mut allocator = GenerationAllocator::default();
        let mut debounce = HostFleetSyncDebounce::default();
        let first = debounce.schedule(&mut allocator);
        let latest = debounce.schedule(&mut allocator);

        assert!(!debounce.accept(first));
        assert!(debounce.accept(latest));
        assert!(!debounce.accept(latest));
    }

    #[test]
    fn rdp_presentation_waits_for_the_first_frame_after_transport_connects() {
        let mut phase = RdpConnectionPhase::Connecting;

        phase.on_transport_connected();
        assert!(matches!(phase, RdpConnectionPhase::Connecting));

        phase.on_frame_uploaded();
        assert!(matches!(phase, RdpConnectionPhase::Connected));
    }

    #[test]
    fn zh_cn_pane_menu_labels_use_window_wording() {
        let zh_cn = include_str!("../locales/zh-CN.yml");
        assert!(zh_cn.contains("ctx_maximize_pane: 最大化窗口"));
        assert!(zh_cn.contains("ctx_restore_pane: 恢复窗口"));
        assert!(zh_cn.contains("ctx_close_pane: 关闭窗口"));
        assert!(zh_cn.contains("key_maximize_pane: 最大化窗口"));
        assert!(zh_cn.contains("key_close_pane: 关闭窗口"));
    }

    #[test]
    fn warp_text_input_custom_actions_have_keystroke_fallbacks() {
        use nexshell::util::bindings::CustomAction;
        use warpui::keymap::Keystroke;

        assert_eq!(
            super::warp_text_input_custom_tag_to_keystroke(CustomAction::Cut.into()),
            Keystroke::parse("cmdorctrl-x").ok()
        );
        let copy_keystroke = if super::platform::OperatingSystem::get().is_mac() {
            "cmd-c"
        } else {
            "ctrl-shift-C"
        };
        assert_eq!(
            super::warp_text_input_custom_tag_to_keystroke(CustomAction::Copy.into()),
            Keystroke::parse(copy_keystroke).ok()
        );
        assert_eq!(
            super::warp_text_input_custom_tag_to_keystroke(CustomAction::Paste.into()),
            Keystroke::parse("cmdorctrl-v").ok()
        );
        assert_eq!(
            super::warp_text_input_custom_tag_to_keystroke(CustomAction::SelectAll.into()),
            Keystroke::parse("cmdorctrl-a").ok()
        );
    }
}
