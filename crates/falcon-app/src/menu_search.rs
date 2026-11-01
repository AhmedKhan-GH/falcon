//! A type-to-filter search box on top of a context menu, after Eagle's
//! searchable menus (item, folder, smart folder and tag).
//!
//! A menu wraps its body: `resp.context_menu(|ui| menu_search::show(ui,
//! |ui| { … }))`. The body stays an ordinary egui menu; nothing in it
//! changes. While the search box is empty the body is drawn as usual.
//! Once something is typed the body still runs every frame, but out of
//! sight on a layer of its own that is never drawn or hit, and the menu
//! shows the body's matching entries instead. Each entry is one of the
//! body's enabled clickable widgets, labelled with the text it painted
//! (its first text, plus its last as the shortcut). Choosing one hands
//! that widget an accessibility click request before the body runs, so
//! the body's own `clicked()` sees it and acts as if it were clicked.
//!
//! As in Eagle, submenu buttons are left out of the results; unlike
//! Eagle, the entries inside submenus are not searched (egui only runs a
//! submenu's contents while it is open).

use eframe::egui::{
    self, Align2, Event, Id, Key, LayerId, Modifiers, Order, Rect, Sense, Shape, TextEdit, Ui, UiBuilder,
    accesskit,
    containers::menu::{MenuState, SubMenu},
    vec2,
};

/// One choice the body offers.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    id: Id,
    label: String,
    /// Shortcut text drawn at the right of the button, if any.
    keys: String,
}

#[derive(Clone, Default)]
struct State {
    query: String,
    /// The body's choices, from the last frame.
    entries: Vec<Entry>,
    /// The highlighted row among the matches.
    sel: usize,
    /// The body's width while it was last shown, so the menu keeps it.
    width: f32,
    last_pass: u64,
}

/// Draws a search box and then `body`, filtered by what is typed.
/// Returns what `body` returned (it runs every frame, shown or not).
pub fn show<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    let ctx = ui.ctx().clone();
    // The popup's own id: unique per menu, and the base of its state.
    let menu = ui.layer_id().id;
    let key = menu.with("menu-search");
    let pass = ctx.cumulative_pass_nr();
    let mut st: State = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    // Not shown in the previous pass: the menu has just opened.
    let fresh = st.last_pass == 0 || st.last_pass + 1 < pass;
    if fresh {
        st = State { width: 180.0, ..State::default() };
    }
    st.last_pass = pass;

    let field_id = menu.with("menu-search-field");
    let field = ui.add(TextEdit::singleline(&mut st.query).id(field_id).hint_text("Search").desired_width(st.width.max(160.0)));
    if fresh {
        field.request_focus();
    }
    if field.clicked() || field.is_pointer_button_down_on() {
        // A context menu closes on any click inside it; not on one that
        // only puts the cursor in the search box.
        ctx.data_mut(|d| d.insert_temp(menu.with("press_started_while_open"), false));
    }
    let filtering = !st.query.trim().is_empty();
    if field.changed() {
        st.sel = 0;
        if filtering {
            // A submenu left open would hang beside the results.
            MenuState::from_ui(ui, |s, _| s.open_item = None);
        }
    }
    ui.separator();

    let body_id = menu.with("menu-body");
    let result = if filtering {
        ui.set_min_width(st.width);
        let hits = rank(&st.entries, &st.query);
        if !hits.is_empty() {
            let last = hits.len() - 1;
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                st.sel = (st.sel + 1).min(last);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                st.sel = st.sel.saturating_sub(1);
            }
            st.sel = st.sel.min(last);
        }
        let mut fire = None;
        let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        for (row, &ix) in hits.iter().enumerate() {
            let e = &st.entries[ix];
            let h = ui.spacing().interact_size.y;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
            if resp.hovered() && moved {
                st.sel = row;
            }
            let v = ui.visuals();
            if row == st.sel {
                ui.painter().rect_filled(rect, v.widgets.hovered.corner_radius, v.widgets.hovered.weak_bg_fill);
            }
            let text = if row == st.sel { v.widgets.hovered.text_color() } else { v.widgets.inactive.text_color() };
            let pad = ui.spacing().button_padding.x;
            let font = egui::TextStyle::Button.resolve(ui.style());
            ui.painter().text(rect.left_center() + vec2(pad, 0.0), Align2::LEFT_CENTER, &e.label, font.clone(), text);
            if !e.keys.is_empty() {
                ui.painter().text(rect.right_center() - vec2(pad, 0.0), Align2::RIGHT_CENTER, &e.keys, font, v.weak_text_color());
            }
            if resp.hovered() && ui.input(|i| i.pointer.primary_clicked()) {
                fire = Some(e.id);
            }
        }
        if hits.is_empty() {
            ui.label(egui::RichText::new("No matches").color(ui.visuals().weak_text_color()));
        } else if ui.input(|i| i.key_pressed(Key::Enter)) {
            fire = Some(st.entries[hits[st.sel.min(hits.len() - 1)]].id);
        }
        if let Some(id) = fire {
            click(&ctx, id);
        }
        // The body, out of sight: far to the left of the window (so no
        // pointer is over its buttons), on a layer that is no area (so it
        // is never hit), clipped to itself (so it still paints its text).
        let hidden = LayerId::new(Order::Middle, menu.with("menu-search-hidden"));
        let far = Rect::from_min_size(ui.min_rect().left_top() - vec2(100_000.0, 0.0), vec2(st.width.max(160.0), 10_000.0));
        let mut child = ui.new_child(UiBuilder::new().scope_id(body_id).layer_id(hidden).max_rect(far).layout(*ui.layout()));
        child.set_clip_rect(far);
        let r = body(&mut child);
        st.entries = harvest(&ctx, hidden, far, field_id);
        // Nothing the body painted there may reach the screen.
        ctx.graphics_mut(|g| {
            if let Some(list) = g.get_mut(hidden) {
                *list = Default::default();
            }
        });
        if fire.is_some() {
            ui.close();
        }
        r
    } else {
        let inner = ui.scope_builder(UiBuilder::new().scope_id(body_id), body);
        st.width = inner.response.rect.width().max(160.0);
        st.entries = harvest(&ctx, ui.layer_id(), inner.response.rect, field_id);
        inner.inner
    };
    ctx.data_mut(|d| d.insert_temp(key, st));
    result
}

/// Asks the widget `id` to act as clicked this frame (egui treats an
/// accessibility click like a pointer click on any enabled button).
fn click(ctx: &egui::Context, id: Id) {
    let request = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: id.accesskit_id(),
        data: None,
    };
    ctx.input_mut(|i| i.events.push(Event::AccessKitActionRequest(request)));
}

/// The enabled clickable widgets inside `within` on `layer`, in the order
/// they were added, each labelled with the text it painted.
fn harvest(ctx: &egui::Context, layer: LayerId, within: Rect, skip: Id) -> Vec<Entry> {
    let widgets: Vec<(Id, Rect)> = ctx.viewport(|v| {
        v.this_pass
            .widgets
            .get_layer(layer)
            .filter(|w| w.id != skip && w.enabled && w.sense.senses_click() && within.contains_rect(w.rect))
            .map(|w| (w.id, w.rect))
            .collect()
    });
    let texts: Vec<(Rect, String)> = ctx.graphics(|g| {
        g.get(layer)
            .map(|list| {
                list.all_entries()
                    .filter_map(|c| match &c.shape {
                        Shape::Text(t) if !t.galley.text().trim().is_empty() => Some((t.visual_bounding_rect(), t.galley.text().to_string())),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    });
    widgets
        .into_iter()
        .filter(|(id, _)| !is_submenu(ctx, *id))
        .filter_map(|(id, rect)| {
            let mut parts: Vec<&(Rect, String)> = texts.iter().filter(|(r, _)| rect.contains(r.center())).collect();
            parts.sort_by(|a, b| a.0.left().total_cmp(&b.0.left()));
            let label = parts.first()?.1.trim().to_string();
            let keys = if parts.len() > 1 { parts.last().map(|p| p.1.trim().to_string()).unwrap_or_default() } else { String::new() };
            Some(Entry { id, label, keys })
        })
        .collect()
}

/// Whether the widget opens a submenu (egui keeps state for those).
fn is_submenu(ctx: &egui::Context, id: Id) -> bool {
    let state = SubMenu::id_from_widget_id(id).with(MenuState::ID);
    ctx.data(|d| d.get_temp::<MenuState>(state).is_some())
}

/// The entries matching `query`, best first: every word of the query must
/// appear in the label (any case); earlier matches rank higher, ties keep
/// the menu's order.
fn rank(entries: &[Entry], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut hits: Vec<(usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let label = e.label.to_lowercase();
            let mut first = usize::MAX;
            for w in &words {
                first = first.min(label.find(w.as_str())?);
            }
            Some((first, i))
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(labels: &[&str]) -> Vec<Entry> {
        labels.iter().enumerate().map(|(i, l)| Entry { id: Id::unique(i), label: l.to_string(), keys: String::new() }).collect()
    }

    #[test]
    fn rank_filters_and_orders_by_match_position() {
        let e = entries(&["Move to Trash", "Rename", "Copy File Path", "Paste Tags", "Copy Tags"]);
        // "Copy Tags" has "tag" earlier than "Paste Tags" does.
        assert_eq!(rank(&e, "tag"), vec![4, 3]);
        assert_eq!(rank(&e, "TRASH"), vec![0]);
        // "copy" starts both copy entries; menu order breaks the tie.
        assert_eq!(rank(&e, "copy"), vec![2, 4]);
        // Every word must match, in any order.
        assert_eq!(rank(&e, "tags copy"), vec![4]);
        assert!(rank(&e, "zzz").is_empty());
        assert_eq!(rank(&e, "  "), vec![0, 1, 2, 3, 4]);
    }

    /// Runs one frame of a stand-in menu with three buttons (the middle
    /// one disabled) and returns which of them reported a click.
    fn frame(ctx: &egui::Context, events: Vec<Event>) -> [bool; 3] {
        let mut clicked = [false; 3];
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            egui::Area::new(Id::unique("menu")).show(ui.ctx(), |ui| {
                show(ui, |ui| {
                    clicked[0] = ui.button("Copy Tags").clicked();
                    clicked[1] = ui.add_enabled(false, egui::Button::new("Paste Tags")).clicked();
                    clicked[2] = ui.add(egui::Button::new("Move to Trash").shortcut_text("⌘⌫")).clicked();
                });
            });
        });
        out.textures_delta.clear();
        clicked
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn typing_filters_and_enter_clicks_the_match() {
        let ctx = egui::Context::default();
        // A new area only measures itself in its first frame.
        assert_eq!(frame(&ctx, vec![]), [false; 3]);
        assert_eq!(frame(&ctx, vec![]), [false; 3]);
        let st: State = ctx.data(|d| d.get_temp(Id::unique("menu").with("menu-search"))).unwrap();
        // The disabled button is no choice; the shortcut text is kept apart.
        let labels: Vec<(&str, &str)> = st.entries.iter().map(|e| (e.label.as_str(), e.keys.as_str())).collect();
        assert_eq!(labels, [("Copy Tags", ""), ("Move to Trash", "⌘⌫")]);
        // The box has focus from the start, so typing goes into it.
        assert_eq!(frame(&ctx, vec![Event::Text("trash".into())]), [false; 3]);
        assert_eq!(frame(&ctx, vec![key(Key::Enter)]), [false, false, true]);
    }

    #[test]
    fn arrows_move_through_the_matches() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![]);
        frame(&ctx, vec![]);
        frame(&ctx, vec![Event::Text("t".into())]);
        // "t" matches both choices; the second one is a row down.
        assert_eq!(frame(&ctx, vec![key(Key::ArrowDown)]), [false; 3]);
        assert_eq!(frame(&ctx, vec![key(Key::Enter)]), [false, false, true]);
    }

    /// One frame of a button with a searchable context menu. Returns
    /// whether "Move to Trash" reported a click and whether the menu is open.
    fn context_frame(ctx: &egui::Context, events: Vec<Event>) -> (bool, bool) {
        let mut trash = false;
        let mut open = false;
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let resp = ui.button("Item");
            resp.context_menu(|ui| {
                show(ui, |ui| {
                    let _ = ui.button("Copy Tags");
                    trash = ui.button("Move to Trash").clicked();
                });
            });
            open = resp.context_menu_opened();
        });
        out.textures_delta.clear();
        (trash, open)
    }

    fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
    }

    #[test]
    fn works_inside_a_real_context_menu() {
        let ctx = egui::Context::default();
        context_frame(&ctx, vec![]);
        let at = egui::pos2(20.0, 15.0);
        context_frame(&ctx, vec![Event::PointerMoved(at), pointer(at, egui::PointerButton::Secondary, true)]);
        context_frame(&ctx, vec![pointer(at, egui::PointerButton::Secondary, false)]);
        for _ in 0..3 {
            assert_eq!(context_frame(&ctx, vec![]), (false, true));
        }
        // Clicking into the search box (which has focus) keeps the menu open.
        let field = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)).expect("the search box has focus").rect.center();
        context_frame(&ctx, vec![Event::PointerMoved(field), pointer(field, egui::PointerButton::Primary, true)]);
        assert_eq!(context_frame(&ctx, vec![pointer(field, egui::PointerButton::Primary, false)]), (false, true));
        assert_eq!(context_frame(&ctx, vec![]), (false, true));
        assert_eq!(context_frame(&ctx, vec![Event::Text("tra".into())]), (false, true));
        assert!(context_frame(&ctx, vec![key(Key::Enter)]).0);
        assert_eq!(context_frame(&ctx, vec![]), (false, false));
    }

    #[test]
    fn rank_prefers_earlier_matches() {
        let e = entries(&["Remove from Folder", "Folder Settings"]);
        assert_eq!(rank(&e, "folder"), vec![1, 0]);
    }
}
