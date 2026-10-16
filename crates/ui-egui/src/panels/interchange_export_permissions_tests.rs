//! The production Save → Folder continuation with a real in-memory export host and tone clip.
//! No filesystem output, native panel or synthetic security-scoped bookmark is used.

use super::*;
use filmcraft_engine::{Services, Session};
use filmcraft_media::{Generator, MediaSource, generators::GeneratorSource};
use filmcraft_project::{ItemId, ItemKind, Label, MediaClip, MediaRef, SequenceSettings};
use filmcraft_time::{FrameRate, TICKS_PER_SECOND, Tick};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, mpsc};

#[derive(Default)]
struct MemoryFiles {
    writes: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Services for MemoryFiles {
    fn read_file(&self, _: &str) -> std::io::Result<Vec<u8>> {
        Err(std::io::ErrorKind::NotFound.into())
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        self.writes.lock().unwrap().push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }

    fn export_in_memory(&self) -> bool {
        true
    }
}

struct Flow {
    app: FilmcraftApp,
    ctx: egui::Context,
    files: Arc<MemoryFiles>,
    grants: Arc<Mutex<Vec<PathBuf>>>,
    requests: Rc<RefCell<Vec<FileDialog>>>,
    replies: Rc<RefCell<VecDeque<mpsc::Sender<Vec<String>>>>>,
    original: ItemId,
    other: ItemId,
    folder: PathBuf,
    omf: bool,
}

impl Flow {
    fn new(omf: bool) -> Self {
        let files = Arc::new(MemoryFiles::default());
        let mut session = Session::new(files.clone());
        let mut project = (*session.project).clone();
        let settings = SequenceSettings { width: 32, height: 18, frame_rate: FrameRate::FPS_24, ..Default::default() };
        let original = project.new_sequence("Captured original", settings.clone(), 0, 1, None);
        let other = project.new_sequence("Unrelated empty sequence", settings, 0, 1, None);
        let tone = Arc::new(GeneratorSource::new(Generator::Tone { hz: 440.0, db: -12.0 }, 32, 18, FrameRate::FPS_24, Tick(TICKS_PER_SECOND / 4)));
        let folder = std::env::current_dir().unwrap().join("__mixel_film_interchange_permissions_fixture__").join("exports");
        let item = project.add_item(
            "Synthetic stereo tone",
            Label::Iris,
            ItemKind::Media(MediaClip {
                media: MediaRef::File { path: folder.with_file_name("source-tone.wav").to_string_lossy().into_owned() },
                info: tone.info().clone(),
                interpret: Default::default(),
                mark_in: None,
                mark_out: None,
                markers: vec![],
                offline: false,
                proxy: None,
                identity: None,
            }),
            None,
        );
        session.project = Arc::new(project);
        session.state.active_sequence = Some(original);
        session.state.open_sequences = vec![original, other];
        session.media.insert(item, tone);
        session.execute("timeline.place", json!({"item": item.0, "track": "A1", "seconds": 0.0})).unwrap();
        assert_eq!(session.project.sequence(original).unwrap().audio_tracks[0].items.len(), 1);
        assert!(session.project.sequence(other).unwrap().audio_tracks[0].items.is_empty());
        let mut app = FilmcraftApp::new(session);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let replies = Rc::new(RefCell::new(VecDeque::new()));
        let (asked, held) = (requests.clone(), replies.clone());
        app.hooks.run_dialog = Some(Box::new(move |request| {
            let (tx, rx) = mpsc::channel();
            asked.borrow_mut().push(request);
            held.borrow_mut().push_back(tx);
            rx
        }));
        Self { app, ctx: egui::Context::default(), files, grants: Arc::default(), requests, replies, original, other, folder, omf }
    }

    fn access(&self) -> FolderAccess {
        let grants = self.grants.clone();
        FolderAccess { required: true, granted: Arc::new(move |path| grants.lock().unwrap().iter().any(|folder| path.starts_with(folder))) }
    }

    fn draft(&mut self) -> Draft {
        open(&mut self.app, &self.ctx, if self.omf { "file.exportOmf" } else { "file.exportAaf" }).unwrap();
        let mut draft = self.take_draft();
        assert_eq!(draft.source.sequence, self.original);
        draft.title = "Retained title".into();
        draft.audio = "separate";
        draft.format = "wav";
        draft.breakout = true;
        draft.sample_rate = 32_000;
        draft.bits = 24;
        draft.trim = true;
        draft.handles = 2;
        draft.render_effects = false;
        draft
    }

    fn take_draft(&self) -> Draft {
        self.ctx.data_mut(|memory| {
            let draft = memory.get_temp::<Option<Draft>>(draft_id()).flatten().expect("the production dialog must restore its complete draft");
            memory.insert_temp::<Option<Draft>>(draft_id(), None);
            draft
        })
    }

    fn start(&mut self, draft: Draft) {
        let access = self.access();
        choose_destination(&mut self.app, self.ctx.clone(), draft, access);
        assert!(
            matches!(self.requests.borrow().as_slice(), [FileDialog::SaveAs { name, exts, .. }] if name == &format!("Retained title.{}", self.ext()) && exts == &[self.ext().to_string()])
        );
        assert!(self.app.native_dialog_open());
        self.zero_writes();
    }

    fn ext(&self) -> &'static str {
        if self.omf { "omf" } else { "aaf" }
    }

    fn path(&self) -> String {
        self.folder.join(format!("chosen.{}", self.ext())).to_string_lossy().into_owned()
    }

    fn answer(&mut self, paths: Vec<String>) {
        self.replies.borrow_mut().pop_front().expect("a real pending host receiver").send(paths).unwrap();
        self.app.poll_native_dialog(&self.ctx);
    }

    fn save(&mut self) {
        self.answer(vec![self.path()]);
    }

    fn folder_pending(&self) {
        assert!(matches!(self.requests.borrow().last(), Some(FileDialog::Folder { at: Some(at) }) if Path::new(at) == self.folder));
        assert!(self.app.native_dialog_open());
        self.zero_writes();
    }

    fn choose(&mut self, folder: PathBuf, retain: bool) {
        // The actual host retains only an explicit native selection, then sends its answer.
        if retain {
            self.grants.lock().unwrap().push(folder.clone());
        }
        self.answer(vec![folder.to_string_lossy().into_owned()]);
    }

    fn zero_writes(&self) {
        assert!(self.files.writes.lock().unwrap().is_empty(), "neither the document nor any companion may be written before permission");
    }

    fn completed(&self) {
        assert!(!self.app.native_dialog_open());
        assert!(!is_open(&self.ctx), "success must not restore an error/retry draft: {}", self.app.ui.status);
        assert!(self.app.ui.status.contains("chosen."), "{}", self.app.ui.status);
        let writes = self.files.writes.lock().unwrap();
        assert_eq!(writes.len(), 3, "real stereo breakout must produce two mono WAVs and the interchange document: {}", self.app.ui.status);
        let document = writes.iter().find(|(path, _)| path == &self.path()).expect("the requested document path");
        assert!(!document.1.is_empty());
        let companions: Vec<_> = writes.iter().filter(|(path, _)| path.ends_with(".wav")).collect();
        assert_eq!(companions.len(), 2);
        assert_ne!(companions[0].0, companions[1].0);
        for (path, bytes) in companions {
            assert!(Path::new(path).starts_with(&self.folder));
            let (pcm, channels, rate, bits) = filmcraft_interchange::wav::parse_wav(bytes).expect("production encoded WAV essence");
            assert_eq!((channels, rate, bits), (1, 32_000, 24));
            assert!(!pcm.is_empty() && pcm.iter().any(|byte| *byte != 0), "the synthetic tone must survive actual audio rendering");
        }
        let format = if self.omf { filmcraft_interchange::Format::Omf } else { filmcraft_interchange::Format::Aaf };
        let (imported, _) = filmcraft_interchange::import(&document.1, format, None).expect("actual interchange document must parse");
        assert_eq!(imported.sequences.len(), 1);
        let sequence = imported.project.sequence(imported.sequences[0]).unwrap();
        assert!(sequence.audio_tracks.iter().any(|track| !track.items.is_empty()), "the captured source contains audio; the unrelated sequence is empty");
    }
}

#[test]
fn separate_aaf_and_omf_wait_for_folder_before_real_audio_fanout() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        flow.start(draft);
        flow.save();
        flow.folder_pending();
        assert!(flow.grants.lock().unwrap().is_empty(), "a Save answer must not infer a containing-folder grant");
        // Empty polling must not run the continuation or open another dialog.
        flow.app.poll_native_dialog(&flow.ctx);
        flow.zero_writes();
        assert_eq!(flow.requests.borrow().len(), 2);
        flow.choose(flow.folder.clone(), true);
        flow.completed();
    }
}

#[test]
fn folder_cancel_restores_filename_and_every_setting_for_retry_without_second_save() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut expected = flow.draft();
        flow.start(expected.clone());
        flow.save();
        expected.path = Some(flow.path());
        flow.folder_pending();
        flow.answer(vec![]);
        flow.zero_writes();
        assert!(!flow.app.native_dialog_open());
        assert_eq!(flow.app.ui.status, filmcraft_engine::aaf_omf::COMPANION_FOLDER_REQUIRED);
        let restored = flow.take_draft();
        assert_eq!(restored, expected);
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), restored, access);
        flow.folder_pending();
        assert_eq!(flow.requests.borrow().iter().filter(|request| matches!(request, FileDialog::SaveAs { .. })).count(), 1);
        flow.choose(flow.folder.clone(), true);
        flow.completed();
    }
}

#[test]
fn wrong_or_prefix_sibling_folder_cannot_authorize_an_export() {
    for omf in [false, true] {
        for name in ["unrelated", "exports-unrelated"] {
            let mut flow = Flow::new(omf);
            let mut expected = flow.draft();
            flow.start(expected.clone());
            flow.save();
            expected.path = Some(flow.path());
            flow.folder_pending();
            // A separate correct grant does not turn an unrelated dialog answer into consent.
            flow.grants.lock().unwrap().push(flow.folder.clone());
            flow.choose(flow.folder.with_file_name(name), true);
            flow.zero_writes();
            assert_eq!(flow.take_draft(), expected);
            assert_eq!(flow.app.ui.status, filmcraft_engine::aaf_omf::COMPANION_FOLDER_REQUIRED);
        }
    }
}

#[test]
fn matching_folder_without_retained_scope_restores_the_draft_and_never_writes() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut expected = flow.draft();
        flow.start(expected.clone());
        flow.save();
        expected.path = Some(flow.path());
        flow.folder_pending();
        flow.choose(flow.folder.clone(), false);
        flow.zero_writes();
        assert_eq!(flow.take_draft(), expected);
    }
}

#[test]
fn disconnected_folder_reply_restores_the_complete_retry_draft_without_writes() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut expected = flow.draft();
        flow.start(expected.clone());
        flow.save();
        expected.path = Some(flow.path());
        flow.folder_pending();
        drop(flow.replies.borrow_mut().pop_front().unwrap());
        flow.app.poll_native_dialog(&flow.ctx);
        flow.zero_writes();
        assert!(!flow.app.native_dialog_open());
        assert_eq!(flow.take_draft(), expected);
    }
}

#[test]
fn an_explicit_ancestor_folder_covers_the_document_and_all_audio_companions() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        flow.start(draft);
        flow.save();
        flow.folder_pending();
        flow.choose(flow.folder.parent().unwrap().to_path_buf(), true);
        flow.completed();
    }
}

#[test]
fn an_existing_folder_grant_exports_after_save_without_another_panel() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        flow.grants.lock().unwrap().push(flow.folder.clone());
        let draft = flow.draft();
        flow.start(draft);
        flow.save();
        assert_eq!(flow.requests.borrow().len(), 1);
        flow.completed();
    }
}

#[test]
fn an_active_sequence_switch_keeps_the_original_audio_source_through_both_panels() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        let project = flow.app.session.project.clone();
        flow.start(draft);
        flow.app.session.execute("sequence.open", json!({"item": flow.other.0})).unwrap();
        assert!(Arc::ptr_eq(&project, &flow.app.session.project));
        flow.save();
        flow.folder_pending();
        flow.choose(flow.folder.clone(), true);
        flow.completed();
        assert_eq!(flow.app.session.state.active_sequence, Some(flow.other), "export must not switch the active editor sequence");
    }
}

#[test]
fn replacing_the_project_while_save_is_open_refuses_export_and_folder_prompt() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        flow.start(draft);
        flow.app.session.project = Arc::new((*flow.app.session.project).clone());
        flow.save();
        flow.zero_writes();
        assert_eq!(flow.requests.borrow().len(), 1);
        assert!(!flow.app.native_dialog_open());
        assert!(!is_open(&flow.ctx), "a stale project draft must not be restored");
        assert!(flow.app.ui.status.contains("project changed"));
    }
}

#[test]
fn replacing_the_project_while_folder_is_open_refuses_all_companion_writes() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        flow.start(draft);
        flow.save();
        flow.folder_pending();
        flow.app.session.project = Arc::new((*flow.app.session.project).clone());
        flow.choose(flow.folder.clone(), true);
        flow.zero_writes();
        assert!(!is_open(&flow.ctx));
        assert!(flow.app.ui.status.contains("project changed"));
    }
}

#[test]
fn retrying_a_retained_filename_can_use_a_new_grant_without_reopening_save() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let draft = flow.draft();
        flow.start(draft);
        flow.save();
        flow.folder_pending();
        flow.answer(vec![]);
        let restored = flow.take_draft();
        flow.grants.lock().unwrap().push(flow.folder.clone());
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), restored, access);
        assert_eq!(flow.requests.borrow().len(), 2, "neither another Save nor another Folder is necessary");
        flow.completed();
    }
}

#[test]
fn cancel_or_disconnect_at_save_never_prompts_for_folder_or_writes() {
    for omf in [false, true] {
        for disconnect in [false, true] {
            let mut flow = Flow::new(omf);
            let draft = flow.draft();
            flow.start(draft);
            if disconnect {
                drop(flow.replies.borrow_mut().pop_front().unwrap());
                flow.app.poll_native_dialog(&flow.ctx);
            } else {
                flow.answer(vec![]);
            }
            flow.zero_writes();
            assert!(!flow.app.native_dialog_open());
            assert_eq!(flow.requests.borrow().len(), 1);
        }
    }
}

#[test]
fn plain_embedded_audio_keeps_the_single_file_save_flow() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut draft = flow.draft();
        draft.audio = "embedded";
        flow.start(draft);
        flow.save();
        assert!(!flow.app.native_dialog_open());
        assert_eq!(flow.requests.borrow().len(), 1);
        let writes = flow.files.writes.lock().unwrap();
        assert_eq!(writes.len(), 1, "embedded audio must stay inside the chosen AAF/OMF: {}", flow.app.ui.status);
        assert_eq!(writes[0].0, flow.path());
        assert!(!writes[0].1.is_empty());
    }
}

#[test]
fn a_busy_native_dialog_preserves_unsaved_export_settings_without_another_request() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let expected = flow.draft();
        flow.app.pick_ui(FileDialog::open_files(&["wav"]), |_, _| {});
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
        flow.zero_writes();
        assert_eq!(flow.requests.borrow().len(), 1);
        assert!(flow.app.native_dialog_open(), "the unrelated native request must stay intact");
        assert_eq!(flow.take_draft(), expected);
        assert!(flow.app.ui.status.contains("already open"));
        flow.answer(vec![]);
        flow.zero_writes();
    }
}

#[test]
fn a_busy_folder_request_preserves_the_saved_filename_until_retry() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut expected = flow.draft();
        expected.path = Some(flow.path());
        flow.app.pick_ui(FileDialog::open_files(&["wav"]), |_, _| {});
        let access = flow.access();
        after_save(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
        flow.zero_writes();
        assert_eq!(flow.requests.borrow().len(), 1);
        assert!(flow.app.native_dialog_open());
        let restored = flow.take_draft();
        assert_eq!(restored, expected);
        flow.answer(vec![]);
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), restored, access);
        flow.folder_pending();
        assert!(!flow.requests.borrow().iter().any(|request| matches!(request, FileDialog::SaveAs { .. })));
        flow.choose(flow.folder.clone(), true);
        flow.completed();
    }
}

#[test]
fn an_unavailable_folder_service_preserves_path_and_settings_for_a_later_retry() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut expected = flow.draft();
        expected.path = Some(flow.path());
        let host = flow.app.hooks.run_dialog.take();
        let access = flow.access();
        after_save(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
        flow.zero_writes();
        assert!(!flow.app.native_dialog_open());
        assert!(flow.requests.borrow().is_empty());
        let restored = flow.take_draft();
        assert_eq!(restored, expected);
        assert!(flow.app.ui.status.contains("unavailable"));
        flow.app.hooks.run_dialog = host;
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), restored, access);
        flow.folder_pending();
        assert_eq!(flow.requests.borrow().len(), 1);
        assert!(!flow.requests.borrow().iter().any(|request| matches!(request, FileDialog::SaveAs { .. })));
        flow.choose(flow.folder.clone(), true);
        flow.completed();
    }
}

#[test]
fn an_unavailable_save_service_restores_the_original_settings_without_io() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let expected = flow.draft();
        flow.app.hooks.run_dialog = None;
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
        flow.zero_writes();
        assert!(flow.requests.borrow().is_empty());
        assert!(!flow.app.native_dialog_open());
        assert_eq!(flow.take_draft(), expected);
        assert!(flow.app.ui.status.contains("no save dialog"));
    }
}

#[test]
fn legacy_only_hooks_preserve_mas_retry_state_and_keep_ordinary_host_behavior() {
    for omf in [false, true] {
        for saved in [false, true] {
            let mut flow = Flow::new(omf);
            let mut expected = flow.draft();
            if saved {
                expected.path = Some(flow.path());
            }
            flow.app.hooks.run_dialog = None;
            let calls = Rc::new(RefCell::new(Vec::new()));
            let (save_calls, folder_calls) = (calls.clone(), calls.clone());
            let (path, folder, grants) = (flow.path(), flow.folder.clone(), flow.grants.clone());
            flow.app.hooks.pick_save_as = Some(Box::new(move |_, _, _| {
                save_calls.borrow_mut().push("save");
                Some(path.clone())
            }));
            flow.app.hooks.pick_folder = Some(Box::new(move || {
                folder_calls.borrow_mut().push("folder");
                grants.lock().unwrap().push(folder.clone());
                Some(folder.to_string_lossy().into_owned())
            }));
            let access = flow.access();
            if saved {
                after_save(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
            } else {
                choose_destination(&mut flow.app, flow.ctx.clone(), expected.clone(), access);
            }
            flow.zero_writes();
            if crate::macos_mas::enabled() {
                assert!(calls.borrow().is_empty(), "MAS must never enter a legacy blocking panel");
                assert!(!flow.app.native_dialog_open());
                assert_eq!(flow.take_draft(), expected);
                assert!(flow.app.ui.status.contains(if saved { "unavailable" } else { "no save dialog" }));
            } else {
                // Ordinary hosts retain their synchronous-picker/deferred-continuation contract.
                assert!(flow.app.native_dialog_open());
                flow.app.poll_native_dialog(&flow.ctx);
                flow.app.poll_native_dialog(&flow.ctx);
                flow.completed();
                assert_eq!(*calls.borrow(), if saved { vec!["folder"] } else { vec!["save", "folder"] });
            }
        }
    }
}

#[test]
fn a_retained_filename_and_directory_grant_need_no_dialog_service_on_retry() {
    for omf in [false, true] {
        let mut flow = Flow::new(omf);
        let mut draft = flow.draft();
        draft.path = Some(flow.path());
        flow.grants.lock().unwrap().push(flow.folder.clone());
        flow.app.hooks.run_dialog = None;
        let access = flow.access();
        choose_destination(&mut flow.app, flow.ctx.clone(), draft, access);
        assert!(flow.requests.borrow().is_empty());
        flow.completed();
    }
}
