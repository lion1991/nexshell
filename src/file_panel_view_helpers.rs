//! 文件面板（远端/本地）视图相关的纯函数 helper：图标按钮、空态提示、上下文菜单、
//! 路径转换、shell 转义、跨平台 reveal 调用、远端 mtime/叶名格式化。

use chrono::{DateTime, Local};
use pathfinder_geometry::vector::vec2f;
use warpui::color::ColorU;
use warpui::elements::{
    Align, ChildAnchor, ConstrainedBox, Container, CornerRadius, DispatchEventResult, EventHandler,
    Hoverable, Icon, MouseStateHandle, OffsetPositioning, Padding, ParentElement,
    PositionedElementAnchor, PositionedElementOffsetBounds, Radius, Stack, Text,
};
use warpui::fonts;
use warpui::Element;

use nexshell::file_panel::{join_path, parent_path};

/// 文件面板行的「悬浮显示完整名」tooltip（本地树行 / 远程行共用）。
/// `base` 内部需含一个 `SavePosition(position_id)` 锚点（即被截断的名字元素）；调用方在 hover 时调用。
/// 复用 git 面板 status_section 的 per-row tooltip 模式。
pub(crate) fn file_panel_name_tooltip(
    base: Box<dyn Element>,
    position_id: &str,
    full_text: String,
    font: fonts::FamilyId,
    tooltip_bg: ColorU,
    tooltip_text: ColorU,
) -> Box<dyn Element> {
    let tooltip = Container::new(
        ConstrainedBox::new(
            Text::new(full_text, font, 12.0)
                .with_line_height_ratio(1.25)
                .with_color(tooltip_text)
                .finish(),
        )
        .with_max_width(520.0)
        .finish(),
    )
    .with_background_color(tooltip_bg)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
    .with_padding(Padding::uniform(8.0))
    .finish();
    let mut stack = Stack::new().with_child(base);
    stack.add_positioned_overlay_child(
        tooltip,
        OffsetPositioning::offset_from_save_position_element(
            position_id.to_string(),
            vec2f(0.0, 6.0),
            PositionedElementOffsetBounds::WindowByPosition,
            PositionedElementAnchor::BottomLeft,
            ChildAnchor::TopLeft,
        ),
    );
    stack.finish()
}

use super::terminal_grid_element::TerminalGridAction;

/// 文件面板里的小图标按钮（路径栏 Up / Refresh 等）。
pub(crate) fn render_file_panel_icon_button(
    state: MouseStateHandle,
    icon_path: &'static str,
    icon_color: ColorU,
    hover_bg: ColorU,
    action: TerminalGridAction,
) -> Box<dyn Element> {
    let button = Hoverable::new(state, move |mouse| {
        let icon = ConstrainedBox::new(Icon::new(icon_path, icon_color).finish())
            .with_width(14.0)
            .with_height(14.0)
            .finish();
        let mut container = Container::new(
            ConstrainedBox::new(Align::new(icon).finish())
                .with_width(22.0)
                .with_height(22.0)
                .finish(),
        )
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)));
        if mouse.is_hovered() {
            container = container.with_background_color(hover_bg);
        }
        container.finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(action.clone());
    })
    .finish();
    Align::new(button).finish()
}

/// 断线时的可点击重连提示：替代普通 error 文本，点击派发 ReconnectTab（中断重连体验）。
pub(crate) fn file_panel_reconnect_message(
    text: &str,
    font: fonts::FamilyId,
    color: ColorU,
    tab_index: usize,
) -> Box<dyn Element> {
    let msg = Container::new(
        Text::new_inline(format!("{text}（点此重连）"), font, 12.0)
            .with_color(color)
            .finish(),
    )
    .with_padding_top(16.0)
    .finish();
    EventHandler::new(msg)
        .on_left_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(TerminalGridAction::ReconnectTab(tab_index));
            DispatchEventResult::StopPropagation
        })
        .finish()
}

/// 面板正文区域的 loading / error / 空态提示。
pub(crate) fn file_panel_message(
    text: &str,
    font: fonts::FamilyId,
    color: ColorU,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(text.to_string(), font, 12.0)
            .with_color(color)
            .finish(),
    )
    .with_padding_top(16.0)
    .finish()
}

pub(crate) fn file_panel_reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "file_panel_ctx_reveal_finder"
    } else if cfg!(target_os = "windows") {
        "file_panel_ctx_reveal_explorer"
    } else {
        "file_panel_ctx_reveal_file_manager"
    }
}

pub(crate) fn remote_file_panel_context_menu_items(
    name: Option<String>,
    cwd: &str,
    is_dir: bool,
    multi_count: Option<usize>,
) -> Vec<warp::menu::MenuItem<TerminalGridAction>> {
    use warp::menu::{MenuItem, MenuItemFields};

    let has_target = name.is_some();
    let placeholder_name = name.unwrap_or_default();
    let multi_select_active = multi_count.is_some();
    let delete_label = match multi_count {
        Some(n) => rust_i18n::t!("file_panel_ctx_delete_many", count = n.to_string()).into_owned(),
        None => rust_i18n::t!("file_panel_ctx_delete").into_owned(),
    };

    let mut items = Vec::new();
    // 文件才有「用编辑器打开」，置顶（ADR 0005：远程经 SFTP 内存读写）；二进制/超大由 handler 提示下载。
    if has_target && !is_dir {
        items.push(
            MenuItemFields::new(rust_i18n::t!("file_panel_ctx_open_with_viewer"))
                .with_on_select_action(TerminalGridAction::FilePanelOpenInCodeViewer {
                    path: join_path(cwd, &placeholder_name),
                })
                .into_item(),
        );
        items.push(MenuItem::Separator);
    }
    items.extend([
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_download_as"))
            .with_disabled(!has_target || multi_select_active)
            .with_on_select_action(TerminalGridAction::FilePanelDownload {
                name: placeholder_name.clone(),
                is_dir,
            })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_rename"))
            .with_disabled(!has_target || multi_select_active)
            .with_on_select_action(TerminalGridAction::FilePanelStartRename {
                name: placeholder_name.clone(),
            })
            .into_item(),
        MenuItemFields::new(delete_label)
            .with_disabled(!has_target)
            .with_on_select_action(TerminalGridAction::FilePanelDelete {
                name: placeholder_name.clone(),
                is_dir,
            })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_copy_path"))
            .with_on_select_action(TerminalGridAction::FilePanelCopyPath {
                name: placeholder_name,
            })
            .into_item(),
        MenuItem::Separator,
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_new_dir"))
            .with_on_select_action(TerminalGridAction::FilePanelStartNewDir)
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_new_file"))
            .with_on_select_action(TerminalGridAction::FilePanelStartNewFile)
            .into_item(),
    ]);
    items
}

pub(crate) fn local_file_panel_context_menu_items(
    target_path: Option<String>,
    root_path: &str,
    is_dir: bool,
) -> Vec<warp::menu::MenuItem<TerminalGridAction>> {
    use warp::menu::{MenuItem, MenuItemFields};

    let has_target = target_path.is_some();
    let path = target_path.unwrap_or_else(|| root_path.to_string());
    let target_is_dir = !has_target || is_dir;
    let create_parent = if target_is_dir {
        path.clone()
    } else {
        parent_path(&path)
    };

    let mut items = Vec::new();
    // 文件才有「打开/编辑」，置顶；目录无（保持 cd / 在新标签打开）
    if has_target && !is_dir {
        items.push(
            MenuItemFields::new(rust_i18n::t!("file_panel_ctx_open"))
                .with_on_select_action(TerminalGridAction::FilePanelOpenWithDefault {
                    path: path.clone(),
                })
                .into_item(),
        );
        items.push(
            MenuItemFields::new(rust_i18n::t!("file_panel_ctx_edit"))
                .with_on_select_action(TerminalGridAction::FilePanelOpenInEditor {
                    path: path.clone(),
                })
                .into_item(),
        );
        // 内置只读查看器（ADR 0002）：二进制 / 超大由 handler 回退「打开」。
        items.push(
            MenuItemFields::new(rust_i18n::t!("file_panel_ctx_open_with_viewer"))
                .with_on_select_action(TerminalGridAction::FilePanelOpenInCodeViewer {
                    path: path.clone(),
                })
                .into_item(),
        );
        items.push(MenuItem::Separator);
    }
    items.extend([
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_new_file"))
            .with_disabled(!target_is_dir)
            .with_on_select_action(TerminalGridAction::FilePanelStartNewFileIn {
                parent: create_parent,
            })
            .into_item(),
        MenuItem::Separator,
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_sync_terminal_cwd"))
            .with_on_select_action(TerminalGridAction::FilePanelSyncToTerminalCwd)
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_cd_to_directory"))
            .with_disabled(!target_is_dir)
            .with_on_select_action(TerminalGridAction::FilePanelCdToDirectory {
                path: path.clone(),
            })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_open_new_tab"))
            .with_disabled(!target_is_dir)
            .with_on_select_action(TerminalGridAction::FilePanelOpenDirectoryInNewTab {
                path: path.clone(),
            })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!(file_panel_reveal_label()))
            .with_on_select_action(TerminalGridAction::FilePanelRevealInFileManager {
                path: path.clone(),
            })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_rename"))
            .with_disabled(!has_target)
            .with_on_select_action(TerminalGridAction::FilePanelStartRename { name: path.clone() })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_delete"))
            .with_disabled(!has_target)
            .with_on_select_action(TerminalGridAction::FilePanelDelete {
                name: path.clone(),
                is_dir,
            })
            .into_item(),
        MenuItem::Separator,
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_copy_path"))
            .with_on_select_action(TerminalGridAction::FilePanelCopyPath { name: path.clone() })
            .into_item(),
        MenuItemFields::new(rust_i18n::t!("file_panel_ctx_copy_relative_path"))
            .with_on_select_action(TerminalGridAction::FilePanelCopyRelativePath { path })
            .into_item(),
    ]);
    items
}

/// 代码查看器 tab 的 preferred id：按 (源终端标签, 文件路径) 哈希。
/// 真正的复用匹配在 open_code_viewer_tab 按字段比对，这里只保证创建时 id 合理唯一。
pub(crate) fn code_viewer_tab_id(source_tab_id: &str, path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    source_tab_id.hash(&mut hasher);
    path.hash(&mut hasher);
    format!("code-viewer-{:016x}", hasher.finish())
}

/// 代码查看器 tab 标签：取文件名（取不到则用完整路径）。
pub(crate) fn code_viewer_tab_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn file_panel_relative_path(root: &str, path: &str) -> String {
    let root = std::path::Path::new(root);
    let path_ref = std::path::Path::new(path);
    if path_ref == root {
        return ".".to_string();
    }
    path_ref
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.to_str())
        .filter(|relative| !relative.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string())
}

pub(crate) fn shell_quote_posix(arg: &str) -> String {
    if arg.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '=' | '@')
    }) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

pub(crate) fn file_panel_cd_command(path: &str) -> Vec<u8> {
    format!("\x15cd {}\n", shell_quote_posix(path)).into_bytes()
}

pub(crate) fn reveal_file_manager_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("open -R failed: {error}"))
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("explorer reveal failed: {error}"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let path_ref = std::path::Path::new(path);
        let open_path = if path_ref.is_dir() {
            path_ref
        } else {
            path_ref.parent().unwrap_or(path_ref)
        };
        std::process::Command::new("xdg-open")
            .arg(open_path)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("xdg-open failed: {error}"))
    }
}

/// 文件面板用：当前年只显示 "MM-DD HH:MM"，其他年显示 "YYYY-MM-DD"。
/// 紧凑、适配窄面板。
pub(crate) fn format_remote_mtime(modified: Option<std::time::SystemTime>) -> String {
    let Some(ts) = modified else {
        return String::new();
    };
    let dt: DateTime<Local> = ts.into();
    let now_year = Local::now().format("%Y").to_string();
    let year = dt.format("%Y").to_string();
    if year == now_year {
        dt.format("%m-%d %H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

pub(crate) fn file_panel_leaf_name(name_or_path: &str) -> String {
    std::path::Path::new(name_or_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(name_or_path)
        .to_string()
}
