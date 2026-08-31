// rdp_view::page_element — RDP 整页画面 Element。
// paint 里取 image cache 里按 asset_id 缓存的当前帧位图，letterbox 居中绘制。
// 纹理上传不在这里做（由 root_view 帧事件按 generation 触发 insert_raw_asset_bytes）；
// 本 Element 只读缓存 + 计算几何 + draw_image，并把 letterbox 矩形回写供第 ④ 步鼠标反算。

use std::sync::{Arc, Mutex};

use pathfinder_color::ColorU;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{Vector2F, Vector2I};

use warpui_core::assets::asset_cache::{AssetCache, AssetSource, AssetState};
use warpui_core::elements::{
    AfterLayoutContext, CornerRadius, Element, EventContext, LayoutContext, Point, SizeConstraint,
};
use warpui_core::event::{DispatchedEvent, Event, KeyEventDetails, KeyState};
use warpui_core::image_cache::{AnimatedImageBehavior, CacheOption, FitType, Image, ImageCache};
use warpui_core::keymap::Keystroke;
use warpui_core::{AppContext, PaintContext, SingletonEntity};

use crate::rdp_view::geometry::{letterbox_rect, viewport_device_coords, RdpViewport};
use crate::rdp_view::keymap;
use nexshell::rdp_session::{RdpButton, RdpInputEvent};

pub struct RdpPageElement {
    /// image cache 中当前帧的稳定键（每 RDP 会话一个，逐帧覆盖，不堆积）。
    asset_id: String,
    /// 远端桌面像素尺寸（= framebuffer 宽高），letterbox 的源尺寸。
    desktop_size: Vector2I,
    /// 无帧 / letterbox 黑边填充色。
    background: ColorU,
    /// 回写当前 letterbox 几何（绝对坐标），供鼠标→远端坐标反算。
    viewport_out: Arc<Mutex<Option<RdpViewport>>>,
    /// 键鼠事件出口（会话线程消费编码成 FastPath）。满/断开则丢弃。
    input_tx: async_channel::Sender<RdpInputEvent>,
    /// 上次发出的鼠标远端坐标（跨帧共享；Element 每帧重建）。去重连续 MouseMove 用。
    last_mouse: Arc<Mutex<Option<(u16, u16)>>>,
    /// 修饰键持续对账器（与 RdpTabState 共用）：每个键鼠事件按本地 flags 补发丢失的 keyup，防 Alt 粘滞。
    mod_tracker: Arc<Mutex<keymap::ModifierTracker>>,
    /// 远端接管光标：鼠标在画面内时套用（每帧由 RootView 传入当前值）。
    cursor: warpui_core::platform::Cursor,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RdpPageElement {
    pub fn new(
        asset_id: String,
        desktop_size: Vector2I,
        background: ColorU,
        viewport_out: Arc<Mutex<Option<RdpViewport>>>,
        input_tx: async_channel::Sender<RdpInputEvent>,
        last_mouse: Arc<Mutex<Option<(u16, u16)>>>,
        mod_tracker: Arc<Mutex<keymap::ModifierTracker>>,
        cursor: warpui_core::platform::Cursor,
    ) -> Self {
        Self {
            asset_id,
            desktop_size,
            background,
            viewport_out,
            input_tx,
            last_mouse,
            mod_tracker,
            cursor,
            size: None,
            origin: None,
        }
    }

    /// 发一个输入事件；满或断开则丢弃（丢新的），绝不阻塞 UI 线程。返回 try_send 是否成功。
    fn send(&self, event: RdpInputEvent) -> bool {
        self.input_tx.try_send(event).is_ok()
    }

    /// 对账：本地 flags 显示某修饰键未按而 tracker 记为按下 → 立即补发 release。防远端修饰键粘滞。
    fn reconcile_modifiers(&self, flags: keymap::ModifierFlags) {
        let Ok(mut tracker) = self.mod_tracker.lock() else {
            return;
        };
        for event in tracker.reconcile(flags) {
            self.send(event);
            if key_trace() {
                eprintln!("[nexshell key-debug] page reconcile 补发 release {event:?}");
            }
        }
    }

    /// 远端光标接管：鼠标在画面内套用远端下发光标，画面外（黑边）恢复箭头。
    /// set_cursor 只在事件分发帧生效；合成 MouseMoved 每帧补发保证跟随。
    fn apply_cursor(&self, position: Vector2F, ctx: &mut EventContext) {
        let Some(z_index) = self.origin.map(|o| o.z_index()) else {
            return;
        };
        let cursor = if self.device_coords(position).is_some() {
            self.cursor
        } else {
            warpui_core::platform::Cursor::Arrow
        };
        ctx.set_cursor(cursor, z_index);
    }

    /// 鼠标绝对坐标 → 远端桌面像素；画面外（黑边）或无 viewport 返回 None。
    fn device_coords(&self, position: Vector2F) -> Option<(u16, u16)> {
        let vp = (*self.viewport_out.lock().ok()?)?;
        viewport_device_coords(&vp, position, self.desktop_size)
    }

    /// 鼠标移动 → 远端 MouseMove。远端坐标未变则不重发（去抖 + 省流）。返回是否已消费。
    fn send_mouse_move(&self, position: Vector2F) -> bool {
        let Some((x, y)) = self.device_coords(position) else {
            return false;
        };
        if let Ok(mut last) = self.last_mouse.lock() {
            if *last == Some((x, y)) {
                return true; // 坐标未变，已处理但不重发
            }
            *last = Some((x, y));
        }
        log_input(position, x, y);
        self.send(RdpInputEvent::MouseMove { x, y });
        true
    }

    /// 右/中键：warpui 不派发其抬起事件，按下即合成完整 click（down+up）。
    /// Windows 右键菜单在 up 弹出，故成对发送即可正常触发。
    fn send_synthetic_click(&self, position: Vector2F, button: RdpButton) -> bool {
        let Some((x, y)) = self.device_coords(position) else {
            return false;
        };
        self.send(RdpInputEvent::MouseButton {
            button,
            pressed: true,
            x,
            y,
        });
        self.send(RdpInputEvent::MouseButton {
            button,
            pressed: false,
            x,
            y,
        });
        true
    }

    /// 滚轮：垂直 + 水平各发一次（有位移才发）。
    fn send_wheel(&self, position: Vector2F, delta: Vector2F, precise: bool) -> bool {
        let Some((x, y)) = self.device_coords(position) else {
            return false;
        };
        let mut handled = false;
        for (axis_delta, horizontal) in [(delta.y(), false), (delta.x(), true)] {
            let units = wheel_units(axis_delta, precise);
            if units == 0 {
                continue;
            }
            self.send(RdpInputEvent::Wheel {
                horizontal,
                delta: units,
                x,
                y,
            });
            handled = true;
        }
        handled
    }

    /// 普通键 KeyDown：只发 press 并记入按住集合；OS 自动重复（is_repeat）不发，远端自己 typematic。
    /// 未映射 / 输入法合成中返回 false 让事件冒泡。
    fn handle_key_down(
        &self,
        keystroke: &Keystroke,
        details: &KeyEventDetails,
        is_composing: bool,
        is_repeat: bool,
    ) -> bool {
        if is_composing {
            if key_trace() {
                eprintln!(
                    "[nexshell key-debug] page KeyDown key={:?} is_composing=true → 跳过",
                    keystroke.key
                );
            }
            return false;
        }
        // keystroke 自带完整修饰键 flags，借普通键按下对账补发丢失的 keyup。
        self.reconcile_modifiers(keymap::ModifierFlags::full(
            keystroke.shift,
            keystroke.ctrl,
            keystroke.alt,
            keystroke.cmd,
        ));
        let Some((scancode, extended)) = key_scancode(keystroke, details) else {
            if key_trace() {
                eprintln!(
                    "[nexshell key-debug] page KeyDown key={:?} kwm={:?} scancode=miss → 不消费",
                    keystroke.key, details.key_without_modifiers
                );
            }
            return false;
        };
        if is_repeat {
            return true;
        }
        let first = self
            .mod_tracker
            .lock()
            .map(|mut t| t.press_key(scancode, extended))
            .unwrap_or(true);
        let sent = first
            && self.send(RdpInputEvent::Key {
                scancode,
                extended,
                pressed: true,
            });
        if key_trace() {
            eprintln!(
                "[nexshell key-debug] page KeyDown key={:?} scancode=0x{:02X} ext={} first={} try_send={}",
                keystroke.key, scancode, extended, first, sent
            );
        }
        true
    }

    /// 普通键 KeyUp：发 release 并移出按住集合；不在集合也发一次（保险），不重复。
    fn handle_key_up(&self, keystroke: &Keystroke, details: &KeyEventDetails) -> bool {
        let Some((scancode, extended)) = key_scancode(keystroke, details) else {
            return false;
        };
        let tracked = self
            .mod_tracker
            .lock()
            .map(|mut t| t.release_key(scancode, extended))
            .unwrap_or(false);
        let sent = self.send(RdpInputEvent::Key {
            scancode,
            extended,
            pressed: false,
        });
        if key_trace() {
            eprintln!(
                "[nexshell key-debug] page KeyUp key={:?} scancode=0x{:02X} ext={} tracked={} try_send={}",
                keystroke.key, scancode, extended, tracked, sent
            );
        }
        true
    }
}

/// 普通键无硬件 keycode：key_without_modifiers（基础字符）优先查表，退回归一化 key
/// （覆盖 enter/方向等特殊键名）。KeyDown / KeyUp 共用同一降级路径保证成对。
fn key_scancode(keystroke: &Keystroke, details: &KeyEventDetails) -> Option<(u8, bool)> {
    details
        .key_without_modifiers
        .as_deref()
        .and_then(|k| keymap::scancode_for_key(&k.to_lowercase()))
        .or_else(|| keymap::scancode_for_key(&keystroke.key.to_lowercase()))
}

/// 诊断（NEXSHELL_RDP_INPUT_LOG=<file>）：把去重后实发的 MouseMove 逐条追加落盘，
/// 含原始逻辑坐标与反算远端坐标，供真机抖动溯源（看远端坐标是否在静止时仍振荡）。
fn log_input(pos: Vector2F, x: u16, y: u16) {
    use std::io::Write;
    let Some(path) = std::env::var_os("NEXSHELL_RDP_INPUT_LOG") else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "move logical=({:.2},{:.2}) remote=({x},{y})",
            pos.x(),
            pos.y()
        );
    }
}

/// 滚轮位移 → RDP rotation units（一格=120）。line 模式 delta 为行数，pixel 模式为像素。
/// set-1 只编码低字节量级，clamp 到 ±255 防截断反号。
fn wheel_units(delta: f32, precise: bool) -> i16 {
    if delta.abs() < f32::EPSILON {
        return 0;
    }
    let raw = if precise { delta } else { delta * 120.0 };
    raw.round().clamp(-255.0, 255.0) as i16
}

impl Element for RdpPageElement {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _: &mut LayoutContext,
        _: &AppContext,
    ) -> Vector2F {
        // 占满内容区；画面通过 paint 内 letterbox 居中。
        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let size = self.size.unwrap_or_default();

        // 背景铺满（含 letterbox 黑边）。
        ctx.scene
            .draw_rect_without_hit_recording(RectF::new(origin, size))
            .with_background(self.background);

        if size.x() <= 0.0 || size.y() <= 0.0 {
            *self.viewport_out.lock().unwrap() = None;
            return;
        }

        // letterbox 几何（相对内容区）→ 叠加绝对 origin 后回写。
        let vp = letterbox_rect(size, self.desktop_size);
        let abs_rect = RectF::new(origin + vp.content_rect.origin(), vp.content_rect.size());
        *self.viewport_out.lock().unwrap() = Some(RdpViewport {
            content_rect: abs_rect,
            scale: vp.scale,
        });

        // 取当前帧位图。CacheOption::Original：不做 CPU 缩放缓存，每帧读最新字节，
        // 由 GPU 采样到 letterbox 矩形——避免「稳定 key + 已缩放缓存」返回旧帧。
        let asset_cache = AssetCache::as_ref(app);
        let image = ImageCache::as_ref(app).image(
            AssetSource::Raw {
                id: self.asset_id.clone(),
            },
            self.desktop_size,
            FitType::Contain,
            AnimatedImageBehavior::FullAnimation,
            CacheOption::Original,
            ctx.max_texture_dimension_2d,
            asset_cache,
        );

        // 首帧到达前 / 被逐出时读不到位图：只留背景，等下一帧重传。
        if let AssetState::Loaded { data } = image {
            if let Image::Static(static_image) = data.as_ref() {
                ctx.scene
                    .draw_image(abs_rect, static_image.clone(), 1.0, CornerRadius::default());
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    // 键鼠输入 → RDP FastPath。本地已注册快捷键（⌘T/⌘W 等）由框架 dispatch_keystroke
    // 先消费，未消费的 KeyDown 才落到这里，故本地优先天然保证（无需在此另判）。
    // 命中画面内则消费(true)，画面外(letterbox 黑边)/未映射键返回 false 让事件冒泡。
    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        _: &AppContext,
    ) -> bool {
        match event.raw_event() {
            // 普通键：KeyDown 只发 press（自动重复不发，远端自己 typematic），KeyUp 发 release。
            Event::KeyDown {
                keystroke,
                details,
                is_composing,
                is_repeat,
                ..
            } => self.handle_key_down(keystroke, details, *is_composing, *is_repeat),
            Event::KeyUp { keystroke, details } => self.handle_key_up(keystroke, details),
            // 修饰键：携带物理 KeyCode + 按下/抬起，维持远端修饰键状态。
            Event::ModifierKeyChanged { key_code, state } => {
                let Some((scancode, extended)) = keymap::scancode_for_modifier(*key_code) else {
                    if key_trace() {
                        eprintln!(
                            "[nexshell key-debug] page Modifier code={:?} state={:?} scancode=miss → 不消费",
                            key_code, state
                        );
                    }
                    return false;
                };
                let pressed = matches!(state, KeyState::Pressed);
                let sent = self.send(RdpInputEvent::Key {
                    scancode,
                    extended,
                    pressed,
                });
                // 记账：本键 down/up 已发，供后续事件对账。
                if let Ok(mut tracker) = self.mod_tracker.lock() {
                    tracker.on_sent(scancode, extended, pressed);
                }
                if key_trace() {
                    eprintln!(
                        "[nexshell key-debug] page Modifier code={:?} scancode=0x{:02X} ext={} pressed={} try_send={}",
                        key_code, scancode, extended, pressed, sent
                    );
                }
                true
            }
            // 合成 MouseMoved（每帧重绘后 warpui 补发，用于刷新 hover）：拖拽期间其坐标冻结在
            // 拖拽前的最后一次真实移动处（≈初始位置），透传给远端会与真实拖拽交替，把窗口反复
            // 拽回初始位（视频/游戏窗每帧重绘时尤甚）→ 丢弃。真实移动 is_synthetic=false 照常。
            Event::MouseMoved {
                position,
                cmd,
                shift,
                is_synthetic,
            } => {
                // 合成 MouseMoved 每帧补发：借它把远端光标持续套用（画面内）/恢复（画面外）。
                self.apply_cursor(*position, ctx);
                if *is_synthetic {
                    return true;
                }
                // 真实移动只带 cmd/shift（无 ctrl/alt）：仅对账这两类，避免误抬 Alt。
                self.reconcile_modifiers(keymap::ModifierFlags::cmd_shift(*cmd, *shift));
                self.send_mouse_move(*position)
            }
            // 左键拖拽。丢弃平台层合成拖拽（静止自动滚动泵：坐标冻结、每 16ms 重发）。
            Event::LeftMouseDragged {
                position,
                modifiers,
            } => {
                self.apply_cursor(*position, ctx);
                if warpui_core::event::is_synthetic_drag() {
                    return true;
                }
                self.reconcile_modifiers(mods_flags(*modifiers));
                self.send_mouse_move(*position)
            }
            Event::LeftMouseDown {
                position,
                modifiers,
                ..
            } => {
                self.reconcile_modifiers(mods_flags(*modifiers));
                let Some((x, y)) = self.device_coords(*position) else {
                    return false;
                };
                self.send(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: true,
                    x,
                    y,
                });
                true
            }
            Event::LeftMouseUp {
                position,
                modifiers,
            } => {
                self.reconcile_modifiers(mods_flags(*modifiers));
                let Some((x, y)) = self.device_coords(*position) else {
                    return false;
                };
                self.send(RdpInputEvent::MouseButton {
                    button: RdpButton::Left,
                    pressed: false,
                    x,
                    y,
                });
                true
            }
            Event::RightMouseDown {
                position,
                cmd,
                shift,
                ..
            } => {
                self.reconcile_modifiers(keymap::ModifierFlags::cmd_shift(*cmd, *shift));
                self.send_synthetic_click(*position, RdpButton::Right)
            }
            Event::MiddleMouseDown {
                position,
                cmd,
                shift,
                ..
            } => {
                self.reconcile_modifiers(keymap::ModifierFlags::cmd_shift(*cmd, *shift));
                self.send_synthetic_click(*position, RdpButton::Middle)
            }
            Event::ScrollWheel {
                position,
                delta,
                precise,
                modifiers,
            } => {
                self.reconcile_modifiers(mods_flags(*modifiers));
                self.send_wheel(*position, *delta, *precise)
            }
            _ => false,
        }
    }
}

/// warpui 完整 ModifiersState → keymap 对账 flags（⌘→Win 归入 command 类）。
fn mods_flags(m: warpui_core::event::ModifiersState) -> keymap::ModifierFlags {
    keymap::ModifierFlags::full(m.shift, m.ctrl, m.alt, m.cmd)
}

/// 按键链路追踪开关（NEXSHELL_DEBUG_KEYS=1，与 warpui 平台层同开关）。
fn key_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NEXSHELL_DEBUG_KEYS").is_ok_and(|v| v == "1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element() -> (RdpPageElement, async_channel::Receiver<RdpInputEvent>) {
        let (tx, rx) = async_channel::bounded(16);
        let el = RdpPageElement::new(
            "rdp:test".into(),
            Vector2I::new(800, 600),
            ColorU::new(0, 0, 0, 255),
            Arc::new(Mutex::new(None)),
            tx,
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(keymap::ModifierTracker::default())),
            warpui_core::platform::Cursor::Arrow,
        );
        (el, rx)
    }

    fn drain(rx: &async_channel::Receiver<RdpInputEvent>) -> Vec<RdpInputEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn key(pressed: bool) -> RdpInputEvent {
        RdpInputEvent::Key {
            scancode: 0x11, // W
            extended: false,
            pressed,
        }
    }

    fn w() -> (Keystroke, KeyEventDetails) {
        let ks = Keystroke {
            key: "w".into(),
            ..Default::default()
        };
        let details = KeyEventDetails {
            key_without_modifiers: Some("w".into()),
            ..Default::default()
        };
        (ks, details)
    }

    #[test]
    fn key_down_sends_press_only() {
        let (el, rx) = element();
        let (ks, d) = w();
        assert!(el.handle_key_down(&ks, &d, false, false));
        assert_eq!(drain(&rx), vec![key(true)]);
    }

    #[test]
    fn repeat_key_down_sends_nothing() {
        let (el, rx) = element();
        let (ks, d) = w();
        el.handle_key_down(&ks, &d, false, false);
        drain(&rx);
        assert!(el.handle_key_down(&ks, &d, false, true));
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn key_up_sends_release_and_untracks() {
        let (el, rx) = element();
        let (ks, d) = w();
        el.handle_key_down(&ks, &d, false, false);
        assert!(el.handle_key_up(&ks, &d));
        assert_eq!(drain(&rx), vec![key(true), key(false)]);
        // 未按住也发一次 release（保险），集合已空。
        assert!(el.handle_key_up(&ks, &d));
        assert_eq!(drain(&rx), vec![key(false)]);
        assert!(el.mod_tracker.lock().unwrap().drain_held_keys().is_empty());
    }

    #[test]
    fn composing_and_unmapped_bubble() {
        let (el, rx) = element();
        let (ks, d) = w();
        assert!(!el.handle_key_down(&ks, &d, true, false));
        let unknown = Keystroke {
            key: "f24".into(),
            ..Default::default()
        };
        assert!(!el.handle_key_down(&unknown, &KeyEventDetails::default(), false, false));
        assert!(!el.handle_key_up(&unknown, &KeyEventDetails::default()));
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn release_all_drains_held_keys() {
        let (el, rx) = element();
        let (ks, d) = w();
        el.handle_key_down(&ks, &d, false, false);
        drain(&rx);
        let mut tracker = el.mod_tracker.lock().unwrap();
        assert_eq!(tracker.drain_held_keys(), vec![key(false)]);
        tracker.clear();
        assert!(tracker.drain_held_keys().is_empty());
    }
}
