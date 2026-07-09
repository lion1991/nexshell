// 容器管理视图：按主机分组的容器卡片网格（ServerCat Containers 风格）。
// 数据来自 RootView 的 ContainerFleet；操作菜单通过 TerminalGridAction 派发给 RootView。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pathfinder_geometry::{rect::RectF, vector::vec2f};
use warpui::{
    color::ColorU,
    elements::{
        AfterLayoutContext, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
        Empty, Expanded, Flex, Hoverable, LayoutContext, MainAxisSize, MouseState,
        MouseStateHandle, ParentElement, Point, Radius, SizeConstraint, Text,
    },
    event::DispatchedEvent,
    fonts, AppContext, Element, EventContext, PaintContext,
};

use nexshell::container_fleet::ContainerFleet;
use nexshell::container_overview::{
    ContainerCollectStatus, ContainerHealth, ContainerInfo, ContainerOverviewUiState,
    ContainerProbeError, ContainerState,
};
use nexshell::host_management::HostCardSnapshot;
use nexshell::host_overview::split_rate;

use crate::host_management_view::constants::*;
use crate::terminal_grid_element::TerminalGridAction;

const CONTAINER_GRID_COLUMNS: usize = 2;
const LOADING_DOT_COUNT: usize = 3;
const LOADING_DOT_SIZE: f32 = 7.0;
const LOADING_DOT_GAP: f32 = 7.0;
const LOADING_DOT_CYCLE: f32 = 0.9;

/// 容器卡片 "⋯" 菜单按钮的 hover 状态，按容器 id 持久（容器列表随刷新变化，用 map 而非 Vec）。
#[derive(Default)]
pub struct ContainerCardStates {
    menu_button_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// 卡片整卡右键的 hover 状态占位（不驱动视觉），key 规则同 menu_button_states。
    card_hover_states: RefCell<HashMap<String, MouseStateHandle>>,
    /// CPU 环 sweep 过渡（key = "{host_id}:{container_id}"）。
    pub gauge_anim: RefCell<nexshell::ui_anim::FloatTransitionMap<String>>,
    /// 容器名搜索框 hover 态，供 render_search_input 用。
    pub search_input_state: MouseStateHandle,
    /// 页面级加载动画是否活跃；RootView 的 16ms UI tick 读取它来驱动重绘。
    pub loading_animating: Cell<bool>,
    loading_started_at: RefCell<Option<Instant>>,
}

impl ContainerCardStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// key = "{host_id}:{container_id}"，与 gauge_anim 同规则，按主机前缀清理。
    fn menu_button_state(&self, key: String) -> MouseStateHandle {
        self.menu_button_states
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone()
    }

    fn card_hover_state(&self, key: String) -> MouseStateHandle {
        self.card_hover_states
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(MouseState::default())))
            .clone()
    }

    fn loading_started_at(&self, now: Instant) -> Instant {
        let mut started_at = self.loading_started_at.borrow_mut();
        *started_at.get_or_insert(now)
    }

    fn set_loading_animating(&self, loading: bool) {
        self.loading_animating.set(loading);
        if !loading {
            *self.loading_started_at.borrow_mut() = None;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContainerPageFallback {
    None,
    Loading,
    Empty,
    SearchNoMatch,
}

pub fn render_container_view(
    hosts: &[HostCardSnapshot],
    fleet: &ContainerFleet,
    states: &ContainerCardStates,
    query: &str,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let ssh_hosts: Vec<&HostCardSnapshot> = hosts
        .iter()
        .filter(|host| host.protocol.eq_ignore_ascii_case("SSH"))
        .collect();
    if ssh_hosts.is_empty() {
        states.set_loading_animating(false);
        return Container::new(Empty::new().finish())
            .with_background_color(hc.panel_bg)
            .finish();
    }

    // 清理已消失主机的动画/按钮状态（key 前缀 = host_id，容器随主机走）。
    let now = Instant::now();
    let host_alive = |key: &str| -> bool { ssh_hosts.iter().any(|host| key.starts_with(&host.id)) };
    states.gauge_anim.borrow_mut().retain(|key| host_alive(key));
    states
        .menu_button_states
        .borrow_mut()
        .retain(|key, _| host_alive(key));
    states
        .card_hover_states
        .borrow_mut()
        .retain(|key, _| host_alive(key));

    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    let needle = query.trim().to_lowercase();
    let searching = !needle.is_empty();
    let mut any_visible = false;
    let mut any_loading = false;
    let mut all_hosts_empty = true;
    for host in ssh_hosts.iter() {
        let state = fleet.state(&host.id);
        any_loading |= host_container_is_loading(state);
        all_hosts_empty &= host_has_empty_container_conclusion(state);
        // 搜索态：无匹配容器的主机整节隐藏（不管加载/错误/无 docker）；非搜索态走原隐藏判定。
        let hidden = if searching {
            !host_has_match(state, &needle)
        } else {
            host_section_hidden(state)
        };
        if hidden {
            continue;
        }
        if any_visible {
            col.add_child(spacer_v(24.0));
        }
        col.add_child(render_host_section(
            host, state, states, now, &needle, searching, ui_font, hc,
        ));
        any_visible = true;
    }
    let page_fallback =
        container_page_fallback(searching, any_visible, any_loading, all_hosts_empty);
    states.set_loading_animating(page_fallback == ContainerPageFallback::Loading);
    match page_fallback {
        ContainerPageFallback::SearchNoMatch => {
            col.add_child(render_none_visible_placeholder_text(
                rust_i18n::t!("container_search_no_match").to_string(),
                ui_font,
                hc,
            ));
        }
        ContainerPageFallback::Loading => {
            col.add_child(render_loading_placeholder(
                states.loading_started_at(now),
                now,
                ui_font,
                hc,
            ));
        }
        ContainerPageFallback::Empty => {
            col.add_child(render_none_visible_placeholder_text(
                rust_i18n::t!("host_container_none_visible").to_string(),
                ui_font,
                hc,
            ));
        }
        ContainerPageFallback::None => {}
    }

    Container::new(col.finish())
        .with_horizontal_padding(24.0)
        .with_vertical_padding(CARD_SPACING)
        .with_background_color(hc.panel_bg)
        .finish()
}

/// 该主机是否存在名称匹配 needle（已 lowercase）的容器；无状态（尚未采集）视为不匹配。
fn host_has_match(state: Option<&ContainerOverviewUiState>, needle: &str) -> bool {
    state.map_or(false, |ui| {
        ui.snapshot
            .containers
            .iter()
            .any(|c| c.name.to_lowercase().contains(needle))
    })
}

fn container_page_fallback(
    searching: bool,
    any_visible: bool,
    any_loading: bool,
    all_hosts_empty: bool,
) -> ContainerPageFallback {
    if any_visible {
        return ContainerPageFallback::None;
    }
    if searching {
        return ContainerPageFallback::SearchNoMatch;
    }
    if any_loading {
        return ContainerPageFallback::Loading;
    }
    if all_hosts_empty {
        return ContainerPageFallback::Empty;
    }
    ContainerPageFallback::None
}

fn host_container_is_loading(state: Option<&ContainerOverviewUiState>) -> bool {
    let Some(ui) = state else { return true };
    let snap = &ui.snapshot;
    !snap.has_collected_data()
        && !matches!(
            snap.status,
            ContainerCollectStatus::Ready | ContainerCollectStatus::Error(_)
        )
}

fn host_has_empty_container_conclusion(state: Option<&ContainerOverviewUiState>) -> bool {
    let Some(ui) = state else { return false };
    let snap = &ui.snapshot;
    snap.containers.is_empty()
        && matches!(
            (&snap.status, &snap.error),
            (ContainerCollectStatus::Ready, None)
                | (
                    ContainerCollectStatus::Ready,
                    Some(ContainerProbeError::NoDocker)
                )
        )
}

/// 隐藏无意义的整节：从未采到数据的过渡/失败态（初次等待/重连采集中/连不上）、未装 docker、已就绪且无容器。
/// 保留：有存量容器的瞬时错误（操作失败，has_collected_data 为真）、权限拒绝、其他错误、有容器。
fn host_section_hidden(state: Option<&ContainerOverviewUiState>) -> bool {
    let Some(ui) = state else { return true };
    let snap = &ui.snapshot;
    // 从未采到有效数据、且未落到 Ready 的态（Waiting/Collecting/连不上的 Error）一律隐藏——
    // 重连的 Collecting 占位帧不再闪「加载中」；有存量容器的瞬时 Error 因 has_collected_data 为真而保留。
    if !snap.has_collected_data() && snap.status != ContainerCollectStatus::Ready {
        return true;
    }
    // 未装 docker
    if matches!(snap.error, Some(ContainerProbeError::NoDocker)) {
        return true;
    }
    // 已就绪且无容器
    snap.containers.is_empty()
        && snap.error.is_none()
        && snap.status == ContainerCollectStatus::Ready
}

/// 全部主机节被隐藏时的居中弱提示，避免整页空白（文案外传：无结论留空 / 无匹配 / 无运行容器）。
fn render_none_visible_placeholder_text(
    text: String,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(
                Text::new_inline(text, ui_font, UI_FONT_SIZE)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_padding_top(40.0)
            .finish(),
        )
        .finish()
}

fn render_loading_placeholder(
    started_at: Instant,
    now: Instant,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(LoadingDots::new(started_at, now, hc.text_secondary).finish())
                .with_padding_top(34.0)
                .with_padding_bottom(12.0)
                .finish(),
        )
        .with_child(
            Text::new_inline(
                rust_i18n::t!("host_container_loading").to_string(),
                ui_font,
                UI_FONT_SIZE,
            )
            .with_color(hc.text_secondary)
            .finish(),
        )
        .finish()
}

struct LoadingDots {
    started_at: Instant,
    now: Instant,
    color: ColorU,
    origin: Option<Point>,
}

impl LoadingDots {
    fn new(started_at: Instant, now: Instant, color: ColorU) -> Self {
        Self {
            started_at,
            now,
            color,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }

    fn size_vec() -> pathfinder_geometry::vector::Vector2F {
        vec2f(
            LOADING_DOT_COUNT as f32 * LOADING_DOT_SIZE
                + LOADING_DOT_COUNT.saturating_sub(1) as f32 * LOADING_DOT_GAP,
            LOADING_DOT_SIZE,
        )
    }
}

impl Element for LoadingDots {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> pathfinder_geometry::vector::Vector2F {
        Self::size_vec()
    }

    fn after_layout(&mut self, _ctx: &mut AfterLayoutContext, _app: &AppContext) {}

    fn paint(
        &mut self,
        origin: pathfinder_geometry::vector::Vector2F,
        ctx: &mut PaintContext,
        _app: &AppContext,
    ) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let phase = loading_phase(self.now.saturating_duration_since(self.started_at));
        for dot in 0..LOADING_DOT_COUNT {
            let x = dot as f32 * (LOADING_DOT_SIZE + LOADING_DOT_GAP);
            let mut color = self.color;
            color.a = loading_dot_alpha(phase, dot);
            ctx.scene
                .draw_rect_without_hit_recording(RectF::new(
                    origin + vec2f(x, 0.0),
                    vec2f(LOADING_DOT_SIZE, LOADING_DOT_SIZE),
                ))
                .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.0)))
                .with_background(color);
        }
    }

    fn size(&self) -> Option<pathfinder_geometry::vector::Vector2F> {
        Some(Self::size_vec())
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

fn loading_phase(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() / LOADING_DOT_CYCLE).fract()
}

fn loading_dot_alpha(phase: f32, dot: usize) -> u8 {
    let dot_phase = dot as f32 / LOADING_DOT_COUNT as f32;
    let wave = ((phase - dot_phase) * std::f32::consts::TAU).cos() * 0.5 + 0.5;
    (0x55 as f32 + wave * 0xaa as f32).round() as u8
}

/// 单主机节：标题 + 容器双列网格 / 异常占位文案。
fn render_host_section(
    host: &HostCardSnapshot,
    state: Option<&ContainerOverviewUiState>,
    states: &ContainerCardStates,
    now: Instant,
    needle: &str,
    searching: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut section = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    section.add_child(
        Container::new(bold_text(host.name.clone(), ui_font, 14.0, hc.text_primary))
            .with_margin_bottom(12.0)
            .finish(),
    );

    if let Some(err) = state
        .and_then(|ui| ui.action_error.as_ref())
        .map(|(e, _)| e)
    {
        section.add_child(render_action_error(err, ui_font, hc));
    }

    section.add_child(match state {
        Some(ui) => render_host_body(
            &host.id,
            &ui.snapshot,
            states,
            now,
            needle,
            searching,
            ui_font,
            hc,
        ),
        None => render_placeholder(
            rust_i18n::t!("host_container_loading").to_string(),
            ui_font,
            hc,
        ),
    });

    section.finish()
}

/// 容器操作失败提示：红色小字，独立于采集状态，不替换容器网格。
fn render_action_error(err: &str, ui_font: fonts::FamilyId, hc: &HostUiColors) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(
            format!("{}：{}", rust_i18n::t!("container_action_error"), err),
            ui_font,
            UI_FONT_SIZE_SMALL,
        )
        .with_color(hc.semantic.danger)
        .finish(),
    )
    .with_horizontal_padding(4.0)
    .with_margin_bottom(8.0)
    .finish()
}

/// 按快照状态分派：连接错误 / docker 错误 / 加载中 / 空 / 容器网格。
/// searching 为 true 时（该主机已确认有匹配容器）直接过滤渲染，跳过 loading/error/empty 分支。
fn render_host_body(
    host_id: &str,
    snapshot: &nexshell::container_overview::ContainerSnapshot,
    states: &ContainerCardStates,
    now: Instant,
    needle: &str,
    searching: bool,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    if searching {
        let matched: Vec<&ContainerInfo> = snapshot
            .containers
            .iter()
            .filter(|c| c.name.to_lowercase().contains(needle))
            .collect();
        return render_container_grid(host_id, &matched, states, now, ui_font, hc);
    }
    if let ContainerCollectStatus::Error(text) = &snapshot.status {
        let msg = format!(
            "{}：{}",
            rust_i18n::t!("host_container_connect_error"),
            text
        );
        return render_placeholder(msg, ui_font, hc);
    }
    if let Some(error) = &snapshot.error {
        let msg = match error {
            ContainerProbeError::NoDocker => rust_i18n::t!("host_container_no_docker").to_string(),
            ContainerProbeError::PermissionDenied => {
                rust_i18n::t!("host_container_permission_denied").to_string()
            }
            ContainerProbeError::Other(text) => {
                format!("{}：{}", rust_i18n::t!("host_container_probe_error"), text)
            }
        };
        return render_placeholder(msg, ui_font, hc);
    }
    if snapshot.containers.is_empty() {
        let text = if snapshot.status == ContainerCollectStatus::Ready {
            rust_i18n::t!("host_container_empty").to_string()
        } else {
            rust_i18n::t!("host_container_loading").to_string()
        };
        return render_placeholder(text, ui_font, hc);
    }
    let all: Vec<&ContainerInfo> = snapshot.containers.iter().collect();
    render_container_grid(host_id, &all, states, now, ui_font, hc)
}

fn render_placeholder(
    text: String,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(text, ui_font, UI_FONT_SIZE)
            .with_color(hc.text_secondary)
            .finish(),
    )
    .with_horizontal_padding(4.0)
    .with_vertical_padding(12.0)
    .finish()
}

/// 双列容器卡片网格，仿 host 卡片网格换行逻辑。containers 为引用切片，搜索态传过滤子集时免 clone。
fn render_container_grid(
    host_id: &str,
    containers: &[&ContainerInfo],
    states: &ContainerCardStates,
    now: Instant,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut col = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

    for row_start in (0..containers.len()).step_by(CONTAINER_GRID_COLUMNS) {
        if row_start > 0 {
            col.add_child(spacer_v(CARD_SPACING));
        }
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);

        let row_end = (row_start + CONTAINER_GRID_COLUMNS).min(containers.len());
        for index in row_start..row_end {
            if index > row_start {
                row.add_child(spacer_h(CARD_SPACING));
            }
            row.add_child(
                Expanded::new(
                    1.0,
                    render_container_card(host_id, containers[index], states, now, ui_font, hc),
                )
                .finish(),
            );
        }
        if row_end - row_start < CONTAINER_GRID_COLUMNS {
            row.add_child(spacer_h(CARD_SPACING));
            row.add_child(Expanded::new(1.0, Empty::new().finish()).finish());
        }

        col.add_child(row.finish());
    }

    col.finish()
}

fn render_container_card(
    host_id: &str,
    container: &ContainerInfo,
    states: &ContainerCardStates,
    now: Instant,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let name_color = if container.state == ContainerState::Running {
        hc.text_primary
    } else {
        hc.text_secondary
    };

    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new_inline(container.name.clone(), ui_font, UI_FONT_SIZE)
                .with_color(name_color)
                .finish(),
        )
        .with_child(Expanded::new(1.0, Empty::new().finish()).finish())
        .with_child(
            Container::new(
                Text::new_inline(container.status_text.clone(), ui_font, UI_FONT_SIZE_SMALL)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_right(8.0)
            .finish(),
        )
        .with_child(render_container_menu_button(
            host_id, container, states, ui_font, hc,
        ))
        .finish();

    let body = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Expanded::new(
                1.0,
                render_cpu_segment(host_id, container, states, now, ui_font, hc),
            )
            .finish(),
        )
        .with_child(Expanded::new(1.0, render_mem_segment(container, ui_font, hc)).finish())
        .with_child(Expanded::new(1.0, render_rw_segment(container, ui_font, hc)).finish())
        .with_child(Expanded::new(1.0, render_net_segment(container, ui_font, hc)).finish())
        .finish();

    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(spacer_v(12.0))
        .with_child(body)
        .finish();

    let card = Container::new(content)
        .with_horizontal_padding(CARD_PADDING)
        .with_vertical_padding(CARD_PADDING)
        .with_background_color(hc.card_bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(CARD_CORNER_RADIUS)))
        .with_border(Border::all(1.0).with_border_color(hc.card_border))
        .finish();

    // 整卡右键：与 "⋯" 按钮派发同一 action；hover 态仅占位，不改卡片视觉。
    let card_hover_state = states.card_hover_state(format!("{host_id}:card:{}", container.id));
    let host_id = host_id.to_string();
    let container_id = container.id.clone();
    let container_name = container.name.clone();
    let container_state = container.state;

    Hoverable::new(card_hover_state, move |_| card)
        .with_defer_events_to_children()
        .on_right_click(move |ctx, _, position| {
            ctx.dispatch_typed_action(TerminalGridAction::ContainerShowMenu {
                host_id: host_id.clone(),
                container_id: container_id.clone(),
                container_name: container_name.clone(),
                state: container_state,
                position,
            });
        })
        .finish()
}

/// header 右侧 "⋯" 菜单按钮：点击派发 ContainerShowMenu，菜单内容按容器状态在 RootView 侧构建。
fn render_container_menu_button(
    host_id: &str,
    container: &ContainerInfo,
    states: &ContainerCardStates,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let state = states.menu_button_state(format!("{host_id}:{}", container.id));
    let host_id = host_id.to_string();
    let container_id = container.id.clone();
    let container_name = container.name.clone();
    let container_state = container.state;
    let hc = *hc;

    Hoverable::new(state, move |mouse| {
        let bg = if mouse.is_hovered() {
            hc.card_bg_hover
        } else {
            ColorU::transparent_black()
        };
        Container::new(
            Text::new_inline("⋯".to_string(), ui_font, UI_FONT_SIZE)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_horizontal_padding(6.0)
        .with_vertical_padding(2.0)
        .with_background_color(bg)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.0)))
        .finish()
    })
    .with_cursor(warpui::platform::Cursor::PointingHand)
    .on_click(move |ctx, _, position| {
        ctx.dispatch_typed_action(TerminalGridAction::ContainerShowMenu {
            host_id: host_id.clone(),
            container_id: container_id.clone(),
            container_name: container_name.clone(),
            state: container_state,
            position,
        });
    })
    .finish()
}

/// CPU 环 + 健康点：无 stats（exited）只画轨道、中心 0%、点位灰。
fn render_cpu_segment(
    host_id: &str,
    container: &ContainerInfo,
    states: &ContainerCardStates,
    now: Instant,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    const DIAMETER: f32 = 44.0;
    const THICKNESS: f32 = 5.0;
    const DOT_SIZE: f32 = 6.0;

    let track = ColorU::new(
        hc.metric_track.r,
        hc.metric_track.g,
        hc.metric_track.b,
        0x4a,
    );
    // sweep 走 FloatTransition：中心文字与弧都用动画值，无 stats 不建动画项、只画轨道。
    let target = container
        .stats
        .map(|s| (s.cpu_percent / 100.0).clamp(0.0, 1.0));
    let display = sample_gauge(states, format!("{host_id}:{}", container.id), target, now);
    let display_percent = display.unwrap_or(0.0) * 100.0;
    let label_text = format!("{display_percent:.0}%");
    // 弧色按用量分档：<60 绿 / <85 橙 / 其余红（与状态卡 level_color 同配方）。
    let value_color = if display_percent >= 85.0 {
        hc.semantic.danger
    } else if display_percent >= 60.0 {
        hc.semantic.warn
    } else {
        hc.semantic.ok
    };
    let mut gauge = nexshell::stat_widgets::RingGauge::new(DIAMETER, THICKNESS, track, value_color)
        .with_label(bold_text(label_text, ui_font, 11.0, hc.text_primary));
    if let Some(fraction) = display {
        gauge = gauge.with_fraction(fraction);
    }

    let dot_color = health_dot_color(container, hc);
    let dot = Container::new(Empty::new().finish())
        .with_background_color(dot_color)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(DOT_SIZE / 2.0)))
        .finish();

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(gauge.finish())
        .with_child(spacer_v(6.0))
        .with_child(
            ConstrainedBox::new(dot)
                .with_width(DOT_SIZE)
                .with_height(DOT_SIZE)
                .finish(),
        )
        .finish()
}

/// retarget+sample：目标不变不重启，中途变更从当前采样值续走（同 status_view 配方）。
fn sample_gauge(
    states: &ContainerCardStates,
    key: String,
    target: Option<f32>,
    now: Instant,
) -> Option<f32> {
    let target = target?;
    let mut map = states.gauge_anim.borrow_mut();
    map.retarget(key.clone(), target, now);
    Some(map.sample(&key, now).unwrap_or(target))
}

/// 健康点档位色：非 Running 灰；Running 时按健康态 ok/warn/danger，无健康数据视为 ok。
fn health_dot_color(container: &ContainerInfo, hc: &HostUiColors) -> ColorU {
    if container.state != ContainerState::Running {
        return hc.text_secondary;
    }
    match container.health {
        Some(ContainerHealth::Unhealthy) => hc.semantic.danger,
        Some(ContainerHealth::Starting) => hc.semantic.warn,
        Some(ContainerHealth::Healthy) | None => hc.semantic.ok,
    }
}

/// MEM 段：大数字 + 小单位，下方 "MEM" 标签；无 stats 显示 "0 M"。
fn render_mem_segment(
    container: &ContainerInfo,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (num, unit) = match container.stats {
        Some(stats) => split_rate(stats.mem_usage_bytes),
        None => ("0".to_string(), "M".to_string()),
    };

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(value_with_unit(num, unit, ui_font, hc))
        .with_child(spacer_v(4.0))
        .with_child(
            Text::new_inline("MEM".to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .finish()
}

/// R/W 段：两行 "R 数值" / "W 数值"，行内标签在前。
fn render_rw_segment(
    container: &ContainerInfo,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (read, write) = container
        .stats
        .map(|s| (s.block_read_bytes, s.block_write_bytes))
        .unwrap_or((0, 0));

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(inline_stat_row("R", read, ui_font, hc))
        .with_child(spacer_v(6.0))
        .with_child(inline_stat_row("W", write, ui_font, hc))
        .finish()
}

/// ↑/↓ 段：docker NetIO 为 rx/tx，↑=tx 上传、↓=rx 接收。
fn render_net_segment(
    container: &ContainerInfo,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (rx, tx) = container
        .stats
        .map(|s| (s.net_rx_bytes, s.net_tx_bytes))
        .unwrap_or((0, 0));

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(inline_stat_row("↑", tx, ui_font, hc))
        .with_child(spacer_v(6.0))
        .with_child(inline_stat_row("↓", rx, ui_font, hc))
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_page_fallback_keeps_loading_until_all_hosts_are_empty() {
        assert_eq!(
            container_page_fallback(false, false, true, false),
            ContainerPageFallback::Loading
        );
        assert_eq!(
            container_page_fallback(false, false, false, true),
            ContainerPageFallback::Empty
        );
    }

    #[test]
    fn loading_dot_alpha_staggers_three_dots() {
        let first_dot_peak = loading_dot_alpha(0.0, 0);
        let second_dot_at_first_peak = loading_dot_alpha(0.0, 1);
        let second_dot_peak = loading_dot_alpha(1.0 / 3.0, 1);

        assert!(first_dot_peak > second_dot_at_first_peak);
        assert!(second_dot_peak > second_dot_at_first_peak);
    }

    #[test]
    fn missing_host_state_is_page_level_loading_not_host_section() {
        assert!(host_container_is_loading(None));
        assert!(host_section_hidden(None));
    }
}

/// 行内 "标签 数字单位"：标签灰小字，值 bold + 单位小灰。
fn inline_stat_row(
    label: &str,
    bytes: u64,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let (num, unit) = split_rate(bytes);
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(
            Text::new_inline(label.to_string(), ui_font, UI_FONT_SIZE_SMALL)
                .with_color(hc.text_secondary)
                .finish(),
        )
        .with_child(spacer_h(4.0))
        .with_child(value_with_unit(num, unit, ui_font, hc))
        .finish()
}

/// 大数字 bold + 小单位灰，一行。
fn value_with_unit(
    num: String,
    unit: String,
    ui_font: fonts::FamilyId,
    hc: &HostUiColors,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::End)
        .with_child(bold_text(num, ui_font, 15.0, hc.text_primary));
    if !unit.is_empty() {
        row.add_child(spacer_h(2.0));
        row.add_child(
            Container::new(
                Text::new_inline(unit, ui_font, 10.0)
                    .with_color(hc.text_secondary)
                    .finish(),
            )
            .with_margin_bottom(1.0)
            .finish(),
        );
    }
    row.finish()
}

fn bold_text(
    content: String,
    ui_font: fonts::FamilyId,
    size: f32,
    color: ColorU,
) -> Box<dyn Element> {
    Text::new_inline(content, ui_font, size)
        .with_color(color)
        .with_style(fonts::Properties::default().weight(fonts::Weight::Bold))
        .finish()
}

fn spacer_h(width: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_width(width)
        .finish()
}

fn spacer_v(height: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Empty::new().finish())
        .with_height(height)
        .finish()
}
