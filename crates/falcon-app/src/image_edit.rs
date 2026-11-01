//! Item edits beyond metadata: custom thumbnails, refreshing thumbnails and
//! palettes, folder covers, rotate, flip and crop, and region comments. Requested
//! as one [`Op`] action (`Action::Edit`) and applied with the library locked.
//! This file is a child module of `app`.

use eframe::egui::{self, Key, Modifiers, Rect, Ui};
use falcon_library::model::{Comment, Item};
use falcon_library::transform::{self, Flip, PixelRect};
use falcon_library::{Library, item_ops, thumbnail};

use super::{Action, App};
use crate::logic::{self, Scope};

/// One edit, applied at the end of the frame.
pub enum Op {
    /// Ask for a picture, then make it the item's thumbnail.
    PickCustomThumbnail(String),
    ResetCustomThumbnail(String),
    RefreshThumbnail(Vec<String>),
    RefreshPalette(Vec<String>),
    /// Folder id, item id.
    SetFolderCover(String, String),
    /// Items and clockwise quarter turns (negative: anticlockwise).
    Rotate(Vec<String>, i32),
    Crop(String, PixelRect),
    /// Import the crop as a new item and leave the original alone.
    CropAsNew(String, PixelRect),
    AddComment(String, Comment),
    UpdateComment(String, Comment),
    /// Item id, comment id.
    DeleteComment(String, String),
    /// Mirror the items' originals in place.
    Flip(Vec<String>, Flip),
    /// Make a frame of the GIF player the item's thumbnail.
    FrameThumbnail(String, image::RgbaImage),
    /// Make a video frame, taken at this second, the item's thumbnail.
    VideoThumbnail(String, image::RgbaImage, f64),
    /// Rename the item (and its files) to this name.
    Rename(String, String),
    /// Set a font's sample language (`fontMetas.preferLng`).
    FontLanguage(String, String),
    /// Eagle's Duplicate: a copy of the item under a new id.
    Duplicate(String),
    /// Pin items to the top of these folders (`true`) or unpin them.
    Pin(Vec<String>, Vec<String>, bool),
}

impl Op {
    /// The undo name.
    pub fn label(&self) -> String {
        match self {
            Op::PickCustomThumbnail(_) => "Set custom thumbnail".into(),
            Op::ResetCustomThumbnail(_) => "Reset thumbnail".into(),
            Op::RefreshThumbnail(ids) => format!("Refresh {} thumbnails", ids.len()),
            Op::RefreshPalette(ids) => format!("Refresh {} palettes", ids.len()),
            Op::SetFolderCover(..) => "Set folder cover".into(),
            Op::Rotate(..) => "Rotate".into(),
            Op::Crop(..) => "Crop".into(),
            Op::CropAsNew(..) => "Crop as new item".into(),
            Op::AddComment(..) => "Add comment".into(),
            Op::UpdateComment(..) => "Edit comment".into(),
            Op::DeleteComment(..) => "Delete comment".into(),
            Op::Flip(_, Flip::Horizontal) => "Flip horizontally".into(),
            Op::Flip(_, Flip::Vertical) => "Flip vertically".into(),
            Op::FrameThumbnail(..) | Op::VideoThumbnail(..) => "Set frame as thumbnail".into(),
            Op::Rename(..) => "Rename".into(),
            Op::FontLanguage(..) => "Change sample language".into(),
            Op::Duplicate(_) => "Duplicate item".into(),
            Op::Pin(ids, _, true) => format!("Pin {} to the top", ids.len()),
            Op::Pin(ids, _, false) => format!("Unpin {}", ids.len()),
        }
    }
}

/// A comment being written in the viewer.
pub struct Draft {
    pub item: String,
    /// The comment being edited; `None` for a new one.
    pub id: Option<String>,
    /// Region in image pixels.
    pub rect: Rect,
    pub text: String,
    /// Focus the text box next frame.
    pub focus: bool,
}

/// Viewer and inspector state for these edits.
#[derive(Default)]
pub struct Tools {
    /// The viewer item the state belongs to.
    item: String,
    pub comment_mode: bool,
    /// Start of a region being dragged, in image pixels.
    pub draw_start: Option<eframe::egui::Pos2>,
    /// A saved region being moved or resized.
    pub region_drag: Option<super::comments::RegionDrag>,
    pub draft: Option<Draft>,
    pub crop: Option<super::crop::CropState>,
    /// Open the crop tool once the viewer shows this item.
    pub crop_next: Option<String>,
    /// Comment edited in the inspector: item id, comment id, text.
    pub list_edit: Option<(String, String, String)>,
}

impl App {
    /// Card context-menu entries for these edits. `targets` are the items
    /// the menu acts on.
    pub(super) fn edit_menu(&mut self, ui: &mut Ui, item: &Item, targets: &[String], lib: &Library) {
        let single = targets.len() == 1;
        ui.separator();
        // Eagle's Pin to Top / Unpin from Top, inside a folder: every picked
        // folder when several are shown together.
        if let Scope::Folder(fid) = &self.ws.scope {
            let picked = &self.ws.folders.sel.ids;
            let folders: Vec<String> = if picked.is_empty() { vec![fid.clone()] } else { picked.clone() };
            let pinned = item_ops::all_pinned(targets.iter().filter_map(|id| lib.item(id)), fid);
            if ui.button(if pinned { "Unpin from Top" } else { "Pin to Top" }).clicked() {
                self.ws.actions.push(Action::Edit(Op::Pin(targets.to_vec(), folders, !pinned)));
            }
        }
        if single
            && let Scope::Folder(fid) = &self.ws.scope
            && ui.button("Set as Folder Cover").clicked()
        {
            self.ws.actions.push(Action::Edit(Op::SetFolderCover(fid.clone(), item.id.clone())));
        }
        if single && thumbnail::custom_thumbnail_allowed(&item.ext) {
            if ui.button("Set Custom Thumbnail…").clicked() {
                self.ws.actions.push(Action::Edit(Op::PickCustomThumbnail(item.id.clone())));
            }
            if thumbnail::has_custom_thumbnail(item) && ui.button("Reset Thumbnail").clicked() {
                self.ws.actions.push(Action::Edit(Op::ResetCustomThumbnail(item.id.clone())));
            }
        }
        let keys = self.shortcut_text(ui.ctx(), crate::shortcuts::Command::RefreshThumbnail);
        if ui.add(egui::Button::new("Refresh Thumbnail").shortcut_text(keys)).clicked() {
            self.ws.actions.push(Action::Edit(Op::RefreshThumbnail(targets.to_vec())));
        }
        if ui.button("Refresh Palette").clicked() {
            self.ws.actions.push(Action::Edit(Op::RefreshPalette(targets.to_vec())));
        }
        let ext_of = |id: &str| lib.item(id).map(|i| i.ext.clone());
        let rotatable = editable(targets, ext_of, transform::can_rotate);
        if !rotatable.is_empty() {
            if ui.button("Rotate Left").clicked() {
                self.rotate_request(rotatable.clone(), -1);
            }
            if ui.button("Rotate Right").clicked() {
                self.rotate_request(rotatable, 1);
            }
        }
        let flippable = editable(targets, ext_of, transform::can_flip);
        if !flippable.is_empty() {
            if ui.button("Flip Horizontal").clicked() {
                self.flip_request(flippable.clone(), Flip::Horizontal);
            }
            if ui.button("Flip Vertical").clicked() {
                self.flip_request(flippable, Flip::Vertical);
            }
        }
        if single && transform::can_crop(&item.ext) && ui.button("Crop…").clicked() {
            self.ws.viewer = Some(item.id.clone());
            self.ws.tools.crop_next = Some(item.id.clone());
        }
    }

    /// Applies one edit. Thumbnails of changed items are dropped from the
    /// texture cache so the new pixels show.
    pub(super) fn apply_edit(&mut self, lib: &mut Library, op: Op) -> falcon_library::Result<()> {
        match op {
            Op::PickCustomThumbnail(id) => {
                let picked = rfd::FileDialog::new().set_title("Choose Thumbnail").add_filter("Images", &["jpg", "jpeg", "png", "bmp", "webp"]).pick_file();
                let Some(path) = picked else { return Ok(()) };
                lib.set_custom_thumbnail(&id, &path)?;
                self.ws.loader.forget(&id);
                self.flash("Thumbnail set");
                Ok(())
            }
            Op::FrameThumbnail(id, img) => {
                lib.set_frame_thumbnail(&id, &img)?;
                self.ws.loader.forget(&id);
                self.flash("Thumbnail set");
                Ok(())
            }
            Op::VideoThumbnail(id, img, at) => {
                lib.set_video_thumbnail(&id, &img, at)?;
                self.ws.loader.forget(&id);
                self.flash("Thumbnail set");
                Ok(())
            }
            Op::Rename(id, name) => {
                let Some(mut item) = lib.item(&id).cloned() else { return Ok(()) };
                item.name = name;
                lib.update_item(item)?;
                self.ws.loader.forget(&id);
                Ok(())
            }
            Op::FontLanguage(id, lang) => {
                lib.set_font_language(&id, &lang)?;
                self.ws.loader.forget(&id);
                Ok(())
            }
            Op::Duplicate(id) => {
                lib.duplicate_item(&id)?;
                self.flash("Duplicated");
                Ok(())
            }
            Op::Pin(ids, folders, pin) => {
                let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
                let n = lib.set_pinned(&ids, &folders, pin, now)?;
                let s = if n == 1 { "" } else { "s" };
                self.flash(if pin { format!("Pinned {n} item{s} to the top") } else { format!("Unpinned {n} item{s}") });
                Ok(())
            }
            Op::ResetCustomThumbnail(id) => {
                lib.reset_custom_thumbnail(&id)?;
                self.ws.loader.forget(&id);
                Ok(())
            }
            Op::RefreshThumbnail(ids) => {
                let ids = self.local_only(lib, &ids);
                if ids.is_empty() {
                    return Ok(());
                }
                let (mut done, mut custom) = (0, 0);
                let mut r = Ok(());
                for id in &ids {
                    match lib.refresh_thumbnail(id) {
                        Ok(true) => done += 1,
                        Ok(false) => custom += 1,
                        Err(e) => r = Err(e),
                    }
                    self.ws.loader.forget(id);
                }
                let mut msg = format!("Refreshed {done} {}", if done == 1 { "thumbnail" } else { "thumbnails" });
                if custom > 0 {
                    msg.push_str(&format!(", kept {custom} custom"));
                }
                self.flash(msg);
                r
            }
            Op::RefreshPalette(ids) => {
                let ids = self.local_only(lib, &ids);
                if ids.is_empty() {
                    return Ok(());
                }
                let mut done = 0;
                let mut r = Ok(());
                for id in &ids {
                    match lib.refresh_palette(id) {
                        Ok(true) => done += 1,
                        Ok(false) => {}
                        Err(e) => r = Err(e),
                    }
                }
                self.flash(format!("Refreshed {done} {}", if done == 1 { "palette" } else { "palettes" }));
                r
            }
            Op::SetFolderCover(fid, id) => {
                lib.set_folder_cover(&fid, Some(&id))?;
                let name = logic::find_folder(&lib.metadata.folders, &fid).map_or_else(String::new, |f| f.name.clone());
                self.flash(format!("Set as the cover of {name}"));
                Ok(())
            }
            Op::Rotate(ids, turns) => {
                let ids = self.local_only(lib, &ids);
                let mut r = Ok(());
                for id in &ids {
                    if lib.item(id).is_some_and(|i| transform::can_rotate(&i.ext)) {
                        r = r.and(lib.rotate_item(id, turns));
                        self.ws.loader.forget(id);
                    }
                }
                r
            }
            Op::Flip(ids, flip) => {
                let ids = self.local_only(lib, &ids);
                let mut r = Ok(());
                for id in &ids {
                    if lib.item(id).is_some_and(|i| transform::can_flip(&i.ext)) {
                        r = r.and(lib.flip_item(id, flip));
                        self.ws.loader.forget(id);
                    }
                }
                r
            }
            Op::Crop(id, rect) => {
                if self.local_only(lib, std::slice::from_ref(&id)).is_empty() {
                    return Ok(());
                }
                let r = lib.crop_item(&id, rect);
                self.ws.loader.forget(&id);
                r
            }
            Op::CropAsNew(id, rect) => {
                let new = lib.crop_item_as_new(&id, rect)?;
                self.ws.tools.crop = None;
                self.ws.viewer = None;
                self.ws.selected = vec![new.clone()];
                self.ws.anchor = Some(new.clone());
                self.ws.scroll_to = Some(new);
                self.flash("Saved the crop as a new item");
                Ok(())
            }
            Op::AddComment(id, c) => lib.add_comment(&id, c).map(|_| ()),
            Op::UpdateComment(id, c) => lib.update_comment(&id, c),
            Op::DeleteComment(id, cid) => lib.delete_comment(&id, &cid),
        }
    }

    /// Keys for the viewer's tools, read before the app's own shortcuts so
    /// Esc and Enter act on the crop tool rather than close the viewer: C
    /// toggles comment mode, ⇧R rotates left, ⇧F flips horizontally (as
    /// Eagle's flip shortcut does).
    pub(super) fn edit_keys(&mut self, ctx: &egui::Context) {
        if self.ws.viewer.is_none() {
            self.ws.tools.crop = None;
            self.ws.tools.draft = None;
            self.ws.tools.draw_start = None;
            self.ws.tools.region_drag = None;
            self.ws.tools.item.clear();
            self.image.tf = Default::default();
            return;
        }
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let none = Modifiers::NONE;
        if let Some(crop) = self.ws.tools.crop.as_mut() {
            let (esc, enter) = ctx.input_mut(|i| (i.consume_key(none, Key::Escape), i.consume_key(none, Key::Enter)));
            ctx.input_mut(|i| {
                for k in [Key::Space, Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown] {
                    i.consume_key(none, k);
                }
            });
            if esc {
                if crop.confirming {
                    crop.confirming = false;
                } else {
                    self.ws.tools.crop = None;
                }
            } else if enter {
                if crop.confirming {
                    let rect = super::crop::pixel_rect(crop.rect);
                    if let Some(id) = self.ws.viewer.clone() {
                        self.ws.actions.push(Action::Edit(Op::Crop(id, rect)));
                    }
                    self.ws.tools.crop = None;
                } else {
                    crop.confirming = true;
                }
            }
            return;
        }
        // ⇧C opens the crop tool (a command); it isn't the bare C of comment mode.
        let shift_c = ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::C));
        let (c, rotate, flip) = ctx.input_mut(|i| (!shift_c && i.consume_key(none, Key::C), i.consume_key(Modifiers::SHIFT, Key::R), i.consume_key(Modifiers::SHIFT, Key::F)));
        if c {
            self.ws.tools.comment_mode = !self.ws.tools.comment_mode;
            self.ws.tools.draft = None;
        }
        if rotate && let Some(id) = self.ws.viewer.clone() {
            self.rotate_request(vec![id], -1);
        }
        if flip && let Some(id) = self.ws.viewer.clone() {
            self.flip_request(vec![id], Flip::Horizontal);
        }
        if self.ws.tools.comment_mode && ctx.input_mut(|i| i.consume_key(none, Key::Escape)) {
            self.ws.tools.comment_mode = false;
            self.ws.tools.draft = None;
        }
    }

    /// Everything these tools draw over the viewer's image (at `shown`,
    /// inside `area`). Their widgets sit above the viewer's canvas, so in
    /// comment mode and while cropping a drag draws instead of panning.
    pub(super) fn edit_overlay(&mut self, ui: &mut Ui, item: &Item, shown: Rect, area: Rect) {
        if self.ws.tools.item != item.id {
            self.ws.tools.item = item.id.clone();
            self.ws.tools.crop = None;
            self.ws.tools.draft = None;
            self.ws.tools.draw_start = None;
            self.ws.tools.region_drag = None;
        }
        let turned = !self.image.tf.is_identity();
        if self.ws.tools.crop_next.as_deref() == Some(item.id.as_str()) {
            self.ws.tools.crop_next = None;
            self.start_crop(item);
        }
        // The overlays work in file pixels, so they wait for a preview-only
        // rotation to be undone (the crop tool drops it) and draw next frame.
        if turned {
            if self.ws.tools.crop.is_some() {
                ui.ctx().request_repaint();
            }
            return;
        }
        if !self.crop_overlay(ui, item, shown, area) {
            self.comment_overlay(ui, item, shown, area);
        }
    }

    /// The tools' buttons in the viewer bar, leftwards from `x`. Returns
    /// the x left of them.
    pub(super) fn edit_bar(&mut self, ui: &mut Ui, item: &Item, x: f32, cy: f32) -> f32 {
        if self.ws.tools.crop.is_some() {
            return self.crop_bar(ui, item, x + 12.0, cy);
        }
        let x = self.comment_button(ui, item, x, cy);
        self.rotate_crop_buttons(ui, item, x, cy) + 14.0
    }
}

/// The targets whose format `allow` accepts (`can_rotate`, `can_flip`);
/// `ext_of` looks an item's extension up.
pub fn editable(targets: &[String], ext_of: impl Fn(&str) -> Option<String>, allow: fn(&str) -> bool) -> Vec<String> {
    targets.iter().filter(|id| ext_of(id).is_some_and(|e| allow(&e))).cloned().collect()
}

/// The mirror a click on the viewer's flip button asks for: horizontal,
/// or vertical with ⇧ held (Eagle).
pub fn flip_for_click(shift: bool) -> Flip {
    if shift { Flip::Vertical } else { Flip::Horizontal }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_targets_follow_the_format() {
        let ext_of = |id: &str| match id {
            "a" => Some("JPG".to_string()),
            "b" => Some("gif".to_string()),
            "c" => Some("webp".to_string()),
            "d" => Some("avif".to_string()),
            "e" => Some("tex".to_string()),
            _ => None,
        };
        let ids: Vec<String> = ["a", "b", "c", "d", "e", "gone"].map(String::from).to_vec();
        assert_eq!(editable(&ids, ext_of, transform::can_flip), ["a", "c"]);
        assert_eq!(editable(&ids, ext_of, transform::can_rotate), ["a", "c"]);
        assert!(editable(&[], ext_of, transform::can_flip).is_empty());
    }

    #[test]
    fn shift_click_flips_vertically() {
        assert_eq!(flip_for_click(false), Flip::Horizontal);
        assert_eq!(flip_for_click(true), Flip::Vertical);
        assert_eq!(Op::Flip(vec![], Flip::Vertical).label(), "Flip vertically");
    }
}
