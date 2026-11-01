//! The font viewer: a specimen drawn with the font itself in four tabs
//! (article, waterfall, alphabet and information), a size slider with zoom
//! buttons, colour themes, the sample language, renaming the file from the
//! font's names, and activating the font as Eagle does (see the desktop
//! app chapter of the docs). A font egui can't parse (some WOFF2 and CFF
//! files) keeps the header, the information tab, renaming and activation,
//! with QuickLook's specimen in place of the drawn tabs.

use eframe::egui::{self, Align2, Color32, FontData, FontFamily, FontId, FontInsert, InsertFontFamily, Pos2, Rect, RichText, Sense, Stroke, TextEdit, Ui, Vec2, vec2};
use falcon_library::fontmeta::{self, FontInfo, SAMPLE_LANGUAGES};
use falcon_library::fontpreview::Specimen;
use falcon_library::model::Item;
use falcon_library::{Library, search};

use crate::app::image_edit::Op;
use crate::app::{Action, App};
use crate::ui::widgets::icon_button;
use crate::icons as ic;
use crate::loader::{Tier, job_for};
use crate::logic::ViewZoom;
use crate::theme::{self as t, font, mono};

/// Eagle's waterfall sizes at 100%, in points.
pub const WATERFALL: [f32; 11] = [72.0, 64.0, 56.0, 48.0, 38.0, 30.0, 24.0, 20.0, 16.0, 14.0, 12.0];
/// The article's body and heading sizes, and the glyph size, at 100%.
const BODY: f32 = 18.0;
const HEADING: f32 = 38.0;
const GLYPH: f32 = 25.0;
/// Zoom limits and step, as a factor of the default size (Eagle's slider
/// runs 31.25–625% of a 62.5% root size, in steps of 31.25).
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 10.0;
pub const ZOOM_STEP: f32 = 0.5;
/// The least time between two wheel zoom steps (Eagle throttles to 200 ms).
const WHEEL_THROTTLE: f64 = 0.2;

/// The viewer's tabs, in Eagle's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FontTab {
    #[default]
    Article,
    Waterfall,
    Alphabet,
    Info,
}

impl FontTab {
    pub const ALL: [FontTab; 4] = [FontTab::Article, FontTab::Waterfall, FontTab::Alphabet, FontTab::Info];

    pub fn label(self) -> &'static str {
        match self {
            FontTab::Article => "Article",
            FontTab::Waterfall => "Waterfall",
            FontTab::Alphabet => "Alphabet",
            FontTab::Info => "Info",
        }
    }
}

/// The specimen's colour themes, in Eagle's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FontTheme {
    /// Follows the app's light or dark appearance.
    #[default]
    Auto,
    Light,
    Dark,
    Purple,
    Yellow,
}

impl FontTheme {
    pub const ALL: [FontTheme; 5] = [FontTheme::Auto, FontTheme::Light, FontTheme::Dark, FontTheme::Purple, FontTheme::Yellow];

    /// Background and text colours; `None` for Auto, which takes the app's.
    pub fn colors(self) -> Option<(Color32, Color32)> {
        let dark_ink = Color32::from_rgb(0x1f, 0x1f, 0x22);
        let light_ink = Color32::from_rgb(0xeb, 0xeb, 0xee);
        match self {
            FontTheme::Auto => None,
            FontTheme::Light => Some((Color32::WHITE, dark_ink)),
            FontTheme::Dark => Some((Color32::from_rgb(0x18, 0x19, 0x1c), light_ink)),
            FontTheme::Purple => Some((Color32::from_rgb(0x3d, 0x45, 0x99), light_ink)),
            FontTheme::Yellow => Some((Color32::from_rgb(0xfa, 0xe1, 0x3c), dark_ink)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FontTheme::Auto => "Auto (follows the app)",
            FontTheme::Light => "Light",
            FontTheme::Dark => "Dark",
            FontTheme::Purple => "Purple",
            FontTheme::Yellow => "Yellow",
        }
    }
}

/// What the font viewer keeps across fonts and launches, as Eagle keeps
/// the tab and theme in local storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FontLook {
    pub tab: FontTab,
    pub theme: FontTheme,
}

/// The font viewer's state for the font on screen.
#[derive(Default)]
pub(crate) struct FontView {
    id: String,
    /// The family the font is registered under, or `None` if egui can't
    /// read it.
    family: Option<FontFamily>,
    /// The specimen language the texts below were made for.
    specimen: Option<Specimen>,
    /// The article's text and the waterfall's line, editable in place.
    text: String,
    line: String,
    /// The text was typed by the user, so it stays when another font opens.
    edited: bool,
    /// Size as a factor of the default (1 = Eagle's 62.5%).
    zoom: f32,
    /// The alphabet's groups, keeping only characters the font has.
    glyphs: Option<Vec<(&'static str, String)>>,
    /// The name field in the header.
    name: String,
    /// Where the font is activated, read when it opens (`None` on systems
    /// without a fonts folder).
    active: Option<crate::font_install::Activation>,
    /// When the wheel last zoomed (egui time).
    wheel_at: f64,
    /// egui can't read the font, so QuickLook draws its specimen.
    fallback: bool,
    /// The fallback shows the information tab rather than the specimen.
    fallback_info: bool,
}

/// Whether egui can parse a font, checked with the same parser it uses, as
/// it panics on fonts it can't read.
pub(crate) fn egui_can_read(data: &[u8]) -> bool {
    skrifa::FontRef::from_index(data, 0).is_ok()
}

/// The zoom after one press of zoom in (`up`) or out.
pub fn zoom_step(zoom: f32, up: bool) -> f32 {
    (zoom + if up { ZOOM_STEP } else { -ZOOM_STEP }).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// `fontMetas.preferLng` for a specimen language.
pub fn language_code(s: Specimen) -> &'static str {
    match s {
        Specimen::Latin => "en",
        Specimen::Japanese => "jp",
        Specimen::Korean => "kr",
        Specimen::SimplifiedChinese => "zh_CN",
        Specimen::TraditionalChinese => "zh_TW",
    }
}

/// falcon's own sample texts for a language: the article's heading and
/// body, and the waterfall line.
pub fn samples(s: Specimen) -> (&'static str, &'static str, &'static str) {
    match s {
        Specimen::Latin => (
            "Letters on a Quiet Morning",
            "Early light settled over the harbour while the town was still asleep. A baker carried trays of bread along the empty street, and gulls circled above the fishing boats, waiting for the nets to come in.\n\nType is the voice of written words. A good typeface disappears into the reading, yet every curve and counter shapes how a text feels: warm or formal, quick or patient, loud or calm.\n\nPack my box with five dozen liquor jugs. How vexingly quick daft zebras jump! 0123456789",
            "Hamburgefonstiv 0123456789",
        ),
        Specimen::Japanese => (
            "静かな朝の手紙",
            "朝の光が港をやわらかく照らし、町はまだ眠っていた。パン屋は焼きたてのパンを抱えて通りを歩き、かもめは漁船の上をゆっくりと回っていた。\n\n文字は書かれた言葉の声である。よい書体は読むことの中に溶け込みながら、その一画一画が文章の印象を形づくる。\n\nいろはにほへと ちりぬるを わかよたれそ つねならむ 0123456789",
            "永遠の文字 あア Aa 0123",
        ),
        Specimen::Korean => (
            "조용한 아침의 편지",
            "이른 햇살이 항구 위로 부드럽게 내려앉았고 마을은 아직 잠들어 있었다. 빵집 주인은 갓 구운 빵을 들고 텅 빈 거리를 걸었다.\n\n글자는 글의 목소리다. 좋은 글꼴은 읽는 가운데 사라지지만, 모든 획이 글의 느낌을 만든다.\n\n다람쥐 헌 쳇바퀴에 타고파 0123456789",
            "한글의 아름다움 Aa 0123",
        ),
        Specimen::SimplifiedChinese => (
            "安静早晨的信",
            "清晨的阳光洒在港口上，小城还在沉睡。面包师端着刚出炉的面包走过空荡的街道，海鸥在渔船上空盘旋。\n\n文字是书面语言的声音。好的字体在阅读中隐去自身，却以每一笔画塑造文章的气质。\n\n天地玄黄，宇宙洪荒。日月盈昃，辰宿列张。0123456789",
            "字体之美 Aa 0123",
        ),
        Specimen::TraditionalChinese => (
            "安靜早晨的信",
            "清晨的陽光灑在港口上，小城還在沉睡。麵包師端著剛出爐的麵包走過空蕩的街道，海鷗在漁船上空盤旋。\n\n文字是書面語言的聲音。好的字體在閱讀中隱去自身，卻以每一筆畫塑造文章的氣質。\n\n天地玄黃，宇宙洪荒。日月盈昃，辰宿列張。0123456789",
            "字體之美 Aa 0123",
        ),
    }
}

/// The alphabet tab's groups for a language: Latin letters, numbers and
/// punctuation, then the language's own script.
pub fn glyph_groups(s: Specimen) -> Vec<(&'static str, String)> {
    let range = |a: u32, b: u32| (a..=b).filter_map(char::from_u32).collect::<String>();
    let mut groups = vec![
        ("Uppercase", range('A' as u32, 'Z' as u32)),
        ("Lowercase", range('a' as u32, 'z' as u32)),
        ("Numbers", range('0' as u32, '9' as u32)),
        ("Punctuation", "!?¡¿&@#%*()[]{}.,:;'\"/\\-–—…«»“”‘’$€£¥¢+−×÷=<>©®".to_string()),
    ];
    match s {
        Specimen::Latin => groups.push(("Accented", "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÑÒÓÔÕÖØŒÙÚÛÜÝŸßàáâãäåæçèéêëìíîïñòóôõöøœùúûüýÿ".to_string())),
        Specimen::Japanese => {
            groups.push(("Hiragana", range(0x3041, 0x3093)));
            groups.push(("Katakana", range(0x30A1, 0x30F3)));
            groups.push(("Kanji", "日月火水木金土山川田人口目耳手足心学校先生年本中大小上下左右名字花草雨雪空海森林春夏秋冬東西南北朝昼夜".to_string()));
        }
        Specimen::Korean => {
            // Each initial consonant with five vowels, no final.
            let syllables = (0..19u32).flat_map(|i| [0u32, 4, 8, 13, 18].map(move |v| 0xAC00 + (i * 21 + v) * 28)).filter_map(char::from_u32).collect();
            groups.push(("Hangul", syllables));
        }
        Specimen::SimplifiedChinese => groups.push(("Hanzi", "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对可她里后小么心多天而能好都然没日于起还发成事只作当想看文无开手十用主行方又如前所本见经头面公同三已老从动两长".to_string())),
        Specimen::TraditionalChinese => groups.push(("Hanzi", "的一是不了人我在有他這中大來上國個到說們為子和你地出道也時年得就那要下以生會自著去之過家學對可她裡後小麼心多天而能好都然沒日於起還發成事只作當想看文無開手十用主行方又如前所本見經頭面公同三已老從動兩長".to_string())),
    }
    groups
}

/// The information tab's rows: label, value, and whether the value can
/// name the file (PostScript, full and family names, as in Eagle). Empty
/// values are left out.
pub fn info_rows(info: &FontInfo) -> Vec<(&'static str, String, bool)> {
    let rows = [
        ("PostScript Name", info.postscript.clone(), true),
        ("Full Name", info.full_name.clone(), true),
        ("Family", info.family.clone(), true),
        ("Style", info.style.clone(), false),
        ("Version", info.version.clone(), false),
        ("Designer", info.designer.clone(), false),
        ("Manufacturer", info.manufacturer.clone(), false),
        ("Description", info.description.clone(), false),
        ("Copyright", info.copyright.clone(), false),
        ("Trademark", info.trademark.clone(), false),
        ("Glyphs", info.glyphs.map(|n| n.to_string()).unwrap_or_default(), false),
    ];
    rows.into_iter().filter(|r| !r.1.is_empty()).collect()
}

/// The information tab's rows, with "Use as File Name" beside the names.
/// Returns the name picked.
fn font_info_grid(ui: &mut Ui, info: &FontInfo, item: &Item, muted: Color32) -> Option<String> {
    let mut name_from = None;
    egui::Grid::new("font-info").num_columns(2).spacing(vec2(24.0, 10.0)).striped(false).show(ui, |ui| {
        for (label, value, renames) in info_rows(info) {
            ui.label(RichText::new(label).color(muted).font(font(12.5)));
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(560.0);
                ui.label(RichText::new(&value).font(font(13.0)));
                if renames && value != item.name && ui.small_button("Use as File Name").clicked() {
                    name_from = Some(value.clone());
                }
            });
            ui.end_row();
        }
    });
    name_from
}

fn metas(item: &Item) -> serde_json::Map<String, serde_json::Value> {
    item.extra.get("fontMetas").and_then(|m| m.as_object()).cloned().unwrap_or_default()
}

impl App {
    /// Registers the font with egui the first time it is viewed; true once
    /// egui has it (a frame later).
    pub(crate) fn font_ready(&mut self, ctx: &egui::Context, lib: &Library, item: &Item) -> bool {
        if search::type_group(&item.ext) != Some("font") {
            return false;
        }
        if self.ws.font_view.id != item.id {
            let family = FontFamily::Name(format!("falcon-font-{}", item.id).into());
            let data = std::fs::read(item.original_path(lib.root())).ok();
            let sfnt = data.as_deref().and_then(fontmeta::sfnt).map(|c| c.into_owned()).filter(|d| egui_can_read(d));
            let fallback = sfnt.is_none();
            let family = sfnt.map(|bytes| {
                // egui keeps a font once added; adding the same name again is a no-op.
                ctx.add_font(FontInsert {
                    name: format!("falcon-font-{}", item.id),
                    data: FontData::from_owned(bytes),
                    families: vec![InsertFontFamily { family: family.clone(), priority: egui::FontPriority::Highest }],
                });
                family
            });
            let old = std::mem::take(&mut self.ws.font_view);
            let active = activation(item);
            self.ws.font_view = FontView {
                id: item.id.clone(),
                family,
                // Text typed by the user stays; samples follow the font's language.
                text: if old.edited { old.text } else { String::new() },
                line: if old.edited { old.line } else { String::new() },
                edited: old.edited,
                zoom: if old.zoom > 0.0 { old.zoom } else { 1.0 },
                name: item.name.clone(),
                active,
                fallback,
                fallback_info: old.fallback_info,
                ..Default::default()
            };
        }
        // The language can change while the font is open.
        let specimen = Specimen::for_metas(&metas(item));
        let fv = &mut self.ws.font_view;
        if fv.specimen != Some(specimen) {
            fv.specimen = Some(specimen);
            fv.glyphs = None;
            if !fv.edited {
                let (_, body, line) = samples(specimen);
                (fv.text, fv.line) = (body.to_string(), line.to_string());
            }
        }
        match &self.ws.font_view.family {
            Some(f) => {
                let ready = ctx.fonts(|fonts| fonts.definitions().families.contains_key(f));
                if !ready {
                    ctx.request_repaint();
                }
                ready
            }
            None => self.ws.font_view.fallback,
        }
    }

    pub(crate) fn font_view(&mut self, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        let Some(family) = self.ws.font_view.family.clone() else {
            if self.ws.font_view.fallback {
                self.font_fallback_view(ui, lib, item, area);
            }
            return;
        };
        let look = self.font_look;
        // The grayscale preview grays the specimen's colours, as Eagle's
        // filter grays its font page.
        let grayscale = self.modes.grayscale;
        let gray = move |c: Color32| if grayscale { crate::logic::gray_color(c) } else { c };
        let (bg, ink) = look.theme.colors().unwrap_or((t::p().main, t::p().text));
        let (bg, ink, accent) = (gray(bg), gray(ink), gray(t::p().primary));
        let muted = ink.gamma_multiply(0.6);
        ui.painter().rect_filled(area, 0.0, bg);
        let specimen = self.ws.font_view.specimen.unwrap_or(Specimen::Latin);

        // ⌘= ⌘− ⌘0 ⌘9 (through the table's zoom commands) and the zoom keys.
        if let Some(z) = self.ws.view.zoom_key.take() {
            let fv = &mut self.ws.font_view;
            fv.zoom = match z {
                ViewZoom::In => zoom_step(fv.zoom, true),
                ViewZoom::Out => zoom_step(fv.zoom, false),
                ViewZoom::Fit | ViewZoom::Actual => 1.0,
                ViewZoom::Edge => fv.zoom,
            };
        }
        // ⌘-, ⌃- or ⌥-scrolling (or pinching) zooms a step at a time.
        if ui.rect_contains_pointer(area) {
            let (pinch, alt_scroll, now) = ui.input(|i| (i.zoom_delta(), if i.modifiers.alt { i.smooth_scroll_delta.y } else { 0.0 }, i.time));
            let dir = if pinch > 1.0 || alt_scroll > 5.0 { Some(true) } else if pinch < 1.0 || alt_scroll < -5.0 { Some(false) } else { None };
            if alt_scroll != 0.0 {
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
            let fv = &mut self.ws.font_view;
            if let Some(up) = dir
                && now - fv.wheel_at >= WHEEL_THROTTLE
            {
                fv.zoom = zoom_step(fv.zoom, up);
                fv.wheel_at = now;
            }
        }

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(48.0, 16.0))));
        let ui = &mut child;
        ui.visuals_mut().override_text_color = Some(ink);

        // Header: the file name (edit to rename), the activate switch and
        // the PostScript name.
        let info = FontInfo::from_metas(&metas(item));
        let (rename, toggle) = self.font_header(ui, item, &info, FontId::new(26.0, family.clone()), [ink, muted, accent]);
        ui.add_space(10.0);

        // Tabs at the left; language and themes at the right.
        let mut language = None;
        ui.horizontal(|ui| {
            for tab in FontTab::ALL {
                let on = self.font_look.tab == tab;
                let text = RichText::new(tab.label()).color(if on { ink } else { muted }).font(font(13.0));
                if ui.add(egui::Button::new(text).fill(if on { ink.gamma_multiply(0.12) } else { Color32::TRANSPARENT }).stroke(Stroke::NONE)).clicked() {
                    self.font_look.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for theme in FontTheme::ALL.iter().rev() {
                    let (sbg, sink) = theme.colors().unwrap_or((t::p().main, t::p().text));
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                    let p = ui.painter();
                    p.circle_filled(r.center(), 10.0, gray(sbg));
                    let ring = if self.font_look.theme == *theme { Stroke::new(2.0, accent) } else { Stroke::new(1.0, muted) };
                    p.circle_stroke(r.center(), 10.0, ring);
                    let glyph = if *theme == FontTheme::Auto { "A" } else { "a" };
                    p.text(r.center(), Align2::CENTER_CENTER, glyph, font(12.0), gray(sink));
                    if resp.on_hover_text(theme.label()).clicked() {
                        self.font_look.theme = *theme;
                    }
                }
                ui.add_space(8.0);
                let current = SAMPLE_LANGUAGES.iter().find(|(c, _)| *c == language_code(specimen)).map_or("English", |l| l.1);
                ui.menu_button(RichText::new(format!("{current} ▾")).color(ink).font(font(12.5)), |ui| {
                    ui.label(RichText::new("Sample language").font(font(11.5)).weak());
                    for (code, name) in SAMPLE_LANGUAGES {
                        if ui.selectable_label(code == language_code(specimen), name).clicked() {
                            language = Some(code);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("The language of the sample text and the thumbnail");
            });
        });

        // The size slider with zoom buttons, except on the information tab.
        let tab = self.font_look.tab;
        if tab != FontTab::Info {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let fv = &mut self.ws.font_view;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                if icon_button(ui, r.center(), ic::MINUS, 15.0, fv.zoom > ZOOM_MIN, "Smaller (⌘−)").clicked() {
                    fv.zoom = zoom_step(fv.zoom, false);
                }
                ui.spacing_mut().slider_width = 220.0;
                ui.add(egui::Slider::new(&mut fv.zoom, ZOOM_MIN..=ZOOM_MAX).show_value(false).logarithmic(true));
                let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                if icon_button(ui, r.center(), ic::PLUS, 15.0, fv.zoom < ZOOM_MAX, "Larger (⌘=)").clicked() {
                    fv.zoom = zoom_step(fv.zoom, true);
                }
                let pct = ui.add(egui::Label::new(RichText::new(format!("{:.0}%", fv.zoom * 100.0)).font(mono(12.0)).color(muted)).sense(Sense::click()));
                if pct.on_hover_text("Click for 100% (⌘0)").clicked() {
                    fv.zoom = 1.0;
                }
                if fv.edited && ui.button("Reset Text").clicked() {
                    let (_, body, line) = samples(specimen);
                    (fv.text, fv.line, fv.edited) = (body.to_string(), line.to_string(), false);
                }
            });
        }
        ui.add_space(8.0);

        let mut name_from = None;
        egui::ScrollArea::vertical().id_salt(("font-view", tab)).auto_shrink([false, false]).show(ui, |ui| match tab {
            FontTab::Article => self.font_article(ui, &family, specimen, ink),
            FontTab::Waterfall => self.font_waterfall(ui, &family, ink, muted),
            FontTab::Alphabet => self.font_alphabet(ui, &family, specimen, ink, muted, bg),
            FontTab::Info => name_from = font_info_grid(ui, &info, item, muted),
        });

        if let Some(name) = rename.or(name_from) {
            self.ws.actions.push(Action::Edit(Op::Rename(item.id.clone(), name)));
        }
        if let Some(code) = language {
            self.ws.actions.push(Action::Edit(Op::FontLanguage(item.id.clone(), code.to_string())));
        }
        if let Some(on) = toggle {
            self.toggle_font(lib, item, on);
        }
    }

    /// The header: the file name (edit to rename) in `name_font`, the
    /// activate switch and the PostScript name. Returns a typed new name
    /// and a requested activation change.
    fn font_header(&mut self, ui: &mut Ui, item: &Item, info: &FontInfo, name_font: FontId, [ink, muted, accent]: [Color32; 3]) -> (Option<String>, Option<bool>) {
        let mut rename = None;
        let mut toggle = None;
        ui.horizontal(|ui| {
            let fv = &mut self.ws.font_view;
            let r = ui.add(
                TextEdit::singleline(&mut fv.name)
                    .id_salt("font-name")
                    .font(name_font)
                    .text_color(ink)
                    .frame(egui::Frame::NONE)
                    .desired_width((ui.available_width() - 140.0).max(120.0)),
            );
            let r = r.on_hover_text("Rename the file");
            if r.lost_focus() {
                let typed = fv.name.trim().to_string();
                if !typed.is_empty() && typed != item.name {
                    rename = Some(typed);
                } else {
                    fv.name = item.name.clone();
                }
            } else if !r.has_focus() {
                fv.name = item.name.clone();
            }
            if let Some(state) = fv.active {
                use crate::font_install::Activation;
                // A font only in Eagle's folder is Eagle's to deactivate.
                let (label, tip) = match state {
                    Activation::Off => ("Activate", "Install the font for every app"),
                    Activation::Falcon => ("Activated", "Deactivate the font"),
                    Activation::Eagle => ("Activated by Eagle", "Eagle installed this font; deactivate it in Eagle"),
                };
                let active = state != Activation::Off;
                let b = egui::Button::new(RichText::new(label).color(if active { Color32::WHITE } else { ink }))
                    .fill(if active { accent } else { Color32::TRANSPARENT })
                    .stroke(Stroke::new(1.0, muted));
                let r = ui.add_enabled(state != Activation::Eagle, b);
                let r = if state == Activation::Eagle { r.on_disabled_hover_text(tip) } else { r.on_hover_text(tip) };
                if r.clicked() {
                    toggle = Some(!active);
                }
            }
        });
        if !info.postscript.is_empty() {
            ui.label(RichText::new(&info.postscript).color(muted).font(font(12.0)));
        }
        (rename, toggle)
    }

    /// A font egui can't draw: the same header, a specimen QuickLook
    /// renders (else the thumbnail) and the information tab.
    fn font_fallback_view(&mut self, ui: &mut Ui, lib: &Library, item: &Item, area: Rect) {
        let grayscale = self.modes.grayscale;
        let gray = move |c: Color32| if grayscale { crate::logic::gray_color(c) } else { c };
        let (bg, ink, accent) = (gray(t::p().main), gray(t::p().text), gray(t::p().primary));
        let muted = ink.gamma_multiply(0.6);
        ui.painter().rect_filled(area, 0.0, bg);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area.shrink2(vec2(48.0, 16.0))));
        let ui = &mut child;
        ui.visuals_mut().override_text_color = Some(ink);
        let info = FontInfo::from_metas(&metas(item));
        let (rename, toggle) = self.font_header(ui, item, &info, FontId::proportional(26.0), [ink, muted, accent]);
        ui.add_space(10.0);

        // Two tabs: the specimen and the information.
        ui.horizontal(|ui| {
            for (label, info_tab) in [("Specimen", false), ("Info", true)] {
                let on = self.ws.font_view.fallback_info == info_tab;
                let text = RichText::new(label).color(if on { ink } else { muted }).font(font(13.0));
                if ui.add(egui::Button::new(text).fill(if on { ink.gamma_multiply(0.12) } else { Color32::TRANSPARENT }).stroke(Stroke::NONE)).clicked() {
                    self.ws.font_view.fallback_info = info_tab;
                }
            }
            ui.add_space(12.0);
            ui.label(RichText::new("Drawn by QuickLook: falcon can't read this font itself").color(muted).font(font(11.5)));
        });
        ui.add_space(8.0);

        let mut name_from = None;
        if self.ws.font_view.fallback_info {
            egui::ScrollArea::vertical().id_salt("font-view-fallback").auto_shrink([false, false]).show(ui, |ui| {
                name_from = font_info_grid(ui, &info, item, muted);
            });
        } else {
            // A tier with no job to run counts as failed.
            let mut failed = true;
            for tier in [Tier::Full, Tier::Preview] {
                if let Some(j) = job_for(item, lib.root(), tier) {
                    self.ws.frame_jobs.push(j);
                    failed &= self.ws.loader.has_failed(&item.id, tier);
                }
            }
            let tex = self.ws.loader.get(&item.id, Tier::Full).cloned().or_else(|| self.ws.loader.get(&item.id, Tier::Preview).cloned());
            let (rest, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
            match tex {
                Some(tex) => {
                    let size = tex.size_vec2();
                    let k = (rest.width() / size.x).min(rest.height() / size.y).min(1.0);
                    let r = Rect::from_center_size(rest.center(), size * k);
                    ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None if failed => {
                    ui.painter().text(rest.center(), Align2::CENTER_CENTER, "No specimen for this font", font(13.0), muted);
                }
                None => {
                    ui.painter().text(rest.center(), Align2::CENTER_CENTER, "Rendering the specimen…", font(13.0), muted);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }

        if let Some(name) = rename.or(name_from) {
            self.ws.actions.push(Action::Edit(Op::Rename(item.id.clone(), name)));
        }
        if let Some(on) = toggle {
            self.toggle_font(lib, item, on);
        }
    }

    fn font_article(&mut self, ui: &mut Ui, family: &FontFamily, specimen: Specimen, ink: Color32) {
        let fv = &mut self.ws.font_view;
        let z = fv.zoom;
        let width = ui.available_width().min(760.0 * z.max(1.0));
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(samples(specimen).0).font(FontId::new(HEADING * z, family.clone())).color(ink));
        });
        ui.add_space(10.0 * z);
        // The text is editable in place, as in Eagle's article.
        let r = ui.add(
            TextEdit::multiline(&mut fv.text)
                .id_salt("font-sample")
                .font(FontId::new(BODY * z, family.clone()))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .desired_rows(1)
                .desired_width(width),
        );
        if r.changed() {
            fv.edited = true;
        }
        ui.add_space(24.0);
    }

    fn font_waterfall(&mut self, ui: &mut Ui, family: &FontFamily, ink: Color32, muted: Color32) {
        let fv = &mut self.ws.font_view;
        for size in WATERFALL {
            let px = size * fv.zoom;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(36.0, px.max(14.0)), Sense::hover());
                ui.painter().text(r.left_center(), Align2::LEFT_CENTER, format!("{size:.0}"), mono(11.0), muted);
                // Every line edits the same text, so an edit shows on all of
                // them, as in Eagle.
                let r = ui.add(
                    TextEdit::singleline(&mut fv.line)
                        .id_salt(("font-line", size as u32))
                        .font(FontId::new(px, family.clone()))
                        .text_color(ink)
                        .frame(egui::Frame::NONE)
                        .clip_text(false)
                        .desired_width(f32::INFINITY),
                );
                if r.changed() {
                    fv.edited = true;
                }
            });
            ui.add_space(4.0 * fv.zoom);
        }
    }

    fn font_alphabet(&mut self, ui: &mut Ui, family: &FontFamily, specimen: Specimen, ink: Color32, muted: Color32, bg: Color32) {
        let fid = FontId::new(GLYPH * self.ws.font_view.zoom, family.clone());
        let groups = self.ws.font_view.glyphs.get_or_insert_with(|| {
            // Only what the font draws itself; other characters would show
            // egui's replacement box.
            ui.ctx().fonts_mut(|f| glyph_groups(specimen).into_iter().map(|(label, chars)| (label, chars.chars().filter(|c| f.has_glyph(&fid, *c)).collect::<String>())).filter(|(_, s)| !s.is_empty()).collect())
        });
        let cell = Vec2::splat((fid.size * 1.6).max(20.0));
        let mut hovered = None;
        for (label, chars) in groups.iter() {
            ui.label(RichText::new(*label).font(font(12.0)).color(muted));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                for c in chars.chars() {
                    let (r, resp) = ui.allocate_exact_size(cell, Sense::hover());
                    ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, muted.gamma_multiply(0.35)), egui::StrokeKind::Inside);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, c, fid.clone(), ink);
                    if resp.hovered() {
                        hovered = Some((c, r));
                    }
                }
            });
            ui.add_space(14.0);
        }
        // Hovering a character shows it large with its code point.
        if let Some((c, r)) = hovered {
            let size = Vec2::new(180.0, 200.0);
            let screen = ui.ctx().content_rect();
            let at = Pos2::new((r.center().x - size.x / 2.0).clamp(screen.left(), screen.right() - size.x), (r.top() - size.y - 6.0).max(screen.top()));
            let pop = Rect::from_min_size(at, size);
            let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("glyph-zoom")));
            p.rect_filled(pop, 8.0, bg);
            p.rect_stroke(pop, 8.0, Stroke::new(1.0, muted), egui::StrokeKind::Inside);
            p.text(pop.center() - vec2(0.0, 14.0), Align2::CENTER_CENTER, c, FontId::new(120.0, family.clone()), ink);
            p.text(Pos2::new(pop.center().x, pop.bottom() - 16.0), Align2::CENTER_CENTER, format!("U+{:04X}", c as u32), mono(11.0), muted);
        }
    }

    /// Activates or deactivates the font after asking, as Eagle's Activate
    /// button does (Eagle doesn't ask; falcon writes outside the library,
    /// so it confirms first). falcon writes only its own folder,
    /// `~/Library/Fonts/Falcon/`, never Eagle's `EagleApp/`.
    fn toggle_font(&mut self, lib: &Library, item: &Item, on: bool) {
        let Some(dir) = search::falcon_font_dir() else { return };
        let Some(file) = crate::font_install::file_name(item) else {
            self.flash("This font has no PostScript name to install it under");
            return;
        };
        let place = format!("~/Library/Fonts/Falcon/{file}");
        let (title, text) = if on {
            ("Activate Font", format!("Copy \u{201c}{}\u{201d} to {place}? macOS then offers it in every app, as fonts activated in Eagle are.", item.name))
        } else {
            ("Deactivate Font", format!("Remove {place}? The font stays in the library."))
        };
        let answer = rfd::MessageDialog::new().set_title(title).set_description(text).set_buttons(rfd::MessageButtons::OkCancel).show();
        if answer != rfd::MessageDialogResult::Ok {
            return;
        }
        let result = if on { crate::font_install::activate(&dir, item, &item.original_path(lib.root())).map(|_| ()) } else { crate::font_install::deactivate(&dir, item) };
        match result {
            Ok(()) => {
                self.ws.font_view.active = activation(item);
                self.ws.filter_ui.forget_font_keys();
                self.flash(if on { format!("Activated {}", item.name) } else { format!("Deactivated {}", item.name) });
            }
            Err(e) => self.flash(format!("Couldn't change the font: {e}")),
        }
    }

    /// Whether the viewer shows this font in the font viewer (digits rate
    /// there rather than zoom).
    pub(crate) fn font_viewer_open(&self) -> bool {
        self.ws.viewer.as_deref() == Some(self.ws.font_view.id.as_str()) && (self.ws.font_view.family.is_some() || self.ws.font_view.fallback)
    }
}

/// The set of characters the alphabet tab would show, for tests.
#[cfg(test)]
fn all_chars(s: Specimen) -> std::collections::HashSet<char> {
    glyph_groups(s).into_iter().flat_map(|(_, g)| g.chars().collect::<Vec<_>>()).collect()
}

/// Where `item` is activated, from falcon's font folder and Eagle's; `None`
/// where there are no such folders.
fn activation(item: &Item) -> Option<crate::font_install::Activation> {
    let falcon = search::falcon_font_dir()?;
    Some(crate::font_install::activation(&falcon, search::eagle_font_dir().as_deref(), item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_egui_cannot_read_are_refused() {
        assert!(!egui_can_read(b"not a font"));
        assert!(!egui_can_read(&[]));
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") else { return };
        assert!(egui_can_read(&data));
        // A registered font lays out text in its own family from the next pass.
        let ctx = egui::Context::default();
        let family = FontFamily::Name("falcon-font-test".into());
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.label("before");
            ui.ctx().add_font(FontInsert {
                name: "falcon-font-test".into(),
                data: FontData::from_owned(data.clone()),
                families: vec![InsertFontFamily { family: family.clone(), priority: egui::FontPriority::Highest }],
            });
        });
        out.textures_delta.clear();
        let mut width = 0.0;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            assert!(ui.ctx().fonts(|f| f.definitions().families.contains_key(&family)));
            width = ui.painter().layout_no_wrap("Sphinx".into(), FontId::new(48.0, family.clone()), Color32::WHITE).size().x;
            // Arial has Latin letters but no kana.
            let fid = FontId::new(20.0, family.clone());
            assert!(ui.ctx().fonts_mut(|f| f.has_glyph(&fid, 'A') && !f.has_glyph(&fid, 'あ')));
        });
        out.textures_delta.clear();
        assert!(width > 50.0, "{width}");
    }

    #[test]
    fn zoom_steps_like_eagles_slider() {
        assert_eq!(zoom_step(1.0, true), 1.5);
        assert_eq!(zoom_step(1.0, false), 0.5);
        assert_eq!(zoom_step(0.5, false), 0.5);
        assert_eq!(zoom_step(9.8, true), 10.0);
    }

    #[test]
    fn themes() {
        assert_eq!(FontTheme::Auto.colors(), None);
        let (bg, ink) = FontTheme::Yellow.colors().unwrap();
        assert!(bg.r() > 200 && bg.g() > 200 && bg.b() < 100 && ink.r() < 60);
        let (bg, ink) = FontTheme::Purple.colors().unwrap();
        assert!(bg.b() > bg.r() && ink.r() > 200);
        assert_eq!(FontTheme::ALL.len(), 5);
    }

    #[test]
    fn samples_and_glyphs_follow_the_language() {
        for (code, _) in SAMPLE_LANGUAGES {
            let s = [Specimen::Latin, Specimen::Japanese, Specimen::Korean, Specimen::SimplifiedChinese, Specimen::TraditionalChinese]
                .into_iter()
                .find(|s| language_code(*s) == code)
                .expect("every language has a specimen");
            let (heading, body, line) = samples(s);
            assert!(!heading.is_empty() && body.contains("\n\n") && line.contains("0123"));
        }
        assert!(all_chars(Specimen::Japanese).contains(&'あ') && all_chars(Specimen::Japanese).contains(&'ア'));
        assert!(all_chars(Specimen::Korean).contains(&'가') && all_chars(Specimen::Korean).contains(&'흐'));
        assert!(!all_chars(Specimen::Latin).contains(&'あ'));
        assert!(all_chars(Specimen::Latin).contains(&'é'));
        assert_eq!(glyph_groups(Specimen::Korean).last().unwrap().1.chars().count(), 95);
    }

    #[test]
    fn info_rows_skip_empty_values() {
        let info = FontInfo { postscript: "A-B".into(), family: "A".into(), glyphs: Some(12), ..Default::default() };
        let rows = info_rows(&info);
        let labels: Vec<_> = rows.iter().map(|r| r.0).collect();
        assert_eq!(labels, ["PostScript Name", "Family", "Glyphs"]);
        assert!(rows[0].2 && rows[1].2 && !rows[2].2, "names can become the file name; counts can't");
    }
}

