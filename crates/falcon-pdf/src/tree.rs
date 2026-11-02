//! The layers list's tree, from a configuration's `/Order` array. The
//! reading of the file itself is platform code; this turns what it read
//! into the tree, following the PDF spec and what pdf.js shows.
//!
//! In `/Order` an optional content group is listed by reference. An array
//! whose first element is a string is a labelled group of what follows the
//! label; the label is text, not a switch. Any other array that follows a
//! group nests under that group. Groups the order never names are listed
//! last, under "additional layers", as pdf.js does. Turning a group on or
//! off never changes the groups under it: the nesting is only for reading.

/// What a walk of `/Order` found.
#[derive(Clone, Debug, PartialEq)]
pub enum OrderItem {
    /// A group of `/OCGs`, by its index there.
    Group(usize),
    /// A string.
    Text(String),
    /// A nested array.
    Array(Vec<OrderItem>),
    /// Anything else, such as a reference to something that isn't one of
    /// the document's groups. Kept so that what follows isn't mistaken for
    /// its children.
    Other,
}

/// One row of the layers tree.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerNode {
    /// A switch: the group's index in [`crate::Document::layers`], and the
    /// rows nested under it.
    Layer { index: usize, children: Vec<LayerNode> },
    /// A heading with no switch.
    Label { text: String, children: Vec<LayerNode> },
    /// The heading for the groups `/Order` leaves out; its wording is the
    /// reader's.
    Additional(Vec<LayerNode>),
}

/// pdf.js stops reading nested arrays past this depth.
const MAX_LEVELS: usize = 10;

/// The tree for `order`, in a document of `groups` groups. Indices past
/// `groups` are skipped. Nothing when the document has no groups.
pub fn layer_tree(order: &[OrderItem], groups: usize) -> Vec<LayerNode> {
    let mut seen = vec![false; groups];
    let mut tree = level(order, 0, &mut seen);
    let rest: Vec<LayerNode> = (0..groups).filter(|&i| !seen[i]).map(|index| LayerNode::Layer { index, children: Vec::new() }).collect();
    if !rest.is_empty() {
        tree.push(LayerNode::Additional(rest));
    }
    tree
}

fn level(items: &[OrderItem], depth: usize, seen: &mut [bool]) -> Vec<LayerNode> {
    let mut out: Vec<LayerNode> = Vec::new();
    if depth > MAX_LEVELS {
        return out;
    }
    // Whether the last item was a group, whose children an array may be.
    let mut after_group = false;
    for item in items {
        let mut group = false;
        match item {
            OrderItem::Group(i) if *i < seen.len() => {
                seen[*i] = true;
                out.push(LayerNode::Layer { index: *i, children: Vec::new() });
                group = true;
            }
            OrderItem::Array(inner) => match inner.first() {
                Some(OrderItem::Text(text)) => {
                    let children = level(&inner[1..], depth + 1, seen);
                    if !children.is_empty() {
                        out.push(LayerNode::Label { text: text.clone(), children });
                    }
                }
                _ => {
                    let nested = level(inner, depth + 1, seen);
                    match out.last_mut() {
                        Some(LayerNode::Layer { children, .. }) if after_group => children.extend(nested),
                        _ => out.extend(nested),
                    }
                }
            },
            _ => {}
        }
        after_group = group;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::OrderItem::*;
    use super::*;

    fn layer(index: usize, children: Vec<LayerNode>) -> LayerNode {
        LayerNode::Layer { index, children }
    }

    fn label(text: &str, children: Vec<LayerNode>) -> LayerNode {
        LayerNode::Label { text: text.into(), children }
    }

    #[test]
    fn groups_labels_and_children_nest() {
        let order = [Group(0), Array(vec![Group(1), Group(2)]), Array(vec![Text("Notes".into()), Group(3)])];
        assert_eq!(layer_tree(&order, 4), vec![layer(0, vec![layer(1, vec![]), layer(2, vec![])]), label("Notes", vec![layer(3, vec![])])]);
    }

    #[test]
    fn groups_left_out_go_last_and_unknown_ones_are_skipped() {
        let order = [Group(2), Group(9), Other, Array(vec![Text("Empty".into())])];
        assert_eq!(layer_tree(&order, 3), vec![layer(2, vec![]), LayerNode::Additional(vec![layer(0, vec![]), layer(1, vec![])])]);
        assert!(layer_tree(&[], 0).is_empty());
    }

    #[test]
    fn an_array_after_something_skipped_is_not_its_children() {
        let order = [Group(0), Other, Array(vec![Group(1)])];
        assert_eq!(layer_tree(&order, 2), vec![layer(0, vec![]), layer(1, vec![])]);
        // With no group before it at all.
        assert_eq!(layer_tree(&[Array(vec![Group(1)])], 2), vec![layer(1, vec![]), LayerNode::Additional(vec![layer(0, vec![])])]);
    }

    #[test]
    fn a_group_listed_twice_is_shown_twice() {
        assert_eq!(layer_tree(&[Group(0), Array(vec![Text("Again".into()), Group(0)])], 1), vec![layer(0, vec![]), label("Again", vec![layer(0, vec![])])]);
    }

    #[test]
    fn nesting_stops_at_pdf_js_limit() {
        let mut deep = vec![Group(1)];
        for _ in 0..30 {
            deep = vec![Array(vec![Text("L".into())].into_iter().chain(deep).collect())];
        }
        let tree = layer_tree(&deep, 2);
        let mut depth = 0;
        let mut at = &tree;
        while let Some(LayerNode::Label { children, .. }) = at.first() {
            depth += 1;
            at = children;
        }
        assert!(depth == 0, "past the limit nothing is kept, got {depth} levels");
        // Group 1 was dropped with it, so it lands in the additional layers.
        assert!(tree.iter().any(|n| matches!(n, LayerNode::Additional(c) if c.contains(&layer(1, vec![])))));
        let shallow = vec![Array(vec![Text("L".into()), Group(0)])];
        assert_eq!(layer_tree(&shallow, 1), vec![label("L", vec![layer(0, vec![])])]);
    }
}
