//! 一个窗口里的文档窗格。
//!
//! 叶子窗格各显示一份已打开的文档，拆分可以左右或上下嵌套。同一份文档不同时出现在两个窗格里。

use eframe::egui::{Pos2, Rect};
use weave_workspace::TabId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    LeftRight,
    TopBottom,
}

#[derive(Clone, Debug)]
enum Node {
    Leaf {
        id: u64,
        tab: Option<TabId>,
    },
    Split {
        id: u64,
        axis: SplitAxis,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug)]
pub struct PlacedPane {
    pub id: u64,
    pub tab: Option<TabId>,
    pub rect: Rect,
}

#[derive(Clone, Debug)]
pub struct SplitDivider {
    pub id: u64,
    pub axis: SplitAxis,
    pub rect: Rect,
    pub bounds: Rect,
}

#[derive(Clone, Debug)]
pub struct PaneLayout {
    pub panes: Vec<PlacedPane>,
    pub dividers: Vec<SplitDivider>,
}

#[derive(Clone, Debug)]
pub struct Panes {
    root: Node,
    focused: u64,
    next_id: u64,
}

const GAP: f32 = 6.0;
const MIN_PANE: f32 = 96.0;

impl Default for Panes {
    fn default() -> Self {
        Self {
            root: Node::Leaf { id: 1, tab: None },
            focused: 1,
            next_id: 2,
        }
    }
}

impl Panes {
    pub fn is_split(&self) -> bool {
        self.leaf_count() > 1
    }

    pub fn leaf_count(&self) -> usize {
        leaf_count(&self.root)
    }

    pub fn focused_id(&self) -> u64 {
        self.focused
    }

    pub fn focused_tab(&self) -> Option<TabId> {
        self.tab_of(self.focused)
    }

    pub fn tab_of(&self, id: u64) -> Option<TabId> {
        find_leaf(&self.root, id).and_then(|(_, tab)| tab)
    }

    pub fn is_shown(&self, tab: TabId) -> bool {
        self.leaf_with(tab).is_some()
    }

    pub fn leaf_with(&self, tab: TabId) -> Option<u64> {
        leaf_with(&self.root, tab)
    }

    pub fn focus(&mut self, id: u64) -> bool {
        if find_leaf(&self.root, id).is_none() {
            return false;
        }
        self.focused = id;
        true
    }

    /// 把文档放进当前窗格。它已经显示在别的窗格里时，只是把键盘焦点移过去。
    pub fn present(&mut self, tab: Option<TabId>) {
        let Some(tab) = tab else {
            if let Node::Leaf { tab: slot, .. } = &mut self.root {
                *slot = None;
            }
            return;
        };
        if let Some(id) = self.leaf_with(tab) {
            self.focused = id;
            return;
        }
        self.set_tab(self.focused, Some(tab));
    }

    pub fn forget(&mut self, tab: TabId) {
        clear_tab(&mut self.root, tab);
    }

    pub fn set_tab(&mut self, id: u64, tab: Option<TabId>) -> bool {
        if let Some(tab) = tab
            && let Some(other) = self.leaf_with(tab)
            && other != id
        {
            clear_tab(&mut self.root, tab);
        }
        set_leaf_tab(&mut self.root, id, tab)
    }

    /// 拆分当前窗格。新窗格在右或下，原来的文档留在原窗格。返回新窗格的 id。
    pub fn split_focused(&mut self, axis: SplitAxis, companion: Option<TabId>) -> Option<u64> {
        if let Some(tab) = companion
            && let Some(other) = self.leaf_with(tab)
            && other != self.focused
        {
            self.set_tab(other, None);
        }
        let mut next = self.next_id;
        let created = split_leaf(&mut self.root, self.focused, axis, companion, &mut next);
        self.next_id = next;
        created
    }

    pub fn close(&mut self, id: u64) -> bool {
        if matches!(self.root, Node::Leaf { .. }) {
            return false;
        }
        let Some(root) = cut(self.root.clone(), id) else {
            return false;
        };
        let focused = self.focused;
        self.root = root;
        if find_leaf(&self.root, focused).is_none() {
            self.focused = first_leaf(&self.root);
        }
        true
    }

    pub fn set_ratio(&mut self, id: u64, ratio: f32) -> bool {
        set_ratio(&mut self.root, id, ratio.clamp(0.15, 0.85))
    }

    pub fn layout(&self, bounds: Rect) -> PaneLayout {
        let mut panes = Vec::new();
        let mut dividers = Vec::new();
        place(&self.root, bounds, &mut panes, &mut dividers);
        PaneLayout { panes, dividers }
    }
}

fn leaf_count(node: &Node) -> usize {
    match node {
        Node::Leaf { .. } => 1,
        Node::Split { first, second, .. } => leaf_count(first) + leaf_count(second),
    }
}

fn find_leaf(node: &Node, id: u64) -> Option<(u64, Option<TabId>)> {
    match node {
        Node::Leaf { id: leaf, tab } if *leaf == id => Some((*leaf, *tab)),
        Node::Leaf { .. } => None,
        Node::Split { first, second, .. } => find_leaf(first, id).or_else(|| find_leaf(second, id)),
    }
}

fn leaf_with(node: &Node, tab: TabId) -> Option<u64> {
    match node {
        Node::Leaf { id, tab: shown } if *shown == Some(tab) => Some(*id),
        Node::Leaf { .. } => None,
        Node::Split { first, second, .. } => {
            leaf_with(first, tab).or_else(|| leaf_with(second, tab))
        }
    }
}

fn first_leaf(node: &Node) -> u64 {
    match node {
        Node::Leaf { id, .. } => *id,
        Node::Split { first, .. } => first_leaf(first),
    }
}

fn clear_tab(node: &mut Node, tab: TabId) {
    match node {
        Node::Leaf { tab: shown, .. } if *shown == Some(tab) => *shown = None,
        Node::Leaf { .. } => {}
        Node::Split { first, second, .. } => {
            clear_tab(first, tab);
            clear_tab(second, tab);
        }
    }
}

fn set_leaf_tab(node: &mut Node, id: u64, tab: Option<TabId>) -> bool {
    match node {
        Node::Leaf {
            id: leaf,
            tab: shown,
        } if *leaf == id => {
            *shown = tab;
            true
        }
        Node::Leaf { .. } => false,
        Node::Split { first, second, .. } => {
            set_leaf_tab(first, id, tab) || set_leaf_tab(second, id, tab)
        }
    }
}

fn split_leaf(
    node: &mut Node,
    target: u64,
    axis: SplitAxis,
    companion: Option<TabId>,
    next_id: &mut u64,
) -> Option<u64> {
    match node {
        Node::Leaf { id, tab } if *id == target => {
            let new_id = *next_id;
            *next_id += 1;
            let split_id = *next_id;
            *next_id += 1;
            *node = Node::Split {
                id: split_id,
                axis,
                ratio: 0.5,
                first: Box::new(Node::Leaf { id: *id, tab: *tab }),
                second: Box::new(Node::Leaf {
                    id: new_id,
                    tab: companion,
                }),
            };
            Some(new_id)
        }
        Node::Split { first, second, .. } => split_leaf(first, target, axis, companion, next_id)
            .or_else(|| split_leaf(second, target, axis, companion, next_id)),
        Node::Leaf { .. } => None,
    }
}

fn cut(node: Node, target: u64) -> Option<Node> {
    match node {
        Node::Leaf { id, .. } if id == target => None,
        Node::Leaf { .. } => Some(node),
        Node::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => match (cut(*first, target), cut(*second, target)) {
            (Some(first), Some(second)) => Some(Node::Split {
                id,
                axis,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            }),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        },
    }
}

fn set_ratio(node: &mut Node, id: u64, ratio: f32) -> bool {
    match node {
        Node::Split {
            id: split,
            ratio: slot,
            first,
            second,
            ..
        } => {
            if *split == id {
                *slot = ratio;
                return true;
            }
            set_ratio(first, id, ratio) || set_ratio(second, id, ratio)
        }
        Node::Leaf { .. } => false,
    }
}

fn place(node: &Node, rect: Rect, panes: &mut Vec<PlacedPane>, dividers: &mut Vec<SplitDivider>) {
    match node {
        Node::Leaf { id, tab } => panes.push(PlacedPane {
            id: *id,
            tab: *tab,
            rect,
        }),
        Node::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let (first_rect, second_rect, divider) = split_rects(rect, *axis, *ratio);
            place(first, first_rect, panes, dividers);
            place(second, second_rect, panes, dividers);
            dividers.push(SplitDivider {
                id: *id,
                axis: *axis,
                rect: divider,
                bounds: rect,
            });
        }
    }
}

fn split_rects(rect: Rect, axis: SplitAxis, ratio: f32) -> (Rect, Rect, Rect) {
    let ratio = ratio.clamp(0.15, 0.85);
    match axis {
        SplitAxis::LeftRight => {
            let usable = (rect.width() - GAP).max(0.0);
            let mut first = usable * ratio;
            if usable >= MIN_PANE * 2.0 {
                first = first.clamp(MIN_PANE, usable - MIN_PANE);
            }
            let x = rect.left() + first;
            (
                Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())),
                Rect::from_min_max(Pos2::new(x + GAP, rect.top()), rect.max),
                Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + GAP, rect.bottom())),
            )
        }
        SplitAxis::TopBottom => {
            let usable = (rect.height() - GAP).max(0.0);
            let mut first = usable * ratio;
            if usable >= MIN_PANE * 2.0 {
                first = first.clamp(MIN_PANE, usable - MIN_PANE);
            }
            let y = rect.top() + first;
            (
                Rect::from_min_max(rect.min, Pos2::new(rect.right(), y)),
                Rect::from_min_max(Pos2::new(rect.left(), y + GAP), rect.max),
                Rect::from_min_max(Pos2::new(rect.left(), y), Pos2::new(rect.right(), y + GAP)),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Vec2;

    fn bounds() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0))
    }

    fn separated(layout: &PaneLayout) -> bool {
        layout.panes.iter().enumerate().all(|(index, pane)| {
            layout
                .panes
                .iter()
                .skip(index + 1)
                .all(|other| !pane.rect.intersects(other.rect.shrink(0.5)))
        })
    }

    #[test]
    fn left_right_split_shares_the_width_and_keeps_a_gap() {
        let mut panes = Panes::default();
        let created = panes.split_focused(SplitAxis::LeftRight, None);
        assert!(created.is_some());
        assert_eq!(panes.focused_id(), 1);
        let layout = panes.layout(bounds());
        assert_eq!(layout.panes.len(), 2);
        assert_eq!(layout.dividers.len(), 1);
        assert_eq!(layout.dividers[0].axis, SplitAxis::LeftRight);
        assert!(separated(&layout));
        assert!((layout.panes[0].rect.width() - layout.panes[1].rect.width()).abs() < 1.0);
        assert!((layout.panes[0].rect.right() + GAP - layout.panes[1].rect.left()).abs() < 0.1);
        assert_eq!(layout.panes[0].rect.height(), 800.0);
    }

    #[test]
    fn top_bottom_split_shares_the_height() {
        let mut panes = Panes::default();
        panes.split_focused(SplitAxis::TopBottom, None);
        let layout = panes.layout(bounds());
        assert!((layout.panes[0].rect.height() - layout.panes[1].rect.height()).abs() < 1.0);
        assert!((layout.panes[0].rect.bottom() + GAP - layout.panes[1].rect.top()).abs() < 0.1);
        assert_eq!(layout.panes[0].rect.width(), 1000.0);
        assert!(separated(&layout));
    }

    #[test]
    fn nested_splits_cover_custom_arrangements_and_close_collapses() {
        let mut panes = Panes::default();
        panes.split_focused(SplitAxis::LeftRight, None);
        panes.split_focused(SplitAxis::TopBottom, None);
        assert_eq!(panes.leaf_count(), 3);
        let layout = panes.layout(bounds());
        assert_eq!(layout.panes.len(), 3);
        assert_eq!(layout.dividers.len(), 2);
        assert!(separated(&layout));
        let focused = panes.focused_id();
        assert!(panes.close(focused));
        assert_eq!(panes.leaf_count(), 2);
        assert!(panes.tab_of(focused).is_none());
        assert_ne!(panes.focused_id(), focused);
        let layout = panes.layout(bounds());
        assert!(separated(&layout));
        assert!(panes.close(panes.focused_id()));
        assert!(!panes.is_split());
        assert!(!panes.close(panes.focused_id()));
    }

    #[test]
    fn divider_ratio_moves_the_first_pane() {
        let mut panes = Panes::default();
        panes.split_focused(SplitAxis::LeftRight, None);
        let layout = panes.layout(bounds());
        assert!(panes.set_ratio(layout.dividers[0].id, 0.25));
        let layout = panes.layout(bounds());
        assert!(layout.panes[0].rect.width() < layout.panes[1].rect.width());
        assert!(layout.panes[0].rect.width() >= MIN_PANE - 0.1);
    }
}
