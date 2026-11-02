//! The Color popup over a card, driven through a real `Context`: pointer
//! events and AccessKit presses must reach the swatch and never the card.

use eframe::egui::{self, Event, Id, PointerButton, Pos2, Rect, Sense, accesskit, pos2, vec2};
use falcon_library::search::Filter;

use super::*;
use crate::access_order::AccessOrder;

const CHIP: Pos2 = pos2(20.0, 20.0);
const RED: [u8; 3] = PRESET_COLORS[0];

#[derive(Default)]
struct Scene {
    filter: Filter,
    text: String,
    card_clicked: bool,
    tree: Option<accesskit::TreeUpdate>,
}

impl Scene {
    /// One frame laid out as the app does: the filter bar, then the grid
    /// drawn later in the frame and lying under the popup.
    fn frame(&mut self, ctx: &egui::Context, events: Vec<Event>) {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let mut clicked = false;
        let mut out = ctx.run_ui(input, |ui| {
            let bar = egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 30.0)));
            ui.scope_builder(bar, |ui| color_filter_chip(ui, &mut self.filter, &mut self.text));
            let card = Rect::from_min_size(pos2(0.0, 50.0), vec2(800.0, 550.0));
            clicked = ui.interact(card, Id::unique("card"), Sense::click_and_drag()).clicked();
        });
        out.textures_delta.clear();
        self.card_clicked |= clicked;
        self.tree = out.platform_output.accesskit_update.take();
    }

    /// A press and release with the pointer arriving in the same frame, as
    /// a synthetic click does. Runs on until the click has settled.
    fn click(&mut self, ctx: &egui::Context, pos: Pos2) {
        self.card_clicked = false;
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        self.frame(ctx, vec![Event::PointerMoved(pos), button(true)]);
        self.frame(ctx, vec![button(false)]);
        self.frame(ctx, vec![]);
    }

    fn node(&self, name: &str) -> (accesskit::NodeId, accesskit::Node) {
        let tree = self.tree.as_ref().expect("AccessKit is on");
        let found = tree.nodes.iter().find(|(_, n)| n.label() == Some(name));
        found.cloned().unwrap_or_else(|| panic!("no node named {name}"))
    }

    fn center(&self, name: &str) -> Pos2 {
        let b = self.node(name).1.bounds().expect("swatches have bounds");
        pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
    }

    /// What a screen reader or automation tool finds at a point: the same
    /// walk and filter `accesskit_macos` uses for its hit test.
    fn accessible_at<R>(&self, pos: Pos2, read: impl FnOnce(accesskit_consumer::NodeRef<'_>) -> R) -> R {
        let tree = accesskit_consumer::Tree::new(self.tree.clone().expect("AccessKit is on"), true);
        let state = tree.state();
        let node = state.root().node_at_point(accesskit::Point::new(pos.x.into(), pos.y.into()), &accesskit_consumer::common_filter).expect("something is there");
        read(node)
    }

    fn press(&mut self, ctx: &egui::Context, target_node: accesskit::NodeId) {
        self.card_clicked = false;
        let request = accesskit::ActionRequest { action: accesskit::Action::Click, target_tree: accesskit::TreeId::ROOT, target_node, data: None };
        self.frame(ctx, vec![Event::AccessKitActionRequest(request)]);
        self.frame(ctx, vec![]);
    }
}

/// A context with the Color popup open and measured.
fn open_popup() -> (egui::Context, Scene) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.add_plugin(AccessOrder);
    let mut s = Scene::default();
    s.frame(&ctx, vec![]);
    s.click(&ctx, CHIP);
    s.frame(&ctx, vec![]);
    s.frame(&ctx, vec![]);
    (ctx, s)
}

#[test]
fn clicking_a_swatch_picks_the_colour_and_not_the_card() {
    let (ctx, mut s) = open_popup();
    let pos = s.center(&preset_color_name(RED));
    s.click(&ctx, pos);
    assert_eq!(s.filter.color.map(|q| q.rgb), Some(RED));
    assert_eq!(s.text, logic::hex(RED));
    assert!(!s.card_clicked, "the click went through the popup");
    // The popup stays open for another pick.
    let other = PRESET_COLORS[3];
    let pos = s.center(&preset_color_name(other));
    s.click(&ctx, pos);
    assert_eq!(s.filter.color.map(|q| q.rgb), Some(other));
    assert!(!s.card_clicked);
}

#[test]
fn every_swatch_answers_a_click() {
    let (ctx, mut s) = open_popup();
    for c in PRESET_COLORS {
        let pos = s.center(&preset_color_name(c));
        s.click(&ctx, pos);
        assert_eq!(s.filter.color.map(|q| q.rgb), Some(c));
        assert!(!s.card_clicked);
    }
}

#[test]
fn clicking_between_swatches_reaches_nothing_underneath() {
    let (ctx, mut s) = open_popup();
    let (a, b) = (s.center(&preset_color_name(PRESET_COLORS[0])), s.center(&preset_color_name(PRESET_COLORS[1])));
    s.click(&ctx, pos2((a.x + b.x) / 2.0, a.y));
    assert_eq!(s.filter.color, None);
    assert!(!s.card_clicked, "the gap let the click through");
}

#[test]
fn the_same_spot_reaches_the_card_once_the_popup_is_gone() {
    let (ctx, mut s) = open_popup();
    let pos = s.center(&preset_color_name(RED));
    // Clicking outside closes the popup; that click may land on the card.
    s.click(&ctx, pos2(700.0, 500.0));
    s.frame(&ctx, vec![]);
    s.click(&ctx, pos);
    assert!(s.card_clicked, "the scene's card is reachable, so the tests above mean something");
    assert_eq!(s.filter.color, None);
}

#[test]
fn swatches_are_exposed_with_their_names_and_state() {
    let (ctx, mut s) = open_popup();
    let (_, red) = s.node(&preset_color_name(RED));
    assert_eq!(red.role(), egui::Role::RadioButton);
    assert!(red.supports_action(accesskit::Action::Click));
    assert_eq!(red.toggled(), Some(accesskit::Toggled::False));
    let pos = s.center(&preset_color_name(RED));
    s.click(&ctx, pos);
    assert_eq!(s.node(&preset_color_name(RED)).1.toggled(), Some(accesskit::Toggled::True));
}

#[test]
fn every_preset_has_its_own_name() {
    let names: std::collections::HashSet<String> = PRESET_COLORS.iter().map(|c| preset_color_name(*c)).collect();
    assert_eq!(names.len(), PRESET_COLORS.len());
    assert!(!names.iter().any(|n| n.starts_with('#')));
}

#[test]
fn an_accessibility_press_on_a_swatch_picks_the_colour() {
    let (ctx, mut s) = open_popup();
    let (id, _) = s.node(&preset_color_name(RED));
    s.press(&ctx, id);
    assert_eq!(s.filter.color.map(|q| q.rgb), Some(RED));
    assert!(!s.card_clicked);
    assert_eq!(s.node(&preset_color_name(RED)).1.toggled(), Some(accesskit::Toggled::True), "the popup stayed open");
}

/// The reported bug: pressing what is at a swatch's position. The press
/// goes wherever the accessibility hit test says.
#[test]
fn the_accessibility_hit_test_finds_the_swatch_above_the_card() {
    let (ctx, mut s) = open_popup();
    let name = preset_color_name(RED);
    let (found, label, clickable) = s.accessible_at(s.center(&name), |n| (n.locate().0, n.label(), n.is_clickable(&accesskit_consumer::common_filter)));
    assert_eq!(label.as_deref(), Some(name.as_str()));
    assert!(clickable);
    s.press(&ctx, found);
    assert_eq!(s.filter.color.map(|q| q.rgb), Some(RED));
    assert!(!s.card_clicked);
}

#[test]
fn the_accessibility_hit_test_stops_at_the_popup_between_swatches() {
    let (ctx, mut s) = open_popup();
    let (a, b) = (s.center(&preset_color_name(PRESET_COLORS[0])), s.center(&preset_color_name(PRESET_COLORS[1])));
    let (found, role, clickable) = s.accessible_at(pos2((a.x + b.x) / 2.0, a.y), |n| (n.locate().0, n.role(), n.is_clickable(&accesskit_consumer::common_filter)));
    assert_eq!(role, egui::Role::Menu);
    assert!(!clickable);
    s.press(&ctx, found);
    assert!(!s.card_clicked);
    assert_eq!(s.filter.color, None);
}

#[test]
fn without_the_reorder_the_hit_test_falls_to_the_card() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut s = Scene::default();
    s.frame(&ctx, vec![]);
    s.click(&ctx, CHIP);
    s.frame(&ctx, vec![]);
    s.frame(&ctx, vec![]);
    let name = preset_color_name(RED);
    let bounds = s.accessible_at(s.center(&name), |n| n.bounding_box().map(|b| (b.width(), b.height())));
    assert_eq!(bounds, Some((800.0, 550.0)), "egui fixed its tree order: `access_order` may be unnecessary");
}

/// Any popup over a card, as the other filter chips and colour rows open
/// theirs. Returns whether the card reported a click in this frame.
fn popup_frame(ctx: &egui::Context, events: Vec<Event>, content: &mut dyn FnMut(&mut Ui)) -> (bool, Option<accesskit::TreeUpdate>) {
    let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
    let mut card_clicked = false;
    let mut out = ctx.run_ui(input, |ui| {
        let bar = egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 30.0)));
        ui.scope_builder(bar, |ui| {
            let resp = filter_chip(ui, "Chip", None);
            egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| content(ui));
        });
        let card = Rect::from_min_size(pos2(0.0, 50.0), vec2(800.0, 550.0));
        card_clicked = ui.interact(card, Id::unique("card"), Sense::click_and_drag()).clicked();
    });
    out.textures_delta.clear();
    (card_clicked, out.platform_output.accesskit_update.take())
}

/// Opens a popup, then clicks and presses the middle of its area, and says
/// whether the card ever heard of it.
fn card_hears_through(content: &mut dyn FnMut(&mut Ui)) -> bool {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.add_plugin(AccessOrder);
    let click = |ctx: &egui::Context, pos: Pos2, content: &mut dyn FnMut(&mut Ui)| {
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        let a = popup_frame(ctx, vec![Event::PointerMoved(pos), button(true)], content).0;
        let b = popup_frame(ctx, vec![button(false)], content).0;
        let (c, tree) = popup_frame(ctx, vec![], content);
        (a || b || c, tree)
    };
    popup_frame(&ctx, vec![], content);
    click(&ctx, CHIP, content);
    popup_frame(&ctx, vec![], content);
    let (_, tree) = popup_frame(&ctx, vec![], content);
    let tree = tree.expect("AccessKit is on");
    let menu = tree.nodes.iter().find(|(_, n)| n.role() == egui::Role::Menu).expect("the popup is open");
    let b = menu.1.bounds().expect("the popup has bounds");
    // Just inside the corner: the popup's frame, where no control sits.
    let corner = pos2(b.x0 as f32 + 2.0, b.y1 as f32 - 2.0);
    let tree_state = accesskit_consumer::Tree::new(tree.clone(), true);
    let found = tree_state.state().root().node_at_point(accesskit::Point::new(corner.x.into(), corner.y.into()), &accesskit_consumer::common_filter).expect("something is there").locate().0;
    assert_eq!(found, menu.0, "the accessibility hit test went past the popup");
    click(&ctx, corner, content).0
}

#[test]
fn other_popups_block_the_card_too() {
    let (mut f, mut text) = (TextFilter::default(), String::new());
    assert!(!card_hears_through(&mut |ui| text_filter_menu(ui, &mut f, &mut text)), "the text filter popup");
    assert!(!card_hears_through(&mut |ui| {
        crate::ui::organize::color_row(ui, Some("red"));
    }), "the folder colour row");
    let mut rgb = [10, 20, 30];
    assert!(!card_hears_through(&mut |ui| {
        crate::ui::color_picker::sv_hue_picker(ui, Id::unique("picker"), &mut rgb);
    }), "the colour picker");
}

#[test]
fn folder_colours_are_named_and_pressable() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut picked = None;
    let mut run = |events| {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            if let Some(p) = crate::ui::organize::color_row(ui, None) {
                picked = Some(p);
            }
        });
        out.textures_delta.clear();
        out.platform_output.accesskit_update.take().expect("AccessKit is on")
    };
    run(vec![]);
    let tree = run(vec![]);
    let (id, node) = tree.nodes.iter().find(|(_, n)| n.label() == Some(crate::folder_icons::color_label("green"))).expect("green is exposed");
    assert_eq!(node.role(), egui::Role::RadioButton);
    let request = accesskit::ActionRequest { action: accesskit::Action::Click, target_tree: accesskit::TreeId::ROOT, target_node: *id, data: None };
    run(vec![Event::AccessKitActionRequest(request)]);
    assert_eq!(picked, Some(Some("green".to_string())));
}
