//! Pane tree for split-pane terminal layout.
//! Adapted from warp/app/src/pane_group/tree.rs.

use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;
use std::collections::HashSet;
use std::fmt;
use warpui::elements::{
    ChildAnchor, ConstrainedBox, DispatchEventResult, Empty, EventHandler, Flex, Hoverable,
    MouseStateHandle, OffsetPositioning, ParentElement, PositionedElementAnchor,
    PositionedElementOffsetBounds, Rect, SavePosition, Shrinkable, Stack,
};
use warpui::{color::ColorU, elements::Element, EntityId, EventContext, View, ViewContext};

use crate::pane_state::NexPaneId;

const DEFAULT_FLEX_VALUE: f32 = 1.0;
const DEFAULT_FLEX_SIZE: PaneFlex = PaneFlex(DEFAULT_FLEX_VALUE);
const DIVIDER_THICKNESS: f32 = 2.0;
const MINIMUM_PANE_SIZE: f32 = 50.0;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn axis(&self) -> SplitDirection {
        match self {
            Direction::Left | Direction::Right => SplitDirection::Horizontal,
            Direction::Up | Direction::Down => SplitDirection::Vertical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DraggedBorder {
    pub border_id: EntityId,
    pub direction: SplitDirection,
    pub previous_mouse_location: Vector2F,
}

#[derive(Debug)]
pub struct PaneFlex(pub f32);

impl Default for PaneFlex {
    fn default() -> Self {
        PaneFlex(DEFAULT_FLEX_VALUE)
    }
}

pub struct PaneData {
    pub root: PaneNode,
    len: usize,
}

pub enum PaneNode {
    Branch(PaneBranch),
    Leaf(NexPaneId),
}

pub struct PaneBranch {
    axis: SplitDirection,
    pub nodes: Vec<(PaneFlex, PaneNode)>,
    dividers: Vec<Divider>,
}

struct Divider {
    id: EntityId,
    mouse_state: MouseStateHandle,
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Divider {
    fn new() -> Self {
        Self {
            id: EntityId::new(),
            mouse_state: Default::default(),
        }
    }
}

enum BranchRemoveResult {
    NotFound,
    Removed,
    Collapse(PaneNode),
}

#[derive(Debug, PartialEq)]
enum FindPaneByDirectionResult {
    Located,
    NotFound,
    Found(HashSet<NexPaneId>),
}

trait FindPaneByDirection {
    fn panes_by_direction(
        &self,
        content: NexPaneId,
        direction: Direction,
    ) -> FindPaneByDirectionResult;
}

// ---------------------------------------------------------------------------
// PaneData
// ---------------------------------------------------------------------------

impl PaneData {
    pub fn new(pane_id: NexPaneId) -> Self {
        Self {
            root: PaneNode::Leaf(pane_id),
            len: 1,
        }
    }

    pub fn split(&mut self, old_id: NexPaneId, new_id: NexPaneId, direction: Direction) -> bool {
        let ok = self.root.split(old_id, new_id, direction);
        if ok {
            self.len += 1;
        }
        ok
    }

    pub fn remove(&mut self, content: NexPaneId) -> bool {
        let ok = self.root.remove(content);
        if ok {
            self.len = self.len.saturating_sub(1);
        }
        ok
    }

    pub fn pane_ids(&self) -> Vec<NexPaneId> {
        self.root.pane_ids()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn render<F, A, R>(
        &self,
        render_pane: &F,
        divider_color: ColorU,
        focus_action: A,
        resize_action: R,
    ) -> Box<dyn Element>
    where
        F: Fn(NexPaneId) -> Box<dyn Element>,
        A: Fn(NexPaneId, &mut EventContext<'_>) + Clone + 'static,
        R: Fn(DraggedBorder, &mut EventContext<'_>) + Clone + 'static,
    {
        match &self.root {
            PaneNode::Leaf(id) => {
                let pane_id = *id;
                let action = focus_action.clone();
                EventHandler::new(render_pane(pane_id))
                    .on_left_mouse_down(move |ctx, _, _| {
                        action(pane_id, ctx);
                        DispatchEventResult::PropagateToParent
                    })
                    .finish()
            }
            PaneNode::Branch(branch) => {
                branch.render(render_pane, divider_color, &focus_action, &resize_action)
            }
        }
    }

    pub fn adjust_pane_size<V: View>(
        &mut self,
        border_id: EntityId,
        delta: f32,
        ctx: &ViewContext<V>,
    ) {
        self.root.adjust_pane_size(border_id, delta, ctx);
    }

    pub fn adjust_pane_size_by_id<V: View>(
        &mut self,
        pane_id: NexPaneId,
        direction: SplitDirection,
        delta: f32,
        ctx: &ViewContext<V>,
    ) {
        self.root
            .adjust_pane_size_by_id(pane_id, direction, delta, ctx);
    }

    pub fn panes_by_direction<V: View>(
        &self,
        pane_id: NexPaneId,
        direction: Direction,
        ctx: &ViewContext<V>,
    ) -> Vec<NexPaneId> {
        if let FindPaneByDirectionResult::Found(ids) =
            self.root.panes_by_direction(pane_id, direction)
        {
            if let Some(current_rect) = ctx.element_position_by_id(pane_id.position_id()) {
                ids.into_iter()
                    .filter(|id| match ctx.element_position_by_id(id.position_id()) {
                        Some(candidate_rect) => {
                            are_rects_overlapping(&current_rect, &candidate_rect, direction.axis())
                        }
                        None => true,
                    })
                    .collect()
            } else {
                Vec::from_iter(ids)
            }
        } else {
            Vec::new()
        }
    }
}

fn are_rects_overlapping(rect1: &RectF, rect2: &RectF, axis: SplitDirection) -> bool {
    match axis {
        SplitDirection::Horizontal => {
            !(rect1.max_y() <= rect2.min_y() || rect1.min_y() >= rect2.max_y())
        }
        SplitDirection::Vertical => {
            !(rect1.max_x() <= rect2.min_x() || rect1.min_x() >= rect2.max_x())
        }
    }
}

// ---------------------------------------------------------------------------
// PaneNode
// ---------------------------------------------------------------------------

impl PaneNode {
    fn split(&mut self, old_id: NexPaneId, new_id: NexPaneId, direction: Direction) -> bool {
        match self {
            PaneNode::Leaf(id) => {
                if *id == old_id {
                    *self = PaneNode::Branch(PaneBranch::for_leaves(old_id, new_id, direction));
                    true
                } else {
                    false
                }
            }
            PaneNode::Branch(branch) => branch.split(old_id, new_id, direction),
        }
    }

    fn remove(&mut self, pane_id: NexPaneId) -> bool {
        match self {
            PaneNode::Leaf(_) => false,
            PaneNode::Branch(branch) => match branch.remove(pane_id) {
                BranchRemoveResult::NotFound => false,
                BranchRemoveResult::Removed => true,
                BranchRemoveResult::Collapse(last_node) => {
                    *self = last_node;
                    true
                }
            },
        }
    }

    fn pane_ids(&self) -> Vec<NexPaneId> {
        match self {
            PaneNode::Leaf(id) => vec![*id],
            PaneNode::Branch(branch) => branch.get_children(),
        }
    }

    fn render<F, A, R>(
        &self,
        render_pane: &F,
        divider_color: ColorU,
        focus_action: &A,
        resize_action: &R,
    ) -> Box<dyn Element>
    where
        F: Fn(NexPaneId) -> Box<dyn Element>,
        A: Fn(NexPaneId, &mut EventContext<'_>) + Clone + 'static,
        R: Fn(DraggedBorder, &mut EventContext<'_>) + Clone + 'static,
    {
        match self {
            PaneNode::Leaf(id) => {
                let pane_id = *id;
                let action = focus_action.clone();
                EventHandler::new(render_pane(pane_id))
                    .on_left_mouse_down(move |ctx, _, _| {
                        action(pane_id, ctx);
                        DispatchEventResult::PropagateToParent
                    })
                    .finish()
            }
            PaneNode::Branch(branch) => {
                branch.render(render_pane, divider_color, focus_action, resize_action)
            }
        }
    }

    fn pane_size<V: View>(&self, ctx: &ViewContext<V>) -> Vector2F {
        match self {
            PaneNode::Leaf(id) => ctx
                .element_position_by_id(id.position_id())
                .map_or(Vector2F::zero(), |rect| rect.size()),
            PaneNode::Branch(branch) => branch.size(ctx),
        }
    }

    fn adjust_pane_size<V: View>(
        &mut self,
        border_id: EntityId,
        delta: f32,
        ctx: &ViewContext<V>,
    ) -> bool {
        match self {
            PaneNode::Leaf(_) => false,
            PaneNode::Branch(branch) => branch.adjust_pane_size(border_id, delta, ctx),
        }
    }

    fn adjust_pane_size_by_id<V: View>(
        &mut self,
        pane_id: NexPaneId,
        direction: SplitDirection,
        delta: f32,
        ctx: &ViewContext<V>,
    ) -> bool {
        match self {
            PaneNode::Leaf(id) => *id == pane_id,
            PaneNode::Branch(branch) => {
                branch.adjust_pane_size_by_id(pane_id, direction, delta, ctx)
            }
        }
    }

    fn first_panes_in_direction(&self, direction: Direction) -> HashSet<NexPaneId> {
        match self {
            PaneNode::Leaf(id) => HashSet::from([*id]),
            PaneNode::Branch(branch) => {
                if branch.axis() == direction.axis() {
                    match direction {
                        Direction::Left | Direction::Up => branch
                            .nodes
                            .last()
                            .expect("PaneBranch has no nodes")
                            .1
                            .first_panes_in_direction(direction),
                        Direction::Right | Direction::Down => branch
                            .nodes
                            .first()
                            .expect("PaneBranch has no nodes")
                            .1
                            .first_panes_in_direction(direction),
                    }
                } else {
                    branch
                        .nodes
                        .iter()
                        .flat_map(|(_, node)| node.first_panes_in_direction(direction))
                        .collect()
                }
            }
        }
    }
}

impl FindPaneByDirection for PaneNode {
    fn panes_by_direction(
        &self,
        pane_id: NexPaneId,
        direction: Direction,
    ) -> FindPaneByDirectionResult {
        match self {
            PaneNode::Leaf(id) => {
                if *id == pane_id {
                    FindPaneByDirectionResult::Located
                } else {
                    FindPaneByDirectionResult::NotFound
                }
            }
            PaneNode::Branch(branch) => branch.panes_by_direction(pane_id, direction),
        }
    }
}

// ---------------------------------------------------------------------------
// PaneBranch
// ---------------------------------------------------------------------------

impl PaneBranch {
    fn new(old_pane: PaneNode, new_pane: PaneNode, direction: Direction) -> Self {
        let axis = direction.axis();
        PaneBranch {
            axis,
            nodes: match direction {
                Direction::Left | Direction::Up => {
                    vec![(DEFAULT_FLEX_SIZE, new_pane), (DEFAULT_FLEX_SIZE, old_pane)]
                }
                Direction::Right | Direction::Down => {
                    vec![(DEFAULT_FLEX_SIZE, old_pane), (DEFAULT_FLEX_SIZE, new_pane)]
                }
            },
            dividers: vec![Divider::new()],
        }
    }

    fn for_leaves(old_leaf: NexPaneId, new_leaf: NexPaneId, direction: Direction) -> Self {
        Self::new(
            PaneNode::Leaf(old_leaf),
            PaneNode::Leaf(new_leaf),
            direction,
        )
    }

    fn split(&mut self, old_pane: NexPaneId, new_pane: NexPaneId, direction: Direction) -> bool {
        for (idx, (_, node)) in self.nodes.iter_mut().enumerate() {
            match node {
                PaneNode::Branch(branch) => {
                    if branch.split(old_pane, new_pane, direction) {
                        return true;
                    }
                }
                PaneNode::Leaf(id) => {
                    if *id == old_pane {
                        if direction.axis() == self.axis {
                            self.nodes.insert(
                                match direction {
                                    Direction::Left | Direction::Up => idx,
                                    Direction::Right | Direction::Down => idx + 1,
                                },
                                (DEFAULT_FLEX_SIZE, PaneNode::Leaf(new_pane)),
                            );
                            self.dividers.insert(idx, Divider::new());
                        } else {
                            *node =
                                PaneNode::Branch(PaneBranch::for_leaves(*id, new_pane, direction));
                        }
                        return true;
                    }
                }
            }
        }
        false
    }

    fn remove(&mut self, pane_id: NexPaneId) -> BranchRemoveResult {
        for (idx, (_, node)) in self.nodes.iter_mut().enumerate() {
            match node {
                PaneNode::Branch(_) => {
                    if node.remove(pane_id) {
                        return BranchRemoveResult::Removed;
                    }
                }
                PaneNode::Leaf(id) => {
                    if *id == pane_id {
                        self.nodes.remove(idx);
                        if !self.dividers.is_empty() {
                            self.dividers.remove(idx.min(self.dividers.len() - 1));
                        }
                        if self.nodes.len() == 1 {
                            return BranchRemoveResult::Collapse(self.nodes.pop().unwrap().1);
                        } else {
                            return BranchRemoveResult::Removed;
                        }
                    }
                }
            }
        }
        BranchRemoveResult::NotFound
    }

    fn get_children(&self) -> Vec<NexPaneId> {
        let mut res = vec![];
        for (_, member) in &self.nodes {
            match member {
                PaneNode::Branch(branch) => res.extend(branch.get_children()),
                PaneNode::Leaf(id) => res.push(*id),
            }
        }
        res
    }

    fn render<F, A, R>(
        &self,
        render_pane: &F,
        divider_color: ColorU,
        focus_action: &A,
        resize_action: &R,
    ) -> Box<dyn Element>
    where
        F: Fn(NexPaneId) -> Box<dyn Element>,
        A: Fn(NexPaneId, &mut EventContext<'_>) + Clone + 'static,
        R: Fn(DraggedBorder, &mut EventContext<'_>) + Clone + 'static,
    {
        let mut parent = match self.axis {
            SplitDirection::Horizontal => Flex::row(),
            SplitDirection::Vertical => Flex::column(),
        };

        let mut dividers = self.dividers.iter();
        let mut divider_positions = Vec::new();

        for (flex, node) in self.nodes.iter() {
            parent.add_child(
                Shrinkable::new(
                    flex.0,
                    node.render(render_pane, divider_color, focus_action, resize_action),
                )
                .finish(),
            );
            if let Some(divider) = dividers.next() {
                let position_id = format!("divider_placeholder_{}", divider.id);
                divider_positions.push((divider, position_id.clone()));
                parent.add_child(create_divider_placeholder(self.axis, &position_id));
            }
        }

        let mut stack = Stack::new().with_constrain_absolute_children();
        stack.add_child(parent.finish());

        for (divider, position_id) in divider_positions {
            let divider_element = create_divider(self.axis, divider, divider_color, resize_action);
            stack.add_positioned_child(
                divider_element,
                OffsetPositioning::offset_from_save_position_element(
                    position_id,
                    Vector2F::new(0., 0.),
                    PositionedElementOffsetBounds::Unbounded,
                    PositionedElementAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }
        stack.finish()
    }

    fn adjust_pane_size<V: View>(
        &mut self,
        border_id: EntityId,
        delta: f32,
        ctx: &ViewContext<V>,
    ) -> bool {
        if let Some(idx) = self
            .dividers
            .iter()
            .position(|divider| divider.id == border_id)
        {
            let pane_size_1 = self.nodes[idx].1.pane_size(ctx);
            let pane_size_2 = self.nodes[idx + 1].1.pane_size(ctx);

            let flex_1 = self.nodes[idx].0 .0;
            let flex_2 = self.nodes[idx + 1].0 .0;
            let total_flex = flex_1 + flex_2;

            let (size_1, size_2) = match self.axis {
                SplitDirection::Horizontal => (pane_size_1.x(), pane_size_2.x()),
                SplitDirection::Vertical => (pane_size_1.y(), pane_size_2.y()),
            };

            if size_1 + delta < MINIMUM_PANE_SIZE
                || size_2 - delta < MINIMUM_PANE_SIZE
                || delta.abs() < f32::EPSILON
            {
                return true;
            }

            let new_flex = ((size_1 + delta) / (size_1 + size_2) * total_flex)
                .max(0.)
                .min(total_flex);

            self.nodes[idx].0 = PaneFlex(new_flex);
            self.nodes[idx + 1].0 = PaneFlex(total_flex - new_flex);
            return true;
        }

        for (_, node) in &mut self.nodes {
            if node.adjust_pane_size(border_id, delta, ctx) {
                return true;
            }
        }
        false
    }

    fn size<V: View>(&self, ctx: &ViewContext<V>) -> Vector2F {
        match self.axis {
            SplitDirection::Horizontal => Vector2F::new(
                self.nodes
                    .iter()
                    .fold(0., |x, (_, node)| x + node.pane_size(ctx).x()),
                self.nodes[0].1.pane_size(ctx).y(),
            ),
            SplitDirection::Vertical => Vector2F::new(
                self.nodes[0].1.pane_size(ctx).x(),
                self.nodes
                    .iter()
                    .fold(0., |y, (_, node)| y + node.pane_size(ctx).y()),
            ),
        }
    }

    fn adjust_pane_size_by_id<V: View>(
        &mut self,
        pane_id: NexPaneId,
        direction: SplitDirection,
        delta: f32,
        ctx: &ViewContext<V>,
    ) -> bool {
        for (idx, (_, node)) in self.nodes.iter_mut().enumerate() {
            if node.adjust_pane_size_by_id(pane_id, direction, delta, ctx) {
                if direction != self.axis {
                    return true;
                }
                let divider_id = self.dividers[idx.min(self.dividers.len() - 1)].id;
                self.adjust_pane_size(divider_id, delta, ctx);
                break;
            }
        }
        false
    }

    fn axis(&self) -> SplitDirection {
        self.axis
    }
}

impl FindPaneByDirection for PaneBranch {
    fn panes_by_direction(
        &self,
        pane_id: NexPaneId,
        direction: Direction,
    ) -> FindPaneByDirectionResult {
        for (idx, (_, node)) in self.nodes.iter().enumerate() {
            let res = node.panes_by_direction(pane_id, direction);
            match res {
                FindPaneByDirectionResult::Found(_) => return res,
                FindPaneByDirectionResult::Located => {
                    if direction.axis() != self.axis {
                        return res;
                    }
                    let target_panes = match direction {
                        Direction::Left | Direction::Up => {
                            if idx == 0 {
                                return res;
                            }
                            self.nodes[idx - 1].1.first_panes_in_direction(direction)
                        }
                        Direction::Right | Direction::Down => {
                            if idx == self.nodes.len() - 1 {
                                return res;
                            }
                            self.nodes[idx + 1].1.first_panes_in_direction(direction)
                        }
                    };
                    return FindPaneByDirectionResult::Found(target_panes);
                }
                FindPaneByDirectionResult::NotFound => (),
            }
        }
        FindPaneByDirectionResult::NotFound
    }
}

// ---------------------------------------------------------------------------
// Divider rendering (adapted from warp/app/src/pane_group/tree.rs)
// ---------------------------------------------------------------------------

fn create_divider_placeholder(direction: SplitDirection, position_id: &str) -> Box<dyn Element> {
    let thickness = DIVIDER_THICKNESS - 1.0;
    let placeholder = match direction {
        SplitDirection::Horizontal => ConstrainedBox::new(Empty::new().finish())
            .with_width(thickness)
            .finish(),
        SplitDirection::Vertical => ConstrainedBox::new(Empty::new().finish())
            .with_height(thickness)
            .finish(),
    };
    SavePosition::new(placeholder, position_id).finish()
}

fn create_divider<R>(
    direction: SplitDirection,
    item: &Divider,
    color: ColorU,
    resize_action: &R,
) -> Box<dyn Element>
where
    R: Fn(DraggedBorder, &mut EventContext<'_>) + Clone + 'static,
{
    let divider = ConstrainedBox::new(Rect::new().with_background_color(color).finish());

    let cursor_shape = match direction {
        SplitDirection::Horizontal => warpui::platform::Cursor::ResizeLeftRight,
        SplitDirection::Vertical => warpui::platform::Cursor::ResizeUpDown,
    };

    let border_id = item.id;
    let on_resize = resize_action.clone();

    Hoverable::new(item.mouse_state.clone(), move |_| {
        let on_resize = on_resize.clone();
        EventHandler::new(match direction {
            SplitDirection::Horizontal => divider.with_width(DIVIDER_THICKNESS).finish(),
            SplitDirection::Vertical => divider.with_height(DIVIDER_THICKNESS).finish(),
        })
        .on_left_mouse_down(move |ctx, _, position| {
            on_resize(
                DraggedBorder {
                    border_id,
                    direction,
                    previous_mouse_location: position,
                },
                ctx,
            );
            DispatchEventResult::StopPropagation
        })
        .finish()
    })
    .with_cursor(cursor_shape)
    .with_propagate_drag()
    .finish()
}

// ---------------------------------------------------------------------------
// Debug impls
// ---------------------------------------------------------------------------

impl fmt::Debug for PaneData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PaneData({:?})", self.root)
    }
}

impl fmt::Debug for PaneNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaneNode::Leaf(id) => write!(f, "Leaf({id:?})"),
            PaneNode::Branch(branch) => write!(f, "Branch {branch:?}"),
        }
    }
}

impl fmt::Debug for PaneBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.axis {
            SplitDirection::Horizontal => write!(f, "Horizontal({:?})", self.nodes),
            SplitDirection::Vertical => write!(f, "Vertical({:?})", self.nodes),
        }
    }
}
