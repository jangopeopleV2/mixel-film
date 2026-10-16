//! File ▸ Export ▸ AAF… and OMF…: the export settings dialogs. OK asks for the file name (the
//! host's save panel) and runs `file.exportAaf` / `file.exportOmf` with the chosen settings; with
//! parameters (`path` …) the menu commands run directly.
//!
//! Automation ids (`<p>` is `aafExport` or `omfExport`): `<p>.mixdownVideo` (AAF),
//! `<p>.breakoutToMono`, `<p>.audio.<embedded|separate|linked>` (linked: AAF only),
//! `<p>.audioFormat.<wav|aiff|mxf>` (mxf: AAF only), `<p>.sampleRate.<rate>`, `<p>.bitDepth.<16|24>`, `<p>.trimAudio`,
//! `<p>.handles` (frames), `<p>.renderAudioEffects`, `<p>.title` (OMF), `<p>.ok`, `<p>.cancel`.

use crate::widgets::CenteredDialog as _;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

use crate::FilmcraftApp;
use crate::native_dialogs::FileDialog;

#[derive(Clone, Debug, PartialEq)]
struct Draft {
    source: ExportSource,
    path: Option<String>,
    omf: bool,
    title: String,
    mixdown_video: bool,
    breakout: bool,
    audio: &'static str,
    format: &'static str,
    sample_rate: u32,
    bits: u16,
    trim: bool,
    handles: i64,
    render_effects: bool,
}

#[derive(Clone, Debug)]
struct ExportSource {
    project: Arc<filmcraft_project::Project>,
    sequence: filmcraft_project::ItemId,
}

impl PartialEq for ExportSource {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.project, &other.project) && self.sequence == other.sequence
    }
}

impl ExportSource {
    fn current(&self, app: &FilmcraftApp) -> bool {
        Arc::ptr_eq(&self.project, &app.session.project) && app.session.project.sequence(self.sequence).is_some()
    }
}

impl Draft {
    fn params(&self, path: &str) -> Value {
        let mut params = json!({
            "path": path,
            "sequence": self.source.sequence.0,
            "breakoutToMono": self.breakout,
            "audio": self.audio,
            "audioFormat": self.format,
            "sampleRate": self.sample_rate,
            "bitDepth": self.bits,
            "trimAudio": self.trim && self.audio != "linked",
            "handles": self.handles,
            "renderAudioEffects": self.render_effects && self.audio != "linked",
        });
        if self.omf {
            params["title"] = json!(self.title);
        } else {
            params["mixdownVideo"] = json!(self.mixdown_video);
        }
        params
    }
}

/// The UI asks the host for permission; the engine independently rechecks actual native scope.
/// Keeping this small policy as a dependency lets headless hosts exercise the real continuation.
#[derive(Clone)]
struct FolderAccess {
    required: bool,
    granted: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
}

impl FolderAccess {
    fn native(app: &FilmcraftApp) -> Self {
        Self {
            required: crate::macos_mas::enabled() && !app.session.services.export_in_memory(),
            granted: Arc::new(|path| crate::macos_mas::directory_granted(path)),
        }
    }
}

fn restore(app: &mut FilmcraftApp, ctx: &egui::Context, draft: Draft, message: &str) {
    app.ui.status = message.to_string();
    if draft.source.current(app) {
        ctx.data_mut(|m| m.insert_temp(draft_id(), Some(draft)));
    }
}

fn execute_export(app: &mut FilmcraftApp, ctx: &egui::Context, draft: Draft) {
    if !draft.source.current(app) {
        app.ui.status = "The project changed while choosing the export destination. Reopen the export settings.".into();
        return;
    }
    let Some(path) = draft.path.as_deref() else { return };
    let cmd = if draft.omf { "file.exportOmf" } else { "file.exportAaf" };
    match app.session.execute(cmd, draft.params(path)) {
        Ok(v) => app.ui.status = tlf!("Exported {path}", path = v["path"].as_str().unwrap_or_default()),
        Err(e) => restore(app, ctx, draft, &e.to_string()),
    }
}

fn after_save(app: &mut FilmcraftApp, ctx: egui::Context, draft: Draft, access: FolderAccess) {
    if !draft.source.current(app) {
        app.ui.status = "The project changed while choosing the export destination. Reopen the export settings.".into();
        return;
    }
    let Some(path) = draft.path.as_deref() else { return };
    let folder = if access.required { filmcraft_engine::aaf_omf::companion_folder(path, &draft.params(path), draft.omf) } else { Ok(None) };
    let folder = match folder {
        Ok(folder) => folder,
        Err(e) => return restore(app, &ctx, draft, &e.to_string()),
    };
    let Some(parent) = folder.filter(|folder| !(access.granted)(folder)) else {
        return execute_export(app, &ctx, draft);
    };
    let dialog = FileDialog::Folder { at: Some(parent.to_string_lossy().into_owned()) };
    if app.native_dialog_open() {
        return restore(app, &ctx, draft, tl!("A file dialog is already open"));
    }
    if (crate::macos_mas::enabled() && app.hooks.run_dialog.is_none()) || !dialog.available(&app.hooks) {
        return restore(app, &ctx, draft, tl!("The native file dialog service is unavailable. Reopen MIXEL Film and try again."));
    }
    app.ui.status = filmcraft_engine::aaf_omf::COMPANION_FOLDER_REQUIRED.into();
    app.pick_ui(dialog, move |app, paths| {
        let message = filmcraft_engine::aaf_omf::COMPANION_FOLDER_REQUIRED;
        let Some(folder) = paths.first() else { return restore(app, &ctx, draft, message) };
        let Some(path) = draft.path.as_deref() else { return };
        // A moved bookmark can change both the saved filename and its containing folder.
        let resolved = crate::macos_mas::resolve(path);
        let selected = crate::macos_mas::resolve(folder);
        let (path, selected) = match (resolved, selected) {
            (Ok(path), Ok(selected)) => (path, selected),
            (Err(e), _) | (_, Err(e)) => return restore(app, &ctx, draft, &e),
        };
        let valid = selected.is_absolute()
            && !selected.components().any(|c| matches!(c, std::path::Component::ParentDir))
            && path.parent().is_some_and(|parent| parent.starts_with(&selected) && (access.granted)(parent));
        if !valid {
            return restore(app, &ctx, draft, message);
        }
        let mut draft = draft;
        draft.path = Some(path.to_string_lossy().into_owned());
        execute_export(app, &ctx, draft);
    });
}

fn choose_destination(app: &mut FilmcraftApp, ctx: egui::Context, draft: Draft, access: FolderAccess) {
    if app.native_dialog_open() {
        return restore(app, &ctx, draft, tl!("A file dialog is already open"));
    }
    if draft.path.is_some() {
        return after_save(app, ctx, draft, access);
    }
    let (ext, filter) = if draft.omf { ("omf", "OMF") } else { ("aaf", "AAF") };
    let stem = if draft.title.is_empty() { "Sequence".to_string() } else { draft.title.replace(['/', '\\', ':'], "_") };
    let dialog = FileDialog::save_as(filter, &[ext], &format!("{stem}.{ext}"));
    if !(crate::macos_mas::enabled() && app.hooks.run_dialog.is_none()) && dialog.available(&app.hooks) {
        app.pick_ui(dialog, move |app, paths| {
            let Some(path) = paths.into_iter().next() else { return };
            let mut draft = draft;
            draft.path = Some(path);
            after_save(app, ctx, draft, access);
        });
    } else {
        restore(app, &ctx, draft, "no save dialog available: run the command with a `path`");
    }
}

fn draft_id() -> egui::Id {
    egui::Id::new("interchange-export-draft")
}

const RATES: [u32; 4] = [32_000, 44_100, 48_000, 96_000];

/// Open the AAF or OMF settings dialog (`id` is the menu command).
pub fn open(app: &mut FilmcraftApp, ctx: &egui::Context, id: &str) -> Result<Value, String> {
    let omf = id == "file.exportOmf";
    let seq = app.session.state.active_sequence.and_then(|s| app.session.project.item(s));
    let title = seq.map(|i| i.name.clone()).unwrap_or_default();
    let sr = app.session.active_sequence().map(|q| q.settings.sample_rate).filter(|r| RATES.contains(r)).unwrap_or(48_000);
    let sequence = app.session.state.active_sequence.ok_or_else(|| filmcraft_engine::EngineError::NoSequence.to_string())?;
    let d = Draft {
        source: ExportSource { project: app.session.project.clone(), sequence },
        path: None,
        omf,
        title,
        mixdown_video: false,
        breakout: false,
        audio: "embedded",
        format: "wav",
        sample_rate: sr,
        bits: 16,
        trim: omf,
        handles: if omf { 30 } else { 0 },
        render_effects: false,
    };
    ctx.data_mut(|m| m.insert_temp(draft_id(), Some(d)));
    Ok(json!({"dialog": if omf { "omfExport" } else { "aafExport" }}))
}

/// Whether the dialog is open.
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|m| m.get_temp::<Option<Draft>>(draft_id())).flatten().is_some()
}

pub fn show(app: &mut FilmcraftApp, ctx: &egui::Context) {
    let Some(Some(mut d)) = ctx.data(|m| m.get_temp::<Option<Draft>>(draft_id())) else { return };
    crate::widgets::revert_drag_on_escape(ctx, egui::Id::new("interchange-export-before-drag"), &mut d);
    let p = if d.omf { "omfExport" } else { "aafExport" };
    let mut close = false;
    let mut apply = false;
    let mut elems: Vec<(String, egui::Rect, String)> = Vec::new();
    let title = if d.omf { "OMF Export Settings" } else { "AAF Export Settings" };
    let shown = if d.omf { tl!("OMF Export Settings") } else { tl!("AAF Export Settings") };
    egui::Window::new(shown).id(egui::Id::new(title)).collapsible(false).resizable(false).centered(ctx).show(ctx, |ui| {
        ui.set_min_width(380.0);
        egui::Grid::new("interchange-export-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            if d.omf {
                ui.label(tl!("OMF Title:"));
                let r = ui.text_edit_singleline(&mut d.title);
                elems.push((format!("{p}.title"), r.rect, d.title.clone()));
                ui.end_row();
            } else {
                ui.label("");
                let r = ui.checkbox(&mut d.mixdown_video, tl!("Mixdown video"));
                elems.push((format!("{p}.mixdownVideo"), r.rect, d.mixdown_video.to_string()));
                ui.end_row();
            }
            ui.label(tl!("Audio:"));
            ui.horizontal(|ui| {
                let modes: Vec<(&'static str, &str)> = if d.omf {
                    vec![("embedded", tl!("Encapsulate")), ("separate", tl!("Separate Audio"))]
                } else {
                    vec![("embedded", tl!("Embed")), ("separate", tl!("Separate Files")), ("linked", tl!("Link to Media"))]
                };
                for (k, label) in &modes {
                    let r = ui.radio(d.audio == *k, *label);
                    if r.clicked() {
                        d.audio = k;
                    }
                    elems.push((format!("{p}.audio.{k}"), r.rect, label.to_string()));
                }
            });
            ui.end_row();
            ui.label(tl!("Audio File Format:"));
            ui.horizontal(|ui| {
                let formats: &[(&'static str, &str)] =
                    if d.omf { &[("wav", "Broadcast Wave"), ("aiff", "AIFF")] } else { &[("wav", "Broadcast Wave"), ("aiff", "AIFF"), ("mxf", "OP-Atom MXF")] };
                for &(k, label) in formats {
                    let r = ui.add_enabled(d.audio == "separate", egui::RadioButton::new(d.format == k, label));
                    if r.clicked() {
                        d.format = k;
                    }
                    elems.push((format!("{p}.audioFormat.{k}"), r.rect, label.to_string()));
                }
            });
            ui.end_row();
            ui.label(tl!("Sample Rate:"));
            ui.horizontal(|ui| {
                for r in RATES {
                    let b = ui.radio(d.sample_rate == r, format!("{r}"));
                    if b.clicked() {
                        d.sample_rate = r;
                    }
                    elems.push((format!("{p}.sampleRate.{r}"), b.rect, format!("{r} Hz")));
                }
            });
            ui.end_row();
            ui.label(tl!("Bits per Sample:"));
            ui.horizontal(|ui| {
                for b in [16u16, 24] {
                    let r = ui.radio(d.bits == b, format!("{b}"));
                    if r.clicked() {
                        d.bits = b;
                    }
                    elems.push((format!("{p}.bitDepth.{b}"), r.rect, format!("{b}-bit")));
                }
            });
            ui.end_row();
            ui.label(tl!("Rendering:"));
            let linked = d.audio == "linked";
            let r = ui.add_enabled(!linked, egui::Checkbox::new(&mut d.trim, tl!("Trim audio files")));
            elems.push((format!("{p}.trimAudio"), r.rect, d.trim.to_string()));
            ui.end_row();
            ui.label(tl!("Handle Frames:"));
            let r = ui.add_enabled(d.trim && !linked, crate::widgets::number_drag(&mut d.handles).range(0..=10_000));
            elems.push((format!("{p}.handles"), r.rect, d.handles.to_string()));
            ui.end_row();
            ui.label("");
            let r = ui.add_enabled(!linked, egui::Checkbox::new(&mut d.render_effects, tl!("Render audio clip effects")));
            elems.push((format!("{p}.renderAudioEffects"), r.rect, d.render_effects.to_string()));
            ui.end_row();
            ui.label("");
            let r = ui.checkbox(&mut d.breakout, tl!("Breakout to mono"));
            elems.push((format!("{p}.breakoutToMono"), r.rect, d.breakout.to_string()));
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let c = ui.button(tl!("Cancel"));
            elems.push((format!("{p}.cancel"), c.rect, "Cancel".into()));
            if c.clicked() {
                close = true;
            }
            let o = ui.add(egui::Button::new(egui::RichText::new(tl!("OK")).color(egui::Color32::WHITE)).fill(app.tokens.accent));
            elems.push((format!("{p}.ok"), o.rect, "OK".into()));
            if o.clicked() {
                apply = true;
            }
        });
    });
    for (id, r, l) in elems {
        app.auto.add(&id, r, &l);
    }
    if crate::widgets::escape_closes(ctx) {
        close = true;
    }
    if apply {
        // Close before starting the continuation: a retry must not overwrite restored settings.
        ctx.data_mut(|m| m.insert_temp::<Option<Draft>>(draft_id(), None));
        let access = FolderAccess::native(app);
        choose_destination(app, ctx.clone(), d, access);
        return;
    }
    ctx.data_mut(|m| m.insert_temp(draft_id(), if close { None } else { Some(d) }));
}

#[cfg(test)]
#[path = "interchange_export_permissions_tests.rs"]
mod macos_mas_output_tests;
