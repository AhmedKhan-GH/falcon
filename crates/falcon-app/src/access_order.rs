//! Keeps the accessibility tree in the order things are stacked on screen.
//!
//! AccessKit answers "what is at this point?" by walking the tree last child
//! first, and egui builds the tree in the order widgets are created. A popup
//! hangs off the chip or row that opened it, so a grid drawn later in the
//! frame is reached first and takes a press aimed at a swatch in the popup
//! above it. Pointer clicks are not affected: egui hit-tests them by layer.
//!
//! So before each tree update leaves the app, every node's children are
//! sorted by the highest layer order in their branch, keeping creation order
//! within an order, and a menu or dialog gets its window's bounds so a press
//! between its controls lands on it and not on what lies underneath.

use std::collections::HashMap;

use eframe::egui::{self, Order, Rect, accesskit};
use egui::accesskit::{Node, NodeId, Role};

/// What the app knows about a node from egui's widget list.
#[derive(Clone, Copy)]
pub(crate) struct Placement {
    pub order: Order,
    pub rect: Rect,
}

pub(crate) struct AccessOrder;

impl egui::Plugin for AccessOrder {
    fn debug_name(&self) -> &'static str {
        "AccessOrder"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        let Some(update) = output.platform_output.accesskit_update.as_mut() else {
            return;
        };
        let Some(root) = update.tree.as_ref().map(|t| t.root) else {
            return;
        };
        // `end_pass` has already moved this frame's widgets to `prev_pass`.
        let places: HashMap<NodeId, Placement> = ctx.viewport(|v| {
            let widgets = &v.prev_pass.widgets;
            widgets.layer_ids().flat_map(|layer| widgets.get_layer(layer).map(move |w| (w.id.accesskit_id(), Placement { order: layer.order, rect: w.rect }))).collect()
        });
        stack_nodes(&mut update.nodes, root, &places);
    }
}

/// Sorts children so the topmost branch comes last, and gives menus and
/// dialogs their bounds.
pub(crate) fn stack_nodes(nodes: &mut [(NodeId, Node)], root: NodeId, places: &HashMap<NodeId, Placement>) {
    let index: HashMap<NodeId, usize> = nodes.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
    let mut highest: HashMap<NodeId, Order> = HashMap::new();
    // Depth first from the root, parents before children; the reverse visits
    // every child before its parent, whatever the depth.
    let mut walk = vec![root];
    let mut at = 0;
    while let Some(id) = walk.get(at).copied() {
        at += 1;
        if let Some(i) = index.get(&id) {
            walk.extend_from_slice(nodes[*i].1.children());
        }
    }
    for id in walk.iter().rev() {
        let Some(&i) = index.get(id) else { continue };
        let own = places.get(id).map_or(Order::Background, |p| p.order);
        let node = &mut nodes[i].1;
        let mut kids = node.children().to_vec();
        kids.sort_by_key(|k| highest.get(k).copied().unwrap_or(Order::Background));
        let top = kids.last().and_then(|k| highest.get(k).copied()).unwrap_or(Order::Background);
        highest.insert(*id, own.max(top));
        node.set_children(kids);
        if matches!(node.role(), Role::Menu | Role::Dialog)
            && node.bounds().is_none()
            && let Some(p) = places.get(id).filter(|p| p.rect.is_positive())
        {
            node.set_bounds(accesskit::Rect { x0: p.rect.min.x.into(), y0: p.rect.min.y.into(), x1: p.rect.max.x.into(), y1: p.rect.max.y.into() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn id(n: u64) -> NodeId {
        NodeId(n)
    }

    fn place(order: Order) -> Placement {
        Placement { order, rect: Rect::from_min_size(pos2(1.0, 2.0), vec2(30.0, 40.0)) }
    }

    fn node(role: Role, kids: &[u64]) -> Node {
        let mut n = Node::new(role);
        n.set_children(kids.iter().map(|k| id(*k)).collect::<Vec<_>>());
        n
    }

    fn kids(nodes: &[(NodeId, Node)], parent: u64) -> Vec<u64> {
        nodes.iter().find(|(i, _)| *i == id(parent)).unwrap().1.children().iter().map(|k| k.0).collect()
    }

    #[test]
    fn a_popups_branch_moves_above_later_widgets_at_every_level() {
        // root > [bar > [chip > [menu], clear], card, other]
        let mut nodes = vec![
            (id(0), node(Role::Window, &[1, 5, 6])),
            (id(1), node(Role::GenericContainer, &[2, 4])),
            (id(2), node(Role::Button, &[3])),
            (id(3), node(Role::Menu, &[])),
            (id(4), node(Role::Button, &[])),
            (id(5), node(Role::Button, &[])),
            (id(6), node(Role::Button, &[])),
        ];
        let places = HashMap::from([(id(3), place(Order::Foreground))]);
        stack_nodes(&mut nodes, id(0), &places);
        assert_eq!(kids(&nodes, 0), [5, 6, 1], "the rest keeps its order");
        assert_eq!(kids(&nodes, 1), [4, 2]);
    }

    #[test]
    fn a_tree_with_nothing_on_top_keeps_its_order() {
        let mut nodes = vec![(id(0), node(Role::Window, &[2, 1])), (id(1), node(Role::Button, &[])), (id(2), node(Role::Button, &[]))];
        stack_nodes(&mut nodes, id(0), &HashMap::new());
        assert_eq!(kids(&nodes, 0), [2, 1]);
    }

    #[test]
    fn higher_layers_sort_above_lower_ones() {
        let mut nodes = vec![(id(0), node(Role::Window, &[1, 2, 3])), (id(1), node(Role::Tooltip, &[])), (id(2), node(Role::Menu, &[])), (id(3), node(Role::Button, &[]))];
        let places = HashMap::from([(id(1), place(Order::Tooltip)), (id(2), place(Order::Foreground))]);
        stack_nodes(&mut nodes, id(0), &places);
        assert_eq!(kids(&nodes, 0), [3, 2, 1]);
    }

    #[test]
    fn menus_get_their_bounds_and_keep_ones_they_have() {
        let mut preset = node(Role::Dialog, &[]);
        preset.set_bounds(accesskit::Rect { x0: 7.0, y0: 7.0, x1: 8.0, y1: 8.0 });
        let mut nodes = vec![(id(0), node(Role::Window, &[1, 2, 3])), (id(1), node(Role::Menu, &[])), (id(2), preset), (id(3), node(Role::GenericContainer, &[]))];
        let places = HashMap::from([(id(1), place(Order::Foreground)), (id(2), place(Order::Foreground)), (id(3), place(Order::Foreground))]);
        stack_nodes(&mut nodes, id(0), &places);
        let bounds = |n: usize| nodes[n].1.bounds().map(|b| (b.x0, b.y0, b.x1, b.y1));
        assert_eq!(bounds(1), Some((1.0, 2.0, 31.0, 42.0)));
        assert_eq!(bounds(2), Some((7.0, 7.0, 8.0, 8.0)));
        assert_eq!(bounds(3), None);
    }
}
