//! Preferences ▸ AI Models: the providers the user adds, their keys, Verify,
//! the model lists and the default models. The interface itself is the
//! `falcon-ai` crate; this is the pane and the glue for the rest of the app.
//!
//! The settings are part of the preferences draft, so Cancel drops them.
//! Keys never enter the draft: a typed key waits in the pane, goes to the
//! Keychain on Save or Apply ([`Pane::commit`]) and is dropped on Cancel.
//! Verify runs on its own thread with the key as typed, or else as stored.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, RichText, Ui};
use falcon_ai::provider::host_of;
use falcon_ai::{AiError, BadOutput, BaseError, Client, Connection, Kind, KeyStore, ModelRef, ProviderId, Secret, Settings};

use crate::locale::{tr, tr_key, tr_n, trf};
use crate::prefs::{Page, note};
use crate::theme as t;

/// Where falcon keeps keys. Tests never reach the login Keychain.
pub fn keys() -> Arc<dyn KeyStore> {
    #[cfg(test)]
    return Arc::new(falcon_ai::MemoryKeys::default());
    #[cfg(not(test))]
    Arc::new(falcon_ai::Keychain::new(falcon_ai::Keychain::SERVICE))
}

/// The shared interface with these settings and falcon's keys.
pub fn client(settings: &Settings) -> Client {
    Client::new(settings.clone(), keys())
}

/// Who gets items under these settings: each default model's provider and
/// host, for the confirmation before a large AI run.
pub fn receivers(settings: &Settings) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in [settings.vision_model(), settings.chat_model()].into_iter().flatten() {
        let base = settings.base_url(m.provider).unwrap_or_default();
        let who = format!("{} ({})", m.provider.name(), host_of(&base));
        if !out.contains(&who) {
            out.push(who);
        }
    }
    out
}

/// What keeps AI steps from running, as a message; none when they can.
pub fn setup_problem(settings: &Settings) -> Option<&'static str> {
    if settings.providers.is_empty() {
        return Some(tr_key("Add a provider in Preferences ▸ AI Models to run AI steps"));
    }
    if settings.vision_model().is_none() {
        return Some(tr_key("Choose a default vision model in Preferences ▸ AI Models to run AI steps"));
    }
    None
}

/// An error from the interface as a sentence for the user.
pub fn reason(e: &AiError) -> String {
    match e {
        AiError::NotAdded(p) => trf("{provider} isn't added in Preferences ▸ AI Models", &[("provider", &p.name())]),
        AiError::BaseUrl(p, BaseError::Missing) => trf("{provider} needs a base URL", &[("provider", &p.name())]),
        AiError::BaseUrl(p, BaseError::Invalid) => trf("The base URL of {provider} isn't a plain http or https address", &[("provider", &p.name())]),
        AiError::PlainHttpKey(p) => trf("Falcon sends the {provider} key only over https, or over http to this Mac", &[("provider", &p.name())]),
        AiError::NoKey(p) => trf("{provider} needs an API key", &[("provider", &p.name())]),
        AiError::KeyStore(e) => trf("The Keychain refused: {e}", &[("e", e)]),
        AiError::Cancelled => tr("Cancelled").to_string(),
        AiError::Timeout => tr("The model took too long to answer").to_string(),
        AiError::Network(e) => trf("Could not reach the provider: {e}", &[("e", e)]),
        AiError::Http { status: 401 | 403, message } => trf("The provider refused the key: {message}", &[("message", message)]),
        AiError::Http { status, message } => trf("The provider answered {status}: {message}", &[("status", status), ("message", message)]),
        AiError::Provider(e) => trf("The provider reported an error: {e}", &[("e", e)]),
        AiError::Bad(BadOutput::Empty) => tr("The model's reply was empty").to_string(),
        AiError::Bad(BadOutput::Cut) => tr("The model's reply was cut off").to_string(),
        AiError::Bad(BadOutput::NotJson) => tr("The model's reply wasn't valid JSON").to_string(),
        AiError::Bad(BadOutput::Schema(e)) => trf("The model's reply had the wrong shape: {e}", &[("e", e)]),
    }
}

/// A Verify on its way.
struct Job {
    cancel: Arc<AtomicBool>,
    done: Receiver<Result<Vec<String>, AiError>>,
}

/// One provider's unsaved state.
#[derive(Default)]
struct Row {
    /// A key typed and not yet saved.
    typed: String,
    /// The stored key goes on Save or Apply.
    remove_key: bool,
    /// The Keychain holds a key.
    saved: bool,
    job: Option<Job>,
    /// What the last Verify said: the number of models, or why it failed.
    status: Option<Result<usize, String>>,
}

/// The pane's own state.
#[derive(Default)]
pub struct Pane {
    rows: HashMap<ProviderId, Row>,
    /// `saved` was read from the Keychain since the window opened.
    read_saved: bool,
    /// Why the last Save couldn't change a key.
    key_error: Option<String>,
}

impl Pane {
    /// The window opened or closed: unsaved keys go, Verify stops.
    pub fn reset(&mut self) {
        for row in self.rows.values() {
            if let Some(job) = &row.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.rows.clear();
        self.read_saved = false;
    }

    /// Save or Apply: typed keys go to the Keychain, removed ones leave it.
    pub fn commit(&mut self) {
        let store = keys();
        self.key_error = self.commit_to(store.as_ref()).err();
    }

    fn commit_to(&mut self, store: &dyn KeyStore) -> Result<(), String> {
        let mut first_error = None;
        for (&id, row) in &mut self.rows {
            let typed = row.typed.trim();
            let result = if !typed.is_empty() {
                store.set(id, &Secret::new(typed)).map(|()| true)
            } else if row.remove_key {
                store.delete(id).map(|()| false)
            } else {
                continue;
            };
            match result {
                Ok(saved) => {
                    row.saved = saved;
                    row.typed.clear();
                    row.remove_key = false;
                }
                Err(e) => first_error = first_error.or(Some(e)),
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Why the last Save couldn't change a key, once.
    pub fn take_key_error(&mut self) -> Option<String> {
        self.key_error.take()
    }

    /// The pane's two blocks.
    pub fn ui(&mut self, p: &mut Page, s: &mut Settings) {
        p.block(
            tr_key("Providers"),
            &["ai", "model", "provider", "key", "api", "openai", "anthropic", "gemini", "google", "deepseek", "tongyi", "qwen", "ollama", "lm studio", "base url", "verify", "keychain"],
            |ui| self.providers(ui, s),
        );
        p.block(tr_key("Default Models"), &["ai", "default", "chat", "vision", "image", "model"], |ui| defaults(ui, s));
    }

    fn providers(&mut self, ui: &mut Ui, s: &mut Settings) {
        note(ui, tr("Falcon sends nothing until you add a provider and press Verify or run an AI step. Keys are kept in the macOS Keychain."));
        ui.add_space(6.0);
        if !self.read_saved {
            self.read_saved = true;
            let store = keys();
            for p in &s.providers {
                self.rows.entry(p.id).or_default().saved = p.id.takes_key() && store.has(p.id).unwrap_or(false);
            }
        }
        self.finish_jobs(s);
        let mut removed = None;
        for id in s.providers.iter().map(|p| p.id).collect::<Vec<_>>() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.provider(ui, s, id) {
                    removed = Some(id);
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = removed {
            let row = self.rows.entry(id).or_default();
            if let Some(job) = row.job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            row.typed.clear();
            row.status = None;
            row.remove_key = row.saved;
            s.remove(id);
        }
        let left: Vec<ProviderId> = ProviderId::ALL.into_iter().filter(|p| s.provider(*p).is_none()).collect();
        ui.add_enabled_ui(!left.is_empty(), |ui| {
            ui.menu_button(format!("{}  {}", crate::icons::PLUS, tr("Add Provider")), |ui| {
                for id in left {
                    if ui.button(id.name()).clicked() {
                        s.add(id);
                        self.rows.entry(id).or_default().saved = id.takes_key() && keys().has(id).unwrap_or(false);
                        ui.close();
                    }
                }
            });
        });
    }

    /// One provider's box; returns whether Remove was pressed.
    fn provider(&mut self, ui: &mut Ui, s: &mut Settings, id: ProviderId) -> bool {
        let mut remove = false;
        let row = self.rows.entry(id).or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new(id.name()).strong());
            let kind = match id.kind() {
                Kind::Cloud => tr("Cloud"),
                Kind::Local => tr("Local"),
                Kind::Custom => tr("Custom"),
            };
            ui.label(RichText::new(kind).size(11.0).color(t::p().text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                remove = ui.small_button(tr("Remove")).clicked();
            });
        });
        egui::Grid::new(("ai-provider", id.id())).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            if id.takes_key() {
                ui.label(tr("API key"));
                ui.horizontal(|ui| {
                    let hint = if row.saved && !row.remove_key {
                        tr("Saved in the Keychain")
                    } else if id.needs_key() {
                        tr("Required")
                    } else {
                        tr("Optional")
                    };
                    ui.add(egui::TextEdit::singleline(&mut row.typed).password(true).hint_text(hint).desired_width(240.0));
                    if row.saved && !row.remove_key && ui.small_button(tr("Remove Key")).clicked() {
                        row.remove_key = true;
                    }
                });
                ui.end_row();
            }
            ui.label(tr("Base URL"));
            let hint = id.default_base().unwrap_or(tr("Required"));
            if let Some(settings) = s.providers.iter_mut().find(|p| p.id == id) {
                ui.add(egui::TextEdit::singleline(&mut settings.base_url).hint_text(hint).desired_width(300.0));
            }
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(job) = &row.job {
                ui.spinner();
                if ui.small_button(tr("Cancel")).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            } else if ui.button(tr("Verify")).clicked() {
                row.status = None;
                row.job = Some(start_verify(ui.ctx(), s, id, row));
            }
            match &row.status {
                Some(Ok(n)) => {
                    ui.label(RichText::new(tr_n(*n, "Verified. {n} model", "Verified. {n} models")).size(12.0).color(t::p().text_muted));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(t::p().icon_red));
                }
                None => {}
            }
        });
        let models = s.provider(id).map(|p| p.models.as_slice()).unwrap_or_default();
        if !models.is_empty() {
            egui::CollapsingHeader::new(tr_n(models.len(), "{n} model", "{n} models")).id_salt(("ai-models", id.id())).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt(("ai-model-list", id.id())).max_height(120.0).show(ui, |ui| {
                    for m in models {
                        ui.add(egui::Label::new(RichText::new(m).size(12.0)).selectable(true));
                    }
                });
            });
        }
        remove
    }

    /// Takes in the Verify results that have arrived.
    fn finish_jobs(&mut self, s: &mut Settings) {
        for (id, row) in &mut self.rows {
            let Some(job) = &row.job else { continue };
            let Ok(result) = job.done.try_recv() else { continue };
            row.job = None;
            row.status = match result {
                Ok(models) => {
                    let n = models.len();
                    if let Some(p) = s.providers.iter_mut().find(|p| p.id == *id) {
                        p.models = models;
                    }
                    Some(Ok(n))
                }
                Err(AiError::Cancelled) => None,
                Err(e) => Some(Err(reason(&e))),
            };
        }
    }
}

/// Verify: lists the provider's models off the UI thread, with the key as
/// typed, or else as stored.
fn start_verify(ctx: &egui::Context, s: &Settings, id: ProviderId, row: &Row) -> Job {
    let base = s.base_url(id).map_err(|e| AiError::BaseUrl(id, e));
    let typed = Some(row.typed.trim()).filter(|k| !k.is_empty()).map(Secret::new);
    let stored = !row.remove_key && id.takes_key();
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, done) = mpsc::channel();
    let flag = cancel.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let store = keys();
        let result = (|| {
            let key = match typed {
                Some(k) => Some(k),
                None if stored => store.get(id).map_err(AiError::KeyStore)?,
                None => None,
            };
            Connection::new(id, base?, key)?.list_models(&flag)
        })();
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    Job { cancel, done }
}

/// The default chat and vision model, each typed or picked from the lists.
fn defaults(ui: &mut Ui, s: &mut Settings) {
    let known = s.all_models();
    let added: Vec<ProviderId> = s.providers.iter().map(|p| p.id).collect();
    egui::Grid::new("ai-defaults").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label(tr("Chat"));
        model_field(ui, "ai-default-chat", &mut s.default_chat, &known, &added);
        ui.end_row();
        ui.label(tr("Vision"));
        model_field(ui, "ai-default-vision", &mut s.default_vision, &known, &added);
        ui.end_row();
    });
    note(ui, tr("AI steps in Actions use the vision model for items with a picture. Other items use the chat model, or the vision model when no chat model is set."));
}

fn model_field(ui: &mut Ui, salt: &str, value: &mut String, known: &[String], added: &[ProviderId]) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(value).id_salt(salt).hint_text("provider::model").desired_width(260.0));
            ui.add_enabled_ui(!known.is_empty(), |ui| {
                ui.menu_button(tr("Choose"), |ui| {
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for m in known {
                            if ui.selectable_label(value == m, m).clicked() {
                                value.clone_from(m);
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
        if let Some(problem) = model_problem(value, added) {
            ui.label(RichText::new(tr(problem)).size(11.0).color(t::p().icon_red));
        }
    });
}

/// What is wrong with a typed default model, if anything.
fn model_problem(name: &str, added: &[ProviderId]) -> Option<&'static str> {
    if name.trim().is_empty() {
        return None;
    }
    match ModelRef::parse(name) {
        None => Some(tr_key("Write it as provider::model")),
        Some(m) if !added.contains(&m.provider) => Some(tr_key("That provider isn't added")),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_ai::MemoryKeys;

    fn settings(default_vision: &str) -> Settings {
        let mut s = Settings { default_vision: default_vision.into(), ..Default::default() };
        s.add(ProviderId::Ollama);
        s
    }

    #[test]
    fn setup_problems_name_what_is_missing() {
        assert_eq!(setup_problem(&Settings::default()), Some("Add a provider in Preferences ▸ AI Models to run AI steps"));
        assert_eq!(setup_problem(&settings("")), Some("Choose a default vision model in Preferences ▸ AI Models to run AI steps"));
        assert_eq!(setup_problem(&settings("openai::x")), Some("Choose a default vision model in Preferences ▸ AI Models to run AI steps"), "its provider isn't added");
        assert_eq!(setup_problem(&settings("ollama::qwen3.8:27b")), None);
    }

    #[test]
    fn receivers_name_each_provider_and_host_once() {
        let mut s = settings("ollama::a");
        s.default_chat = "ollama::b".into();
        assert_eq!(receivers(&s), ["Ollama (localhost:11434)"]);
        s.add(ProviderId::Anthropic);
        s.default_chat = "anthropic::c".into();
        assert_eq!(receivers(&s), ["Ollama (localhost:11434)", "Anthropic (api.anthropic.com)"]);
    }

    #[test]
    fn typed_models_are_checked() {
        let added = [ProviderId::Ollama];
        assert_eq!(model_problem("", &added), None);
        assert_eq!(model_problem("ollama::qwen3.8:27b", &added), None);
        assert_eq!(model_problem("qwen3.8:27b", &added), Some("Write it as provider::model"));
        assert_eq!(model_problem("openai::x", &added), Some("That provider isn't added"));
    }

    #[test]
    fn keys_wait_for_save_and_leave_nothing_in_the_pane() {
        let store = MemoryKeys::default();
        let mut pane = Pane::default();
        pane.rows.entry(ProviderId::OpenAi).or_default().typed = " sk-1 ".into();
        assert!(!store.has(ProviderId::OpenAi).unwrap(), "nothing until Save");
        pane.commit_to(&store).unwrap();
        assert_eq!(store.get(ProviderId::OpenAi).unwrap(), Some(Secret::new("sk-1")));
        let row = &pane.rows[&ProviderId::OpenAi];
        assert!(row.saved && row.typed.is_empty());
        pane.rows.get_mut(&ProviderId::OpenAi).unwrap().remove_key = true;
        pane.commit_to(&store).unwrap();
        assert_eq!(store.get(ProviderId::OpenAi).unwrap(), None);
        assert!(!pane.rows[&ProviderId::OpenAi].saved);
    }

    #[test]
    fn cancel_drops_a_typed_key() {
        let store = MemoryKeys::default();
        let mut pane = Pane::default();
        pane.rows.entry(ProviderId::OpenAi).or_default().typed = "sk-1".into();
        pane.reset();
        pane.commit_to(&store).unwrap();
        assert!(!store.has(ProviderId::OpenAi).unwrap());
    }

    #[test]
    fn saved_preferences_hold_no_key() {
        let mut pane = Pane::default();
        pane.rows.entry(ProviderId::OpenAi).or_default().typed = "sk-typed-key".into();
        let mut prefs = crate::prefs::Prefs::default();
        prefs.ai_models.add(ProviderId::OpenAi);
        pane.commit_to(&MemoryKeys::default()).unwrap();
        assert!(!serde_json::to_string(&prefs).unwrap().contains("sk-typed-key"));
    }

    #[test]
    fn reasons_read_as_sentences_and_hold_no_key() {
        assert_eq!(reason(&AiError::Bad(BadOutput::NotJson)), "The model's reply wasn't valid JSON");
        assert_eq!(reason(&AiError::Timeout), "The model took too long to answer");
        assert_eq!(reason(&AiError::NoKey(ProviderId::Google)), "Google Gemini needs an API key");
        assert!(reason(&AiError::Http { status: 401, message: "no".into() }).starts_with("The provider refused the key"));
        assert!(reason(&AiError::Http { status: 500, message: "boom".into() }).contains("500"));
    }
}
