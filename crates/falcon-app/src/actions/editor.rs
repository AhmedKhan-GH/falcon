//! The action editor: name, shortcut and ordered steps of one action,
//! each step with its own tags, folders or rating.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, TextEdit, Ui};
use falcon_library::Library;
use falcon_library::actions::{ActionDef, Step};
use falcon_library::model::Folder;

use super::ActionsState;
use crate::app::App;
use crate::folder_picker::{self, Picker, Row};
use crate::icons as ic;
use crate::locale::{tr, tr_key, trf};
use crate::shortcuts::{self, Recorded, Shortcuts};
use crate::theme as t;

/// How long a refused shortcut's note stays, in seconds.
const CONFLICT_SECS: f64 = 1.5;

/// One step being edited, with the text typed into its tag field.
#[derive(Clone, Debug, PartialEq)]
pub struct EditStep {
    pub step: Step,
    pub input: String,
}

impl EditStep {
    pub fn new(step: Step) -> Self {
        EditStep { step, input: String::new() }
    }
}

/// An action as the editor holds it until Save.
#[derive(Clone, Debug)]
pub struct Editor {
    pub id: String,
    pub is_new: bool,
    pub name: String,
    pub shortcut: Option<KeyboardShortcut>,
    pub steps: Vec<EditStep>,
    /// The shortcut button waits for a key press.
    pub recording: bool,
    /// A refused chord: what uses it, and when.
    pub conflict: Option<(String, f64)>,
    /// The folder picker open for a step.
    pub folder_pick: Option<(usize, Picker)>,
    /// Why the last save failed.
    pub error: Option<String>,
}

/// The name of a step kind.
pub fn kind_label(step: &Step) -> &'static str {
    match step {
        Step::AddTags { .. } => tr_key("Add tags"),
        Step::RemoveTags { .. } => tr_key("Remove tags"),
        Step::ClearTags => tr_key("Clear all tags"),
        Step::AddFolders { .. } => tr_key("Add to folders"),
        Step::RemoveFolders { .. } => tr_key("Remove from folders"),
        Step::ClearFolders => tr_key("Remove from all folders"),
        Step::Rating { .. } => tr_key("Set rating"),
    }
}

impl Editor {
    /// A new action with one empty Add tags step.
    pub fn new_action() -> Self {
        Editor {
            id: falcon_library::ids::new_id(),
            is_new: true,
            name: String::new(),
            shortcut: None,
            steps: vec![EditStep::new(Step::AddTags { tags: Vec::new() })],
            recording: false,
            conflict: None,
            folder_pick: None,
            error: None,
        }
    }

    pub fn edit(def: &ActionDef) -> Self {
        Editor {
            id: def.id.clone(),
            is_new: false,
            name: def.name.clone(),
            shortcut: super::chord_of(def),
            steps: def.steps.iter().cloned().map(EditStep::new).collect(),
            recording: false,
            conflict: None,
            folder_pick: None,
            error: None,
        }
    }

    /// The action to save, with tags still in the fields taken in.
    pub fn to_def(&self) -> ActionDef {
        let steps = self
            .steps
            .iter()
            .map(|s| {
                let mut step = s.step.clone();
                if let Step::AddTags { tags } | Step::RemoveTags { tags } = &mut step {
                    add_tags(tags, &s.input);
                }
                step
            })
            .collect();
        ActionDef { id: self.id.clone(), name: self.name.trim().to_string(), shortcut: self.shortcut.map(|c| shortcuts::format(&c)), steps, runnable: true }
    }

    /// What keeps Save disabled, if anything.
    pub fn problem(&self) -> Option<&'static str> {
        let def = self.to_def();
        if def.name.is_empty() {
            return Some(tr_key("Name the action"));
        }
        if def.steps.is_empty() {
            return Some(tr_key("Add a step"));
        }
        for s in &def.steps {
            match s {
                Step::AddTags { tags } | Step::RemoveTags { tags } if tags.is_empty() => return Some(tr_key("Every tag step needs a tag")),
                Step::AddFolders { folders } | Step::RemoveFolders { folders } if folders.is_empty() => return Some(tr_key("Every folder step needs a folder")),
                _ => {}
            }
        }
        None
    }

    /// Moves a step from `from` to `to` (an index after its removal).
    pub fn move_step(&mut self, from: usize, to: usize) {
        if from < self.steps.len() {
            let s = self.steps.remove(from);
            self.steps.insert(to.min(self.steps.len()), s);
            self.folder_pick = None;
        }
    }

    /// Changes a step's kind, keeping tags or folders when both kinds have them.
    pub fn set_kind(&mut self, i: usize, kind: &Step) {
        let Some(s) = self.steps.get_mut(i) else { return };
        if s.step.same_kind(kind) {
            return;
        }
        let tags = match &s.step {
            Step::AddTags { tags } | Step::RemoveTags { tags } => tags.clone(),
            _ => Vec::new(),
        };
        let folders = match &s.step {
            Step::AddFolders { folders } | Step::RemoveFolders { folders } => folders.clone(),
            _ => Vec::new(),
        };
        s.step = match kind {
            Step::AddTags { .. } => Step::AddTags { tags },
            Step::RemoveTags { .. } => Step::RemoveTags { tags },
            Step::AddFolders { .. } => Step::AddFolders { folders },
            Step::RemoveFolders { .. } => Step::RemoveFolders { folders },
            other => other.clone(),
        };
        self.folder_pick = None;
    }

    /// One key press while the shortcut button records, under Eagle's
    /// recorder rules: Esc stops, Backspace clears, and a chord another
    /// action or a command uses is refused.
    pub fn feed(&mut self, key: Key, m: Modifiers, actions: &ActionsState, keys: &Shortcuts, now: f64) {
        match shortcuts::record(key, m) {
            Recorded::Cancel => self.recording = false,
            Recorded::Clear => {
                self.shortcut = None;
                self.recording = false;
            }
            Recorded::Ignore => {}
            Recorded::Chord(c) => match actions.conflict(c, &self.id, keys) {
                Some(user) => self.conflict = Some((user, now)),
                None => {
                    self.shortcut = Some(c);
                    self.recording = false;
                    self.conflict = None;
                }
            },
        }
    }
}

/// Appends the tags typed in `text` (comma separated) that `tags` lacks.
fn add_tags(tags: &mut Vec<String>, text: &str) {
    for t in crate::tags::parse_tags(text) {
        if let Some(t) = falcon_library::tagging::clean(&t)
            && !tags.contains(&t)
        {
            tags.push(t);
        }
    }
}

/// Known tags containing `typed`, at most `n`, leaving out `have`.
fn tag_matches(known: &[(String, usize)], typed: &str, have: &[String], n: usize) -> Vec<String> {
    let typed = crate::tags::fold(typed.trim());
    if typed.is_empty() {
        return Vec::new();
    }
    known.iter().filter(|(t, _)| !have.contains(t) && crate::tags::fold(t).contains(&typed)).take(n).map(|(t, _)| t.clone()).collect()
}

fn folder_name(roots: &[Folder], id: &str) -> String {
    crate::logic::find_folder(roots, id).map_or_else(|| tr("Missing folder").to_string(), |f| f.name.clone())
}

impl App {
    /// The editor dialog, while one is open.
    pub(crate) fn action_editor_ui(&mut self, ctx: &egui::Context, lib: &Library) {
        let Some(mut ed) = self.user_actions.editor.take() else { return };
        let now = ctx.input(|i| i.time);
        if ed.recording {
            let presses = ctx.input_mut(|i| {
                let p: Vec<_> = i.events.iter().filter_map(shortcuts::press).collect();
                i.events.retain(|e| shortcuts::press(e).is_none());
                p
            });
            for (key, m) in presses {
                ed.feed(key, m, &self.user_actions, &self.shortcuts, now);
            }
        }
        if ed.conflict.as_ref().is_some_and(|c| now - c.1 > CONFLICT_SECS) {
            ed.conflict = None;
        }
        if ed.conflict.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let counts = crate::logic::tag_counts(lib.items());
        let roots = &lib.metadata.folders;
        let mut keep = true;
        let mut save = false;
        let modal = egui::Modal::new(egui::Id::unique("action-editor")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(if ed.is_new { tr("New Action") } else { tr("Edit Action") }).size(16.0));
            ui.add_space(8.0);
            egui::Grid::new("action-editor-head").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Name"));
                ui.add(TextEdit::singleline(&mut ed.name).hint_text(tr("Name")).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label(tr("Shortcut"));
                ui.horizontal(|ui| self.shortcut_button(ui, &mut ed));
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.label(RichText::new(tr("Steps")).strong());
            ui.label(RichText::new(tr("Run in order on every selected item.")).color(t::p().text_muted).size(12.0));
            ui.add_space(4.0);
            let max_h = (ctx.content_rect().height() - 300.0).max(160.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                let mut moved = None;
                let mut removed = None;
                let n = ed.steps.len();
                for i in 0..n {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}.", i + 1)).color(t::p().text_muted));
                            let current = ed.steps[i].step.clone();
                            egui::ComboBox::from_id_salt(("action-step-kind", i)).selected_text(tr(kind_label(&current))).width(200.0).show_ui(ui, |ui| {
                                for k in Step::kinds() {
                                    if ui.selectable_label(current.same_kind(&k), tr(kind_label(&k))).clicked() {
                                        ed.set_kind(i, &k);
                                    }
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(ic::X).on_hover_text(tr("Remove step")).clicked() {
                                    removed = Some(i);
                                }
                                if ui.add_enabled(i + 1 < n, egui::Button::new(ic::CARET_DOWN).small()).on_hover_text(tr("Move down")).clicked() {
                                    moved = Some((i, i + 1));
                                }
                                if ui.add_enabled(i > 0, egui::Button::new(ic::CARET_UP).small()).on_hover_text(tr("Move up")).clicked() {
                                    moved = Some((i, i - 1));
                                }
                            });
                        });
                        self.step_params(ui, &mut ed, i, &counts, roots);
                    });
                    ui.add_space(4.0);
                }
                if let Some((from, to)) = moved {
                    ed.move_step(from, to);
                }
                if let Some(i) = removed {
                    ed.steps.remove(i);
                    ed.folder_pick = None;
                }
            });
            ui.menu_button(format!("{}  {}", ic::PLUS, tr("Add Step")), |ui| {
                for k in Step::kinds() {
                    if ui.button(tr(kind_label(&k))).clicked() {
                        ed.steps.push(EditStep::new(k));
                        ui.close();
                    }
                }
            });
            ui.add_space(10.0);
            let problem = ed.problem();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(problem.is_none(), egui::Button::new(tr("Save"))).clicked() {
                    save = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    keep = false;
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if let Some(e) = &ed.error {
                        ui.label(RichText::new(e).color(t::p().icon_red));
                    } else if let Some(p) = problem {
                        ui.label(RichText::new(tr(p)).color(t::p().text_muted));
                    }
                });
            });
        });
        if modal.should_close() && !ed.recording && ed.folder_pick.is_none() {
            keep = false;
        }
        if save {
            let def = ed.to_def();
            match self.user_actions.commit(|f| f.put(&def)) {
                Ok(()) => keep = false,
                Err(e) => ed.error = Some(trf("Could not save: {e}", &[("e", &e)])),
            }
        }
        if keep {
            self.user_actions.editor = Some(ed);
        }
    }

    fn shortcut_button(&self, ui: &mut Ui, ed: &mut Editor) {
        let text = match (ed.recording, ed.shortcut) {
            (true, _) => RichText::new(tr("Type a shortcut")).color(t::p().text_muted),
            (false, Some(c)) => RichText::new(shortcuts::display(ui.ctx(), &c)),
            (false, None) => RichText::new(tr("None")).color(t::p().text_disabled),
        };
        let b = ui.add(egui::Button::new(text).selected(ed.recording).min_size(egui::vec2(150.0, 24.0)));
        if b.clicked() {
            ed.recording = !ed.recording;
            ed.conflict = None;
        }
        if ed.shortcut.is_some() && !ed.recording && ui.small_button(ic::X).on_hover_text(tr("Clear the shortcut")).clicked() {
            ed.shortcut = None;
        }
        if let Some((user, _)) = &ed.conflict {
            ui.label(RichText::new(trf("Used by {command}", &[("command", user)])).color(egui::Color32::from_rgb(0xff, 0xb0, 0x40)).size(11.5));
        } else if ed.recording {
            ui.label(RichText::new(tr("Esc stops, Backspace clears")).color(t::p().text_muted).size(11.5));
        }
    }

    fn step_params(&self, ui: &mut Ui, ed: &mut Editor, i: usize, counts: &[(String, usize)], roots: &[Folder]) {
        let picking = ed.folder_pick.as_ref().is_some_and(|(at, _)| *at == i);
        let s = &mut ed.steps[i];
        match &mut s.step {
            Step::AddTags { tags } | Step::RemoveTags { tags } => {
                let mut gone = None;
                ui.horizontal_wrapped(|ui| {
                    for (n, tag) in tags.iter().enumerate() {
                        if ui.add(egui::Button::new(format!("{tag}  {}", ic::X)).small()).on_hover_text(tr("Remove")).clicked() {
                            gone = Some(n);
                        }
                    }
                });
                if let Some(n) = gone {
                    tags.remove(n);
                }
                let field = ui.add(TextEdit::singleline(&mut s.input).id_salt(("action-tags", i)).hint_text(tr("Type tags, separated by commas")).desired_width(f32::INFINITY));
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if entered || s.input.ends_with(',') {
                    add_tags(tags, &s.input);
                    s.input.clear();
                    if entered {
                        field.request_focus();
                    }
                }
                let hits = tag_matches(counts, &s.input, tags, 8);
                if !hits.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for h in hits {
                            if ui.add(egui::Button::new(&h).small().fill(t::p().button)).clicked() {
                                tags.push(h);
                                s.input.clear();
                            }
                        }
                    });
                }
            }
            Step::AddFolders { folders } | Step::RemoveFolders { folders } => {
                let mut gone = None;
                ui.horizontal_wrapped(|ui| {
                    for (n, id) in folders.iter().enumerate() {
                        let label = format!("{} {}  {}", ic::FOLDER, folder_name(roots, id), ic::X);
                        if ui.add(egui::Button::new(label).small()).on_hover_text(tr("Remove")).clicked() {
                            gone = Some(n);
                        }
                    }
                    if !picking && ui.small_button(tr("Choose Folders…")).clicked() {
                        ed.folder_pick = Some((i, Picker::new(Vec::new(), None, folders.clone(), roots)));
                    }
                });
                if let Some(n) = gone {
                    folders.remove(n);
                }
                if picking && let Some((_, p)) = ed.folder_pick.as_mut() {
                    let mut done = false;
                    egui::Frame::NONE.fill(t::p().button).corner_radius(t::RADIUS).inner_margin(6.0).show(ui, |ui| {
                        ui.add(TextEdit::singleline(&mut p.search).id_salt(("action-folder-search", i)).hint_text(tr("Search folders")).desired_width(f32::INFINITY));
                        egui::ScrollArea::vertical().id_salt(("action-folder-rows", i)).max_height(200.0).show(ui, |ui| {
                            for row in folder_picker::rows(p, roots, &[], |_| false) {
                                let Row::Folder { id, depth, kids } = row else { continue };
                                ui.horizontal(|ui| {
                                    ui.add_space(depth as f32 * 14.0);
                                    if kids {
                                        let open = p.expanded.contains(&id);
                                        if ui.small_button(if open { ic::CARET_DOWN } else { ic::CARET_RIGHT }).clicked() {
                                            if open {
                                                p.expanded.remove(&id);
                                            } else {
                                                p.expanded.insert(id.clone());
                                            }
                                        }
                                    }
                                    let mut on = p.checked.contains(&id);
                                    if ui.checkbox(&mut on, folder_name(roots, &id)).changed() {
                                        p.toggle(&id);
                                    }
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button(tr("Done")).clicked() {
                                done = true;
                            }
                        });
                    });
                    if done {
                        let (add, _) = Picker { existing: Vec::new(), ..p.clone() }.changes(roots);
                        // Folders no longer in the library stay until removed by hand.
                        let missing: Vec<String> = folders.iter().filter(|id| crate::logic::find_folder(roots, id).is_none()).cloned().collect();
                        *folders = missing.into_iter().chain(add).collect();
                        ed.folder_pick = None;
                    }
                }
            }
            Step::Rating { rating } => {
                // The inspector's rating row; 0 stars clears the rating.
                if let Some(n) = crate::ui::widgets::rating(ui, u64::from(*rating)) {
                    *rating = n as u8;
                }
            }
            Step::ClearTags | Step::ClearFolders => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn typed_tags_count_on_save() {
        let mut ed = Editor::new_action();
        ed.name = "  Tidy ".into();
        assert_eq!(ed.problem(), Some("Every tag step needs a tag"));
        ed.steps[0].input = "a, b ,a".into();
        assert_eq!(ed.problem(), None);
        let def = ed.to_def();
        assert_eq!(def.name, "Tidy");
        assert_eq!(def.steps, [Step::AddTags { tags: s(&["a", "b"]) }]);
        assert!(def.runnable && def.shortcut.is_none());
    }

    #[test]
    fn save_needs_a_name_and_complete_steps() {
        let mut ed = Editor::new_action();
        assert_eq!(ed.problem(), Some("Name the action"));
        ed.name = "X".into();
        ed.steps.clear();
        assert_eq!(ed.problem(), Some("Add a step"));
        ed.steps.push(EditStep::new(Step::AddFolders { folders: Vec::new() }));
        assert_eq!(ed.problem(), Some("Every folder step needs a folder"));
        ed.steps[0].step = Step::AddFolders { folders: s(&["F"]) };
        ed.steps.push(EditStep::new(Step::ClearTags));
        assert_eq!(ed.problem(), None);
    }

    #[test]
    fn steps_reorder_and_change_kind() {
        let mut ed = Editor::new_action();
        ed.steps = vec![EditStep::new(Step::AddTags { tags: s(&["t"]) }), EditStep::new(Step::Rating { rating: 2 }), EditStep::new(Step::ClearFolders)];
        ed.move_step(2, 0);
        assert_eq!(ed.steps.iter().map(|s| s.step.clone()).collect::<Vec<_>>(), [Step::ClearFolders, Step::AddTags { tags: s(&["t"]) }, Step::Rating { rating: 2 }]);
        ed.set_kind(1, &Step::RemoveTags { tags: Vec::new() });
        assert_eq!(ed.steps[1].step, Step::RemoveTags { tags: s(&["t"]) }, "tags carry over");
        ed.set_kind(1, &Step::Rating { rating: 0 });
        assert_eq!(ed.steps[1].step, Step::Rating { rating: 0 });
    }

    #[test]
    fn recording_refuses_taken_keys() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("actions.config.json"), r#"[{"id":"A","name":"Other","shortcut":"Command + Shift + 1","steps":[]}]"#).unwrap();
        let mut state = ActionsState::default();
        state.follow(Some(d.path()), 0.0);
        let keys = Shortcuts::default();
        let mut ed = Editor::new_action();
        ed.recording = true;
        let cmd_shift = Modifiers { shift: true, ..Modifiers::COMMAND };
        ed.feed(Key::Num1, cmd_shift, &state, &keys, 1.0);
        assert_eq!(ed.conflict.as_ref().map(|c| c.0.as_str()), Some("Other"));
        assert!(ed.recording && ed.shortcut.is_none());
        ed.feed(Key::T, Modifiers::COMMAND, &state, &keys, 1.2);
        assert!(ed.shortcut.is_none(), "⌘T adds tags");
        ed.feed(Key::Q, Modifiers::NONE, &state, &keys, 1.3);
        assert!(ed.recording, "a bare letter waits for more");
        ed.feed(Key::Num2, cmd_shift, &state, &keys, 1.4);
        assert!(!ed.recording);
        assert_eq!(ed.to_def().shortcut.as_deref(), Some("Command + Shift + 2"));
        ed.recording = true;
        ed.feed(Key::Backspace, Modifiers::NONE, &state, &keys, 2.0);
        assert!(ed.shortcut.is_none() && !ed.recording);
    }

    #[test]
    fn tag_suggestions_match_known_tags() {
        let known = vec![("Reference".to_string(), 3), ("ref sheet".to_string(), 1), ("other".to_string(), 9)];
        assert_eq!(tag_matches(&known, "ref", &[], 8), s(&["Reference", "ref sheet"]));
        assert_eq!(tag_matches(&known, "ref", &s(&["Reference"]), 8), s(&["ref sheet"]));
        assert!(tag_matches(&known, " ", &[], 8).is_empty());
    }
}
