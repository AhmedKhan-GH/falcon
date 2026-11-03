//! Background runs in real workspaces on temp libraries, against the mock
//! server of `ai/tests.rs`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use falcon_ai::{Client, Limits, MemoryKeys, ProviderId, Settings};
use falcon_library::Library;
use falcon_library::actions::Step;
use falcon_library::undo::WriteMode;
use serde_json::json;

use super::super::tests::{Reply, library, notes, plan_with, run, says, serve, tags};
use super::*;
use crate::app::state::EditBuf;
use crate::ui::dialogs::force_close::{self, stop_tasks, tasks_running};
use crate::ui::dialogs::quit_confirm::{MenuQuit, menu_quit};
use crate::workspaces::Parked;

const TAG: &str = r#"{"tags":["t"]}"#;

fn slow(text: &str) -> Reply {
    Reply::After(Duration::from_millis(150), text.to_string())
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn start(ws: &mut Workspace, ctx: &egui::Context, ids: &[String], steps: Vec<Step>, base: &str) {
    let shared = ws.lib.clone().unwrap();
    // Generous limits: these runs are ended by replies or Cancel, not by timeouts.
    let limits = Limits { connect: Duration::from_secs(5), first_byte: Duration::from_secs(10), idle: Duration::from_secs(10), total: Duration::from_secs(20) };
    ws.ai_run = Some(BackgroundRun::start(ctx, shared, run(ids, steps), plan_with(base, limits)));
}

fn wait_done(ws: &Workspace) {
    wait_for("the run to end", || !tasks_running(ws));
}

#[test]
fn a_run_counts_as_a_task_and_ends_with_its_undo_step_in_the_history() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![slow(TAG)]);
    assert!(!tasks_running(&ws));
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    assert!(tasks_running(&ws), "the switch warning, close and quit see it");
    assert!(!ws.op.busy(), "there is no modal to hold the window");
    assert!(settle(&mut ws, &Place::Shown).is_none(), "nothing to report yet");
    wait_done(&ws);
    assert_eq!(tags(&shared, &ids[0]), ["t"]);
    let ended = settle(&mut ws, &Place::Shown).expect("reported once");
    assert!(ended.undo_toast && ended.alert.is_none());
    assert_eq!(ended.message, "Label changed 1 item");
    assert_eq!((ws.undo.len(), ws.undo[0].label.as_str()), (1, "Run Label"));
    assert!(ws.ai_run.is_none());
    assert!(settle(&mut ws, &Place::Shown).is_none());
}

#[test]
fn the_library_stays_usable_while_a_run_goes_and_rule_actions_are_their_own_step() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a", "b"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![slow(TAG), slow(TAG)]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    // A rule-only action runs at once, as a frame's edit would.
    {
        let mut lib = lock(&shared);
        lib.begin_transaction("Rule");
        lib.run_steps(&ids, &[Step::Rating { rating: 2 }]).unwrap();
        let tx = lib.end_transaction().unwrap();
        ws.undo.push(tx);
    }
    assert_eq!(tags(&shared, &ids[0]), Vec::<String>::new(), "nothing is written before the end");
    wait_done(&ws);
    settle(&mut ws, &Place::Shown).unwrap();
    assert_eq!(ws.undo.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Rule", "Run Label"]);
    let item = lock(&shared).item(&ids[1]).unwrap().clone();
    assert_eq!((item.tags, item.extra.get("star")), (vec!["t".to_string()], Some(&json!(2))));
    // Undo takes back the run and leaves the rating set during it.
    let tx = ws.undo.pop().unwrap();
    lock(&shared).revert(tx).unwrap();
    let item = lock(&shared).item(&ids[1]).unwrap().clone();
    assert_eq!((item.tags, item.extra.get("star")), (Vec::<String>::new(), Some(&json!(2))));
}

#[test]
fn cancel_stops_it_and_keeps_what_finished() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a", "b", "c"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![says(TAG), Reply::Stalls, says(TAG)]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    wait_for("the second item", || ws.ai_run.as_ref().unwrap().counts().0 == 1);
    ws.ai_run.as_ref().unwrap().cancel();
    assert!(ws.ai_run.as_ref().unwrap().cancelling());
    wait_done(&ws);
    assert_eq!(tags(&shared, &ids[0]), ["t"]);
    assert!(tags(&shared, &ids[1]).is_empty() && tags(&shared, &ids[2]).is_empty());
    let ended = settle(&mut ws, &Place::Shown).unwrap();
    let a = ended.alert.expect("a stopped run has a report");
    assert!(a.body.contains("Cancel stopped the run before 2 items."), "{}", a.body);
    assert_eq!(ws.undo.len(), 1);
}

#[test]
fn the_panels_cancel_button_stops_the_run() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a"]);
    let mut ws = Workspace::new(&ctx, Some(shared));
    let (base, _) = serve(vec![Reply::Stalls]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0));
    let run = ws.ai_run.as_ref().unwrap();
    let mut button = None;
    let mut frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        ctx.run_ui(input, |ui| button = draw_panel(ui.ctx(), run, "Running Label")).drop_without_applying_deltas();
        button
    };
    frame(vec![]);
    let at = frame(vec![]).expect("Cancel shows while the run goes");
    assert!(screen.contains_rect(at), "on the screen: {at:?}");
    assert!(at.center().x > 700.0 && at.center().y > 600.0, "in the bottom right corner: {at:?}");
    let press = |pressed| egui::Event::PointerButton { pos: at.center(), button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(vec![egui::Event::PointerMoved(at.center())]);
    frame(vec![press(true)]);
    frame(vec![press(false)]);
    assert!(run.cancelling());
    assert_eq!(frame(vec![]), None, "it says it is stopping instead");
    wait_done(&ws);
}

#[test]
fn switching_away_leaves_the_run_on_its_own_library() {
    let ctx = egui::Context::default();
    let (_a, shared_a, ids_a) = library(&["a"]);
    let (_b, shared_b, ids_b) = library(&["b"]);
    let mut shown = Workspace::new(&ctx, Some(shared_a.clone()));
    let (base, _) = serve(vec![slow(TAG)]);
    start(&mut shown, &ctx, &ids_a, vec![Step::AutoTag], &base);
    // The switch: A goes into the background, B is shown.
    let mut others: Parked<Workspace> = Parked::default();
    others.push(&mut shown, Workspace::new(&ctx, Some(shared_b.clone())));
    assert!(lock(shown.lib.as_ref().unwrap()).item(&ids_b[0]).is_some());
    assert!(!tasks_running(&shown));
    assert!(others.iter(&shown).any(tasks_running), "quit still sees it");
    wait_for("the run to end", || !others.iter(&shown).any(tasks_running));
    assert_eq!(tags(&shared_a, &ids_a[0]), ["t"]);
    assert!(tags(&shared_b, &ids_b[0]).is_empty());
    let parked = others.parked_mut().next().unwrap();
    let place = Place::Parked(library_name(parked));
    let ended = settle(parked, &place).unwrap();
    assert!(!ended.undo_toast, "its Undo would act on the shown library");
    assert_eq!(ended.message, "T: Label changed 1 item");
    assert_eq!(parked.undo.len(), 1, "the undo step is in its own library's history");
    assert!(shown.undo.is_empty());
}

#[test]
fn closing_stops_the_run_writes_what_finished_and_reports_without_undo() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a", "b", "c"]);
    lock(&shared).set_write_mode(WriteMode::Batched).unwrap();
    let root = lock(&shared).root().to_path_buf();
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![says(TAG), Reply::Stalls, says(TAG)]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    wait_for("the second item", || ws.ai_run.as_ref().unwrap().counts().0 == 1);
    // The steps `force_close_dialog` goes through.
    assert_eq!(force_close::step(false, tasks_running(&ws)), force_close::Step::Ask);
    stop_tasks(&ws);
    wait_for("the worker to stop", || force_close::step(true, tasks_running(&ws)) == force_close::Step::Close);
    let ended = settle_closing(&mut ws).expect("the report outlives the library");
    assert!(ended.undo.is_none() && !ended.undo_toast);
    let a = ended.alert.unwrap();
    assert!(a.body.contains("Library: T") && a.body.contains("Cancel stopped") && !a.body.contains("Undo"), "{}", a.body);
    // The worker wrote it out itself: no frame flushes a library in the background.
    let back = Library::open(&root).unwrap();
    assert_eq!(back.item(&ids[0]).unwrap().tags, ["t"]);
    assert!(back.item(&ids[1]).unwrap().tags.is_empty());
}

#[test]
fn a_close_that_could_not_wait_stops_the_run() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a", "b"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![Reply::Stalls, Reply::Stalls]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    assert!(settle_closing(&mut ws).is_none());
    assert!(ws.ai_run.as_ref().unwrap().cancelling());
    wait_done(&ws);
}

#[test]
fn quitting_asks_stops_every_run_and_then_goes() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let (base, _) = serve(vec![Reply::Stalls]);
    start(&mut ws, &ctx, &ids, vec![Step::AutoTag], &base);
    assert_eq!(menu_quit(tasks_running(&ws)), MenuQuit::Ask);
    stop_tasks(&ws);
    wait_done(&ws);
    assert_eq!(menu_quit(tasks_running(&ws)), MenuQuit::Now);
    assert_eq!(force_close::step(true, tasks_running(&ws)), force_close::Step::Close);
}

#[test]
fn a_second_run_is_refused_naming_the_first_and_its_library() {
    let ctx = egui::Context::default();
    let (_d, shared, _) = library(&[]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let other = Workspace::new(&ctx, None);
    assert_eq!(second_run_message([&other, &ws].into_iter()), None);
    let (base, _) = serve(vec![]);
    start(&mut ws, &ctx, &[], vec![Step::AutoTag], &base);
    let message = second_run_message([&other, &ws].into_iter()).expect("refused");
    assert_eq!(message, "Label is still running in T. Wait for it to finish or cancel it before starting another AI run.");
    // Until its report is out, so a report can't be overtaken.
    wait_done(&ws);
    assert!(second_run_message([&other, &ws].into_iter()).is_some());
    settle(&mut ws, &Place::Shown).unwrap();
    assert_eq!(second_run_message([&other, &ws].into_iter()), None);
}

#[test]
fn a_crashed_worker_ends_the_run_with_an_error() {
    let ctx = egui::Context::default();
    let (_d, shared, _) = library(&[]);
    let mut ws = Workspace::new(&ctx, Some(shared));
    let live = Arc::new(Mutex::new(Live { finished: Some(Finished::Crashed), ..Default::default() }));
    ws.ai_run = Some(BackgroundRun { name: "Label".into(), total: 1, started: Instant::now(), live, cancel: Default::default() });
    assert!(!tasks_running(&ws));
    let ended = settle(&mut ws, &Place::Shown).unwrap();
    assert_eq!(ended.error.as_deref(), Some("Label stopped on an internal error."));
}

/// A transaction that sets `text` as the notes of `id`.
fn notes_edit(shared: &crate::app::Shared, id: &str, text: &str) -> Transaction {
    let mut lib = lock(shared);
    lib.begin_transaction("Run Label");
    let mut item = lib.item(id).unwrap().clone();
    item.annotation = text.into();
    lib.update_item(item).unwrap();
    lib.end_transaction().unwrap()
}

fn buffer(id: &str, notes: &str) -> EditBuf {
    EditBuf { id: id.into(), name: "typed name".into(), notes: notes.into(), url: "typed url".into() }
}

#[test]
fn the_inspector_follows_the_run_but_keeps_what_was_typed() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["a", "b"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    // Left alone: it follows, or its stale text would overwrite the notes on blur.
    ws.edit = Some(buffer(&ids[0], ""));
    let tx = notes_edit(&shared, &ids[0], "From the model.");
    follow_notes(&mut ws, &tx);
    let e = ws.edit.as_ref().unwrap();
    assert_eq!((e.notes.as_str(), e.name.as_str(), e.url.as_str()), ("From the model.", "typed name", "typed url"));
    // Typing under way: it stays.
    ws.edit = Some(buffer(&ids[1], "I am typing"));
    let tx = notes_edit(&shared, &ids[1], "From the model.");
    follow_notes(&mut ws, &tx);
    assert_eq!(ws.edit.as_ref().unwrap().notes, "I am typing");
    // Another item's buffer is none of the run's business.
    ws.edit = Some(buffer(&ids[1], "I am typing"));
    let tx = notes_edit(&shared, &ids[0], "Again.");
    follow_notes(&mut ws, &tx);
    assert_eq!(ws.edit.as_ref().unwrap().notes, "I am typing");
    assert_eq!(notes(&shared, &ids[0]), "Again.");
}

/// The whole background path against the real Ollama with the model the user
/// chose, renaming the item while the model is working.
/// Run with `cargo test -p falcon-app real_ollama -- --ignored --nocapture`.
#[test]
#[ignore = "needs Ollama with qwen3.8:27b"]
fn real_ollama_runs_in_the_background_while_the_item_is_edited() {
    let ctx = egui::Context::default();
    let (_d, shared, ids) = library(&["swatch"]);
    let mut ws = Workspace::new(&ctx, Some(shared.clone()));
    let mut s = Settings { default_vision: "ollama::qwen3.8:27b".into(), ..Default::default() };
    s.add(ProviderId::Ollama);
    let plan = Plan::new(Client::new(s, Arc::new(MemoryKeys::default()))).unwrap();
    let began = Instant::now();
    ws.ai_run = Some(BackgroundRun::start(&ctx, shared.clone(), run(&ids, vec![Step::AutoTag, Step::Rating { rating: 3 }]), plan));
    std::thread::sleep(Duration::from_millis(200));
    let waited = Instant::now();
    {
        let mut lib = lock(&shared);
        let mut item = lib.item(&ids[0]).unwrap().clone();
        item.name = "renamed during the run".into();
        lib.update_item(item).unwrap();
    }
    println!("the lock took {:?} while the model worked", waited.elapsed());
    assert!(waited.elapsed() < Duration::from_secs(1));
    assert!(tasks_running(&ws));
    let until = Instant::now() + Duration::from_secs(600);
    while tasks_running(&ws) {
        assert!(Instant::now() < until, "still running after ten minutes");
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("the run took {:?}", began.elapsed());
    let ended = settle(&mut ws, &Place::Shown).unwrap();
    println!("{} / {:?}", ended.message, ended.alert.as_ref().map(|a| a.body.clone()));
    assert!(ended.alert.is_none(), "a clean run");
    let item = lock(&shared).item(&ids[0]).unwrap().clone();
    println!("tags: {:?}", item.tags);
    assert_eq!(item.name, "renamed during the run");
    assert!(!item.tags.is_empty());
    assert_eq!(item.extra.get("star"), Some(&json!(3)));
    let tx = ws.undo.pop().unwrap();
    lock(&shared).revert(tx).unwrap();
    let item = lock(&shared).item(&ids[0]).unwrap().clone();
    assert_eq!((item.name.as_str(), item.tags.len()), ("renamed during the run", 0));
}
