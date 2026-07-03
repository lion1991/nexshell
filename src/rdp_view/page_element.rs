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
use warpui_core::event::{DispatchedEvent, Event, KeyState};
use warpui_core::image_cache::{AnimatedImageBehavior, CacheOption, FitType, Image, ImageCache};
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
    ) -> Self {
        Self {
            asset_id,
            desktop_size,
            background,
            viewport_out,
            input_tx,
            size: None,
            origin: None,
        }
    }

    /// 发一个输入事件；满或断开则丢弃（丢新的），绝不阻塞 UI 线程。
    fn send(&self, event: RdpInputEvent) {
        let _ = self.input_tx.try_send(event);
    }

    /// 鼠标绝对坐标 → 远端桌面像素；画面外（黑边）或无 viewport 返回 None。
    fn device_coords(&self, position: Vector2F) -> Option<(u16, u16)> {
        let vp = (*self.viewport_out.lock().ok()?)?;
        viewport_device_coords(&vp, position, self.desktop_size)
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
        _ctx: &mut EventContext,
        _: &AppContext,
    ) -> bool {
        match event.raw_event() {
            // 普通键：无硬件 keycode，用 key_without_modifiers（基础字符）优先查表，
            // 退回归一化 key（覆盖 enter/方向等特殊键名）。warpui 不给 keyup，按下即发 down+up。
            Event::KeyDown {
                keystroke,
                details,
                is_composing,
                ..
            } => {
                if *is_composing {
                    return false;
                }
                let scancode = details
                    .key_without_modifiers
                    .as_deref()
                    .and_then(|k| keymap::scancode_for_key(&k.to_lowercase()))
                    .or_else(|| keymap::scancode_for_key(&keystroke.key.to_lowercase()));
                let Some((scancode, extended)) = scancode else {
                    return false;
                };
                self.send(RdpInputEvent::Key {
                    scancode,
                    extended,
                    pressed: true,
                });
                self.send(RdpInputEvent::Key {
                    scancode,
                    extended,
                    pressed: false,
                });
                true
            }
            // 修饰键：携带物理 KeyCode + 按下/抬起，维持远端修饰键状态。
            Event::ModifierKeyChanged { key_code, state } => {
                let Some((scancode, extended)) = keymap::scancode_for_modifier(*key_code) else {
                    return false;
                };
                self.send(RdpInputEvent::Key {
                    scancode,
                    extended,
                    pressed: matches!(state, KeyState::Pressed),
                });
                true
            }
            // 移动（含左键拖拽）。
            Event::MouseMoved { position, .. } | Event::LeftMouseDragged { position, .. } => {
                let Some((x, y)) = self.device_coords(*position) else {
                    return false;
                };
                self.send(RdpInputEvent::MouseMove { x, y });
                true
            }
            Event::LeftMouseDown { position, .. } => {
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
            Event::LeftMouseUp { position, .. } => {
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
            Event::RightMouseDown { position, .. } => {
                self.send_synthetic_click(*position, RdpButton::Right)
            }
            Event::MiddleMouseDown { position, .. } => {
                self.send_synthetic_click(*position, RdpButton::Middle)
            }
            Event::ScrollWheel {
                position,
                delta,
                precise,
                ..
            } => self.send_wheel(*position, *delta, *precise),
            _ => false,
        }
    }
}
