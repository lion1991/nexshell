// file_panel_section — RootView 的文件面板（SFTP / 本地 Project explorer）方法集合。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本目录只含 impl RootView，无自由函数；
// 新增 helper 一律加到 src/file_panel_view_helpers.rs。
//
// 切分：
// - mod.rs   ：面板 chrome（shell / divider / header / input_bar）+ worker 数据装配 + 入口判定
// - body.rs  ：列表 / 目录树 / 远程目录 / 传输任务区渲染
// - actions.rs：handle_* action handler + inline 输入编辑器 + 上传/下载/reveal

mod actions;
mod body;

use std::path::PathBuf;
use std::sync::Arc;

use crate::terminal_grid_element::TerminalGridAction;
use crate::ui_colors::HostOverviewColors;
use crate::{
    FilePanelInputIntent, RootView, TerminalSessionKind, TerminalSessionTab,
    FILE_PANEL_DIVIDER_WIDTH, FILE_PANEL_WIDTH_DEFAULT, FILE_PANEL_WIDTH_MAX, FILE_PANEL_WIDTH_MIN,
};
use nexshell::file_drop_target::FileDropTarget;
use nexshell::file_panel::{
    apply_local_file_panel_event, spawn_local_file_worker, FilePanelWorkerHandle, SftpRequest,
};
use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DragAxis, Draggable, Empty,
    Expanded, Fill, Flex, Hoverable, MainAxisSize, ParentElement, Radius, Text,
};
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{AppContext, Element, ViewContext};

impl RootView {
    /// 文件面板当前作用的 tab：CodeViewer / GitDiff 这类查看伪 tab 经 source_terminal_tab_index
    /// 代理到源终端 tab，故打开文件 / 看 diff 后文件面板仍展示源终端目录、可继续操作；
    /// 源终端已关（孤儿）则为 None，面板随之惰性禁用（与 git 面板一致）。
    pub(in crate::root_view) fn file_panel_tab(&self) -> Option<&TerminalSessionTab> {
        self.source_terminal_tab_index()
            .and_then(|idx| self.terminal_tabs.get(idx))
    }

    /// file_panel_tab 的可变版。
    pub(in crate::root_view) fn file_panel_tab_mut(&mut self) -> Option<&mut TerminalSessionTab> {
        let idx = self.source_terminal_tab_index()?;
        self.terminal_tabs.get_mut(idx)
    }

    pub(crate) fn should_render_file_panel(&self) -> bool {
        self.file_panel_tab()
            .map(|tab| tab.file_panel_open)
            .unwrap_or(false)
    }

    pub(in crate::root_view) fn render_file_panel(&self, _app: &AppContext) -> Box<dyn Element> {
        let colors = HostOverviewColors::from_theme(&self.cached_warp_theme);
        let Some(tab) = self.file_panel_tab() else {
            return self.render_file_panel_shell(
                Text::new_inline("文件", self.ui_font, 12.0)
                    .with_color(colors.text_primary)
                    .finish(),
                FILE_PANEL_WIDTH_DEFAULT,
                &colors,
            );
        };
        let width = tab
            .file_panel_width
            .clamp(FILE_PANEL_WIDTH_MIN, FILE_PANEL_WIDTH_MAX);

        let is_local = matches!(tab.kind, TerminalSessionKind::Local);
        let header = if is_local {
            self.render_local_file_panel_header(tab, &colors)
        } else {
            self.render_file_panel_header(tab, &colors)
        };
        let body = if is_local {
            self.render_local_file_panel_body(tab, &colors)
        } else {
            self.render_file_panel_body(tab, &colors)
        };
        let transfers = self.render_file_panel_transfers(tab, &colors);

        let mut content = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header);
        if let Some(input_bar) = self.render_file_panel_input_bar(&colors) {
            content.add_child(input_bar);
        }
        content.add_child(Expanded::new(1.0, body).finish());
        if let Some(transfers) = transfers {
            content.add_child(transfers);
        }

        let shell = self.render_file_panel_shell(content.finish(), width, &colors);
        let callback: nexshell::file_drop_target::DropCallback =
            Arc::new(|ctx, paths| {
                ctx.dispatch_typed_action(TerminalGridAction::FilePanelDropFiles(paths));
            });
        FileDropTarget::new(shell, callback).finish()
    }

    fn render_file_panel_shell(
        &self,
        inner: Box<dyn Element>,
        width: f32,
        colors: &HostOverviewColors,
    ) -> Box<dyn Element> {
        let padded = Container::new(inner)
            .with_padding_left(10.0)
            .with_padding_right(10.0)
            .with_padding_top(10.0)
            .with_padding_bottom(10.0)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_file_panel_divider())
            .with_child(Expanded::new(1.0, padded).finish());
        Container::new(ConstrainedBox::new(row.finish()).with_width(width).finish())
            .with_background_color(colors.panel_bg)
            .with_border(Border::left(1.0).with_border_color(colors.panel_border))
            .finish()
    }

    /// 左缘拖拽条：透明命中区域。Hoverable 提供 ResizeLeftRight 光标 + cursor 反馈，
    /// EventHandler 只接 mouse_down 起 anchor。dragged/up 由 RootView 根 EventHandler
    /// 全局接收，避免鼠标超出 6px 命中区时事件丢失（参考 warp pane_group/mod.rs:7943
    /// 在外层包 dragged 的同样目的）。
    fn render_file_panel_divider(&self) -> Box<dyn Element> {
        // warpui::Draggable 提供"按住-离开元素也持续派发"的 capture flow
        // （ui_components/slider.rs 同款）。EventHandler.on_mouse_dragged 是 hit-test
        // 派发，鼠标移出 6px 命中区就停了。
        // Draggable 在 mouse_down 时硬写 set_cursor(PointingHand)（draggable.rs:611），
        // 所以 on_drag_start / on_drag 里用更高 z 的 Overlay 覆盖回 ResizeLeftRight。
        let drag_state = self.file_panel_divider_drag_state.clone();
        Hoverable::new(self.file_panel_divider_state.clone(), move |_mouse| {
            let inner = ConstrainedBox::new(Empty::new().finish())
                .with_width(FILE_PANEL_DIVIDER_WIDTH)
                .finish();
            Draggable::new(drag_state.clone(), inner)
                .with_drag_axis(DragAxis::HorizontalOnly)
                .with_keep_original_visible(true)
                .on_drag_start(|ctx, _, rect| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeLeftRight,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::FilePanelResizeStart(
                        rect.origin_x(),
                    ));
                })
                .on_drag(|ctx, _, rect, _| {
                    ctx.set_cursor(
                        warpui::platform::Cursor::ResizeLeftRight,
                        warpui::elements::ZIndex::Overlay(usize::MAX),
                    );
                    ctx.dispatch_typed_action(TerminalGridAction::FilePanelResizeMove(
                        rect.origin_x(),
                    ));
                })
                .on_drop(|ctx, _, _rect, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::FilePanelResizeEnd);
                })
                .finish()
        })
        .with_cursor(warpui::platform::Cursor::ResizeLeftRight)
        .finish()
    }

    /// inline 输入栏：file_panel_input_intent.is_some() 时显示在 header 下方。
    /// Enter 提交、Esc 取消（事件由 handle_file_panel_input_editor_event 处理）。
    fn render_file_panel_input_bar(&self, colors: &HostOverviewColors) -> Option<Box<dyn Element>> {
        let intent = self.file_panel_input_intent.as_ref()?;
        let label_text = match intent {
            FilePanelInputIntent::NewDir => rust_i18n::t!("file_panel_input_new_dir").to_string(),
            FilePanelInputIntent::NewFile | FilePanelInputIntent::NewFileIn { .. } => {
                rust_i18n::t!("file_panel_input_new_file").to_string()
            }
            FilePanelInputIntent::Rename { old_name } => {
                rust_i18n::t!("file_panel_input_rename", name = old_name.as_str()).to_string()
            }
        };
        let label = Text::new_inline(label_text, self.ui_font, 11.0)
            .with_color(colors.text_muted)
            .finish();
        let input = TextInput::new(
            self.file_panel_input_editor.clone(),
            UiComponentStyles::default()
                .set_background(Fill::None)
                .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                .set_border_width(1.0),
        )
        .build()
        .finish();
        let row = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Container::new(label).with_padding_bottom(2.0).finish())
            .with_child(input);
        Some(
            Container::new(row.finish())
                .with_padding_top(6.0)
                .with_padding_bottom(6.0)
                .finish(),
        )
    }

    fn start_local_file_worker_for_tab(
        view: &mut Self,
        tab_id: &str,
        init_path: std::path::PathBuf,
        ctx: &mut ViewContext<Self>,
    ) {
        let label = {
            let Some(tab) = view.terminal_tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            if !matches!(tab.kind, TerminalSessionKind::Local) {
                return;
            }
            tab.id.clone()
        };
        match spawn_local_file_worker(&label, init_path.clone()) {
            Ok((worker, evt_rx)) => {
                worker.send(SftpRequest::List(init_path.to_string_lossy().into_owned()));
                if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.sftp_worker = Some(FilePanelWorkerHandle::Local(worker));
                    tab.file_panel_state.loading = true;
                    tab.file_panel_state.error = None;
                }
                let owner = tab_id.to_string();
                ctx.spawn_stream_local(
                    evt_rx,
                    move |view, evt, ctx| {
                        if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == owner) {
                            apply_local_file_panel_event(&mut tab.file_panel_state, evt);
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

    /// 终端断线时：清死 SFTP worker（其 session 随 SSH 通道关闭，留着会一直返回错误），
    /// 标记文件区断开，等手动重连推上新 handle 再重建（中断重连体验，缺陷1）。
    pub(super) fn mark_remote_file_panel_disconnected(&mut self, tab_id: &str) {
        let Some(tab) = self.terminal_tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        // 仅远程 SSH tab：本地文件 worker 不受 SSH 断线影响。
        if tab.ssh_handle.is_none() {
            return;
        }
        tab.sftp_worker = None;
        if tab.file_panel_open {
            tab.file_panel_state.loading = false;
            tab.file_panel_state.error = Some("连接已断开".to_string());
        }
    }

    pub(super) fn refresh_or_restart_file_panel_worker(
        &mut self,
        tab_id: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(index) = self.terminal_tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        if !Self::terminal_tab_is_connected(&self.terminal_tabs[index]) {
            let tab = &mut self.terminal_tabs[index];
            tab.sftp_worker = None;
            tab.file_panel_state.loading = false;
            tab.file_panel_state.error = Some("连接已断开".to_string());
            return;
        }

        if matches!(self.terminal_tabs[index].kind, TerminalSessionKind::Local) {
            let request_path = self.local_file_panel_request_path(index);
            if let Some(worker) = self.terminal_tabs[index].sftp_worker.as_ref() {
                let sent = match request_path.as_ref() {
                    Some(path) => {
                        worker.send(SftpRequest::List(path.to_string_lossy().into_owned()))
                    }
                    None => worker.send(SftpRequest::Refresh),
                };
                if sent {
                    return;
                }
                self.terminal_tabs[index].sftp_worker = None;
            }

            let init_path =
                request_path.unwrap_or_else(|| self.local_file_panel_fallback_path(index));
            Self::start_local_file_worker_for_tab(self, tab_id, init_path, ctx);
            return;
        }

        if let Some(worker) = self.terminal_tabs[index].sftp_worker.as_ref() {
            if worker.send(SftpRequest::Refresh) {
                return;
            }
            self.terminal_tabs[index].sftp_worker = None;
        }

        if self.terminal_tabs[index].ssh_handle.is_some() {
            Self::start_sftp_worker_for_tab(self, tab_id, ctx);
        } else {
            let tab = &mut self.terminal_tabs[index];
            tab.file_panel_state.loading = false;
            tab.file_panel_state.error = Some("SSH 连接尚未就绪".to_string());
        }
    }

    fn local_file_panel_request_path(&self, index: usize) -> Option<std::path::PathBuf> {
        let tab = self.terminal_tabs.get(index)?;
        if tab.file_panel_state.follow_cwd {
            tab.terminal.lock().ok()?.snapshot().local_cwd.clone()
        } else {
            let cwd = tab.file_panel_state.cwd.trim();
            (!cwd.is_empty() && cwd != ".").then(|| std::path::PathBuf::from(cwd))
        }
    }

    fn local_file_panel_fallback_path(&self, index: usize) -> std::path::PathBuf {
        self.terminal_tabs
            .get(index)
            .and_then(|tab| tab.terminal.lock().ok()?.snapshot().local_cwd.clone())
            .or_else(|| {
                self.terminal_tabs.get(index).and_then(|tab| {
                    let cwd = tab.file_panel_state.cwd.trim();
                    (!cwd.is_empty() && cwd != ".").then(|| std::path::PathBuf::from(cwd))
                })
            })
            .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    pub(crate) fn dispatch_file_panel_cwd_updates(view: &mut Self, ctx: &mut ViewContext<Self>) {
        let pending: Vec<(String, PathBuf, bool)> = view
            .terminal_tabs
            .iter()
            .filter(|t| {
                matches!(t.kind, TerminalSessionKind::Local)
                    && t.file_panel_open
                    && t.file_panel_state.follow_cwd
            })
            .filter_map(|t| {
                let snap_cwd = t.terminal.lock().ok()?.snapshot().local_cwd.clone()?;
                if t.file_panel_state.cwd == snap_cwd.to_string_lossy() {
                    None
                } else {
                    Some((t.id.clone(), snap_cwd, t.sftp_worker.is_some()))
                }
            })
            .collect();

        for (tab_id, cwd, had_worker) in pending {
            if !had_worker {
                Self::start_local_file_worker_for_tab(view, &tab_id, cwd, ctx);
                continue;
            }
            if let Some(tab) = view.terminal_tabs.iter_mut().find(|t| t.id == tab_id) {
                if let Some(worker) = tab.sftp_worker.as_ref() {
                    if !worker.send(SftpRequest::List(cwd.to_string_lossy().into_owned())) {
                        tab.sftp_worker = None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::file_panel_view_helpers::file_panel_relative_path;

    #[test]
    fn local_file_panel_relative_path_uses_project_root() {
        assert_eq!(
            file_panel_relative_path("/Users/example", "/Users/example/.codex/config.toml"),
            ".codex/config.toml"
        );
        assert_eq!(
            file_panel_relative_path("/Users/example", "/Users/example"),
            "."
        );
        assert_eq!(
            file_panel_relative_path("/Users/example", "/private/tmp/x"),
            "/private/tmp/x"
        );
    }
}
