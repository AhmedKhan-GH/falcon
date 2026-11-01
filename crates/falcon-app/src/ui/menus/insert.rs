//! The Insert menu of the batch rename dialogs (items, folders and smart
//! folders): the name tokens, with Number and Date submenus, behind a
//! type-to-filter search box as in Eagle's menus.

use eframe::egui::{self, RichText, Ui};

use crate::locale::{tr, tr_key};
use crate::theme as t;
use crate::ui::menus::search;

/// The Date submenu's sources, each with the letter of its tokens.
const DATE_SOURCES: [(&str, char); 3] = [(tr_key("Today"), 'D'), (tr_key("Modified"), 'M'), (tr_key("Created"), 'B')];

/// The Date submenu's forms, each with the token suffix it stands for.
const DATE_FORMS: [(&str, &str); 5] = [
    (tr_key("Date"), "1"),
    (tr_key("Month and day"), "2"),
    (tr_key("Written out"), "3"),
    (tr_key("Date and time"), "HM"),
    (tr_key("Date and time with seconds"), "HMS"),
];

/// The token a date form of a source inserts: `%D`, `%DD`, `%DDD`, `%DHM`…
fn date_token(letter: char, suffix: &str) -> String {
    match suffix {
        "1" => format!("%{letter}"),
        "2" => format!("%{letter}{letter}"),
        "3" => format!("%{letter}{letter}{letter}"),
        s => format!("%{letter}{s}"),
    }
}

/// The Insert button next to a rename template: each entry appends its
/// token to `template`. `folders` offers the item's folders token (items
/// only).
pub(crate) fn insert_menu(ui: &mut Ui, template: &mut String, folders: bool) {
    ui.menu_button(tr("Insert"), |ui| {
        search::show(ui, |ui| {
            ui.set_min_width(200.0);
            let mut pick = |ui: &mut Ui, label: &str, token: &str| {
                if ui.add(egui::Button::new(label).shortcut_text(token)).clicked() {
                    template.push_str(token);
                    ui.close();
                }
            };
            search::submenu(ui, tr("Number"), |ui| {
                for (label, token) in [("1", "%N"), ("01", "%NN"), ("001", "%NNN"), ("0001", "%NNNN")] {
                    pick(ui, label, token);
                }
            });
            search::submenu(ui, tr("Date"), |ui| {
                for (head, letter) in DATE_SOURCES {
                    let mut forms = |ui: &mut Ui| {
                        for (label, suffix) in DATE_FORMS {
                            pick(ui, tr(label), &date_token(letter, suffix));
                        }
                    };
                    if search::filtering(ui) {
                        // Found under "Date ▸ Modified", so the three
                        // sources' same-named forms tell apart.
                        search::submenu(ui, tr(head), forms);
                    } else {
                        ui.label(RichText::new(tr(head)).color(t::p().text_muted).size(12.0));
                        forms(ui);
                    }
                }
            });
            pick(ui, tr("Tags"), "%T");
            if folders {
                pick(ui, tr("Folders"), "%F");
            }
            pick(ui, tr("Original Name"), "*");
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_tokens_follow_the_rename_template_syntax() {
        assert_eq!(date_token('D', "1"), "%D");
        assert_eq!(date_token('M', "2"), "%MM");
        assert_eq!(date_token('B', "3"), "%BBB");
        assert_eq!(date_token('D', "HMS"), "%DHMS");
    }

    fn frame(ctx: &egui::Context, template: &mut String, events: Vec<egui::Event>) -> egui::Rect {
        let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            rect = ui.scope(|ui| insert_menu(ui, template, true)).response.rect;
        });
        out.textures_delta.clear();
        rect
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn the_search_finds_a_date_form_by_its_source() {
        let ctx = egui::Context::default();
        let mut template = String::from("x");
        let at = frame(&ctx, &mut template, vec![]).center();
        frame(&ctx, &mut template, vec![egui::Event::PointerMoved(at), press(at, true)]);
        frame(&ctx, &mut template, vec![press(at, false)]);
        for _ in 0..3 {
            frame(&ctx, &mut template, vec![]);
        }
        // The search box has focus once the menu is open.
        frame(&ctx, &mut template, vec![egui::Event::Text("modified seconds".into())]);
        frame(&ctx, &mut template, vec![]);
        let enter = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&ctx, &mut template, vec![enter]);
        frame(&ctx, &mut template, vec![]);
        assert_eq!(template, "x%MHMS");
    }
}
