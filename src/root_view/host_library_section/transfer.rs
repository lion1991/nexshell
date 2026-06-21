// host_library_section::transfer — RootView 的主机库加密导入 / 导出 + 密码输入栏。
//
// 详见 docs/adr/0001-root-view-multi-file-impl.md。本文件只含 impl RootView，无自由函数。

use crate::host_management_view::constants::HostUiColors;
use crate::{host_export, HostPasswordIntent, RootView, TerminalGridAction};
use nexshell::host_management::{
    default_database_path, upsert_group_in_db, upsert_host_card_in_db_path,
};
use warp_core::ui::appearance::Appearance;
use nexshell::text_editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use warpui::color::ColorU;
use warpui::elements::{
    Border, Container, CornerRadius, CrossAxisAlignment, Expanded, Fill, Flex, Hoverable,
    ParentElement, Radius, Text,
};
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{Element, SingletonEntity as _, ViewContext};

impl RootView {
    pub(super) fn start_host_export(&mut self, ctx: &mut ViewContext<Self>) {
        if self.host_state.snapshot.hosts.is_empty() {
            self.host_state.notice = Some(rust_i18n::t!("toast_export_no_hosts").to_string());
            ctx.notify();
            return;
        }
        self.open_host_password_bar(HostPasswordIntent::Export, ctx);
    }

    pub(super) fn start_host_import(&mut self, ctx: &mut ViewContext<Self>) {
        let config = warpui::platform::FilePickerConfiguration::new();
        let weak = ctx.handle();
        ctx.open_file_picker(
            move |result, view_ctx| {
                let Ok(paths) = result else { return };
                let Some(path_str) = paths.into_iter().next() else {
                    return;
                };
                use warpui::UpdateView;
                let Some(handle) = weak.upgrade(view_ctx) else {
                    return;
                };
                view_ctx.update_view(&handle, |view, sub_ctx| match std::fs::read(&path_str) {
                    Ok(bytes) => {
                        view.open_host_password_bar(
                            HostPasswordIntent::Import {
                                encrypted_bytes: bytes,
                            },
                            sub_ctx,
                        );
                    }
                    Err(error) => {
                        view.host_state.notice = Some(
                            rust_i18n::t!("toast_import_read_failed", error = error.to_string())
                                .to_string(),
                        );
                        sub_ctx.notify();
                    }
                });
            },
            config,
        );
    }

    fn open_host_password_bar(&mut self, intent: HostPasswordIntent, ctx: &mut ViewContext<Self>) {
        let editor = ctx.add_typed_action_view(|ctx| {
            let font_size = Appearance::as_ref(ctx).ui_font_size();
            let options = SingleLineEditorOptions {
                text: TextOptions {
                    font_size_override: Some(font_size),
                    ..Default::default()
                },
                is_password: true,
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text(
                rust_i18n::t!("host_export_password_placeholder").to_string(),
                ctx,
            );
            editor
        });
        ctx.subscribe_to_view(&editor, |me, _, event: &EditorEvent, ctx| {
            me.handle_host_password_event(event, ctx);
        });
        ctx.focus(&editor);
        self.host_password_editor = Some(editor);
        self.host_password_intent = Some(intent);
        ctx.notify();
    }

    fn handle_host_password_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        if self.host_password_editor.is_none() || self.host_password_busy {
            return;
        }
        match event {
            EditorEvent::Enter => self.commit_host_password(ctx),
            EditorEvent::Escape => self.cancel_host_password(ctx),
            _ => {}
        }
    }

    pub(super) fn commit_host_password(&mut self, ctx: &mut ViewContext<Self>) {
        if self.host_password_busy {
            return;
        }
        let Some(editor) = self.host_password_editor.clone() else {
            return;
        };
        let Some(intent) = self.host_password_intent.as_ref() else {
            return;
        };
        let password = editor.as_ref(ctx).buffer_text(ctx);
        if password.is_empty() {
            let key = match intent {
                HostPasswordIntent::Export => "toast_export_password_required",
                HostPasswordIntent::Import { .. } => "toast_import_password_required",
            };
            self.host_state.notice = Some(rust_i18n::t!(key).to_string());
            ctx.notify();
            return;
        }
        match intent {
            HostPasswordIntent::Export => self.spawn_host_export(password, ctx),
            HostPasswordIntent::Import { encrypted_bytes } => {
                let bytes = encrypted_bytes.clone();
                self.spawn_host_import(bytes, password, ctx);
            }
        }
    }

    /// 加密在后台线程跑，完成回主线程后弹保存对话框。
    fn spawn_host_export(&mut self, password: String, ctx: &mut ViewContext<Self>) {
        let hosts = self.host_state.snapshot.hosts.clone();
        // 导出真实分组（剔除合成的 "all"），保证带分组主机可跨档案 round-trip
        let groups: Vec<host_export::ExportGroup> = self
            .host_state
            .snapshot
            .groups
            .iter()
            .filter(|g| g.id != "all")
            .map(|g| host_export::ExportGroup {
                id: g.id.clone(),
                name: g.label.clone(),
            })
            .collect();
        let count = hosts.len();
        self.host_password_busy = true;
        self.schedule_busy_tick(ctx);
        ctx.notify();
        let _ = ctx.spawn(
            async move { host_export::encrypt_export(&hosts, &groups, &password) },
            move |view, result, ctx| {
                view.host_password_busy = false;
                let bytes = match result {
                    Ok(b) => b,
                    Err(error) => {
                        view.host_state.notice =
                            Some(rust_i18n::t!("toast_export_failed", error = error).to_string());
                        ctx.notify();
                        return;
                    }
                };
                let default_name = format!(
                    "nexshell-hosts-{}.nshenc",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                );
                let config = warpui::platform::SaveFilePickerConfiguration::new()
                    .with_default_filename(default_name);
                ctx.open_save_file_picker(
                    move |chosen, view, sub_ctx| {
                        let Some(path_str) = chosen else {
                            sub_ctx.notify();
                            return;
                        };
                        let path = std::path::PathBuf::from(path_str);
                        match std::fs::write(&path, &bytes) {
                            Ok(()) => {
                                view.host_state.notice = Some(
                                    rust_i18n::t!("toast_export_success", count = count)
                                        .to_string(),
                                );
                                view.clear_host_password_state(sub_ctx);
                            }
                            Err(error) => {
                                view.host_state.notice = Some(
                                    rust_i18n::t!("toast_export_failed", error = error.to_string())
                                        .to_string(),
                                );
                                sub_ctx.notify();
                            }
                        }
                    },
                    config,
                );
            },
        );
    }

    /// 解密 + DB 写入都在后台线程跑（DB 写入相对快，跟解密一起搬，逻辑简洁）。
    fn spawn_host_import(
        &mut self,
        encrypted_bytes: Vec<u8>,
        password: String,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(db_path) = default_database_path() else {
            self.host_state.notice =
                Some(rust_i18n::t!("toast_host_library_unavailable_save").to_string());
            ctx.notify();
            return;
        };
        self.host_password_busy = true;
        self.schedule_busy_tick(ctx);
        ctx.notify();
        let _ = ctx.spawn(
            async move {
                let lib = host_export::decrypt_export(&encrypted_bytes, &password)?;
                // 先重建分组，避免随后写入的 host.group_id 悬空
                let mut groups_written = 0usize;
                let mut group_error: Option<String> = None;
                for (idx, g) in lib.groups.iter().enumerate() {
                    match upsert_group_in_db(&db_path, &g.id, &g.name, idx as i64) {
                        Ok(()) => groups_written += 1,
                        // 分组写失败不能静默：否则纯分组归档会假报成功，
                        // 主机归档则可能写入悬空 group_id（FK 关闭时）
                        Err(error) => {
                            if group_error.is_none() {
                                group_error = Some(error);
                            }
                        }
                    }
                }
                let hosts = lib.hosts;
                let total = hosts.len();
                let mut imported = 0usize;
                let mut last_error: Option<String> = None;
                for host in &hosts {
                    match upsert_host_card_in_db_path(&db_path, host) {
                        Ok(()) => imported += 1,
                        Err(error) => last_error = Some(error),
                    }
                }
                // 主机错误优先（保持原有 host-only 行为不变），否则暴露分组错误
                let last_error = last_error.or(group_error);
                Ok::<(usize, usize, usize, Option<String>), String>((
                    imported,
                    total,
                    groups_written,
                    last_error,
                ))
            },
            |view, result, ctx| {
                view.host_password_busy = false;
                match result {
                    Ok((imported, total, groups_written, last_error)) => {
                        // 仅导入分组（无主机）时也要刷新，否则恢复成功却界面无变化
                        if imported > 0 || groups_written > 0 {
                            let _ = view.load_host_snapshot_from_db();
                        }
                        view.host_state.notice = if let Some(error) = last_error {
                            Some(rust_i18n::t!("toast_import_failed", error = error).to_string())
                        } else {
                            Some(
                                rust_i18n::t!(
                                    "toast_import_success",
                                    count = imported,
                                    total = total
                                )
                                .to_string(),
                            )
                        };
                        view.clear_host_password_state(ctx);
                    }
                    Err(error) => {
                        view.host_state.notice =
                            Some(rust_i18n::t!("toast_import_failed", error = error).to_string());
                        ctx.notify();
                    }
                }
            },
        );
    }

    pub(super) fn cancel_host_password(&mut self, ctx: &mut ViewContext<Self>) {
        if self.host_password_busy {
            // 加密/解密在后台跑，没有 cancel token；让任务跑完后 callback 会自然清状态。
            return;
        }
        self.clear_host_password_state(ctx);
    }

    fn clear_host_password_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.host_password_editor = None;
        self.host_password_intent = None;
        self.host_password_busy = false;
        ctx.focus_self();
        ctx.notify();
    }

    /// 自递归调度：每 300ms notify 一次以驱动省略号循环；busy 转 false 时自动停。
    fn schedule_busy_tick(&mut self, ctx: &mut ViewContext<Self>) {
        let _ = ctx.spawn(
            async {
                warpui::r#async::Timer::after(std::time::Duration::from_millis(300)).await;
            },
            |view, _, ctx| {
                if view.host_password_busy {
                    ctx.notify();
                    view.schedule_busy_tick(ctx);
                }
            },
        );
    }

    pub(in crate::root_view) fn render_host_password_bar(&self) -> Option<Box<dyn Element>> {
        let editor = self.host_password_editor.as_ref()?;
        let intent = self.host_password_intent.as_ref()?;
        let (prompt_key, confirm_key, busy_key) = match intent {
            HostPasswordIntent::Export => (
                "host_export_password_prompt",
                "host_export_confirm",
                "host_export_busy",
            ),
            HostPasswordIntent::Import { .. } => (
                "host_import_password_prompt",
                "host_import_confirm",
                "host_import_busy",
            ),
        };
        let hc = HostUiColors::from_theme(&self.cached_warp_theme);
        let label = Text::new_inline(rust_i18n::t!(prompt_key).to_string(), self.ui_font, 11.0)
            .with_color(hc.text_secondary)
            .finish();
        let ui_font = self.ui_font;
        let busy = self.host_password_busy;

        let row: Box<dyn Element> = if busy {
            // 后台跑 PBKDF2/AES：用省略号循环模拟进度。每 ~300ms 换一帧；
            // ctx.spawn 完成后会 notify 触发整体重渲染（这里无需 timer）。
            let dots_count = (chrono::Local::now().timestamp_millis() / 300) % 4;
            let dots = ".".repeat(dots_count as usize);
            let busy_text = format!("{}{}", rust_i18n::t!(busy_key), dots);
            let processing = Text::new_inline(busy_text, ui_font, 12.0)
                .with_color(hc.text_primary)
                .finish();
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Expanded::new(1.0, processing).finish())
                .finish()
        } else {
            let input = TextInput::new(
                editor.clone(),
                UiComponentStyles::default()
                    .set_background(Fill::None)
                    .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .set_border_width(1.0),
            )
            .build()
            .finish();
            let confirm_label = rust_i18n::t!(confirm_key).to_string();
            let confirm_btn =
                Hoverable::new(self.host_password_confirm_state.clone(), move |mouse| {
                    let bg = if mouse.is_hovered() {
                        hc.accent_bg
                    } else {
                        hc.card_bg_hover
                    };
                    let fg = if mouse.is_hovered() {
                        hc.accent_text
                    } else {
                        hc.text_primary
                    };
                    Container::new(
                        Text::new_inline(confirm_label.clone(), ui_font, 12.0)
                            .with_color(fg)
                            .finish(),
                    )
                    .with_horizontal_padding(12.0)
                    .with_vertical_padding(6.0)
                    .with_background_color(bg)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .finish()
                })
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::HostPasswordConfirm);
                })
                .finish();
            let cancel_btn =
                Hoverable::new(self.host_password_cancel_state.clone(), move |mouse| {
                    let bg = if mouse.is_hovered() {
                        hc.card_bg_hover
                    } else {
                        ColorU::transparent_black()
                    };
                    Container::new(
                        Text::new_inline(
                            rust_i18n::t!("host_export_cancel").to_string(),
                            ui_font,
                            12.0,
                        )
                        .with_color(hc.text_secondary)
                        .finish(),
                    )
                    .with_horizontal_padding(12.0)
                    .with_vertical_padding(6.0)
                    .with_background_color(bg)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
                    .finish()
                })
                .with_cursor(warpui::platform::Cursor::PointingHand)
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(TerminalGridAction::HostPasswordCancel);
                })
                .finish();
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Expanded::new(1.0, input).finish())
                .with_child(Container::new(confirm_btn).with_margin_left(8.0).finish())
                .with_child(Container::new(cancel_btn).with_margin_left(6.0).finish())
                .finish()
        };

        let col = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Container::new(label).with_padding_bottom(6.0).finish())
            .with_child(row)
            .finish();
        Some(
            Container::new(col)
                .with_horizontal_padding(16.0)
                .with_vertical_padding(10.0)
                .with_background_color(hc.search_bar_bg)
                .with_border(Border::bottom(1.0).with_border_color(hc.search_bar_border))
                .finish(),
        )
    }
}
