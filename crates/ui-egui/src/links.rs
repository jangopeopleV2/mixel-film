//! Community and project links (Help menu, About dialog, header project links, Home screen).
//!
//! MIXEL Film exposes separate company, product project and source repository links.

/// The app's short name, used in the website and repository URLs.
pub const APP: &str = "filmcraft";

/// The MIXEL company website.
pub const WEBSITE: &str = "https://www.mixel.ch";
/// MIXEL Film's project page.
pub const APP_PAGE: &str = "https://github.com/jangopeopleV2/mixel-film";
/// MIXEL Film's source repository.
pub const GITHUB: &str = "https://github.com/jangopeopleV2/mixel-film";
/// New issue on MIXEL Film's repository.
pub const ISSUES: &str = "https://github.com/jangopeopleV2/mixel-film/issues";

/// (command id, label, url) for every link, in menu order.
pub const ALL: [(&str, &str, &str); 4] = [
    ("help.website", "MIXEL Website", WEBSITE),
    ("help.appPage", "MIXEL Film project page", APP_PAGE),
    ("help.github", "MIXEL Film on GitHub", GITHUB),
    ("help.reportIssue", "Report an Issue…", ISSUES),
];

/// A link the web browser couldn't open (`ui.failedLink`): the address and the reason.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FailedLink {
    pub url: String,
    pub error: String,
}

/// The URL a `help.*` link command opens.
pub fn url_for(command: &str) -> Option<&'static str> {
    ALL.iter().find(|(id, _, _)| *id == command).map(|(_, _, u)| *u)
}

/// What a Help menu item does, shown when the pointer rests on it: the address a link opens in
/// the web browser, or that the item stays inside MIXEL Film.
pub fn menu_hint(command: &str) -> Option<String> {
    let url = if command == "help.filmcraftHelp" { Some(crate::panels::keyboard::HELP_URL) } else { url_for(command) };
    if let Some(url) = url {
        return Some(tlf!("Opens in your web browser: {url}", url));
    }
    Some(
        match command {
            "help.revealLogFiles" => tl!("Saves this session's log and shows it in your file manager."),
            "help.systemCompatibilityReport" => tl!("Shows a report on this computer."),
            "app.about" => tl!("Shows the version, community links and credits in a MIXEL Film window."),
            _ => return None,
        }
        .to_string(),
    )
}

/// Open `url` in the system browser (a new tab on the web). The desktop host's
/// [`HostHooks::open_url`](crate::HostHooks::open_url) opens it; without the hook, or when it
/// fails, it goes through `ctx.open_url`, which only does something on the web: the native egui
/// backend is built without its `links` feature, so there it would only log (#642). When the
/// host can't open a browser, a dialog shows the address until it is closed ([`show_failed`]),
/// so the click is never silently lost: the clipboard can fail too, and egui only logs that.
pub fn open(app: &mut crate::FilmcraftApp, ctx: &egui::Context, url: &str) {
    if let Some(f) = app.hooks.open_url.as_mut() {
        match f(url) {
            Ok(()) => return,
            Err(error) => {
                log::warn!("can't open {url} in the browser: {error}");
                app.ui.failed_link = Some(FailedLink { url: url.to_string(), error });
            }
        }
    }
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// The dialog for a link the browser couldn't open: the reason, and the address in a read-only
/// field you can select from, with Copy. It stays until Close or Escape, so the address is still
/// there after switching to another window. Automation ids: `failedLink.url`, `failedLink.copy`,
/// `failedLink.close`.
pub fn show_failed(app: &mut crate::FilmcraftApp, ctx: &egui::Context) {
    let Some(link) = app.ui.failed_link.clone() else { return };
    let mut close = false;
    let mut elems: Vec<(&str, egui::Rect, String)> = Vec::new();
    egui::Window::new(tl!("Couldn't open your web browser"))
        .id(egui::Id::new("failed-link"))
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            if !link.error.is_empty() {
                ui.label(egui::RichText::new(&link.error).small());
            }
            ui.label(tl!("Open this address in your browser yourself:"));
            ui.horizontal(|ui| {
                let mut text = link.url.as_str();
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(340.0));
                elems.push(("failedLink.url", r.rect, link.url.clone()));
                let c = ui.button(tl!("Copy"));
                elems.push(("failedLink.copy", c.rect, "Copy".into()));
                if c.clicked() {
                    ui.ctx().copy_text(link.url.clone());
                }
            });
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.button(tl!("Close"));
                elems.push(("failedLink.close", r.rect, "Close".into()));
                close |= r.clicked();
            });
        });
    for (id, r, label) in elems {
        app.auto.add(id, r, &label);
    }
    if close || ctx.input(|i| i.key_pressed(egui::Key::Escape) && !i.pointer.any_down()) {
        app.ui.failed_link = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_mixel_routes() {
        assert_eq!(APP_PAGE, "https://github.com/jangopeopleV2/mixel-film".to_string());
        assert_eq!(GITHUB, "https://github.com/jangopeopleV2/mixel-film".to_string());
        assert!(ALL.iter().all(|(id, _, u)| id.starts_with("help.") && u.starts_with("https://")));
        assert_eq!(url_for("help.discord"), None);
        assert_eq!(url_for("help.website"), Some("https://www.mixel.ch"));
        assert_eq!(url_for("help.nope"), None);
    }

    #[test]
    fn links_open_through_the_host_hook() {
        // #642: on the desktop every link (Help menu, About, project links, Import screen) must
        // reach the host's browser opener; `ctx.open_url` alone is a no-op natively.
        use std::cell::RefCell;
        use std::rc::Rc;
        let opened: Rc<RefCell<Vec<String>>> = Rc::default();
        let rec = opened.clone();
        let mut app = crate::FilmcraftApp::new(filmcraft_engine::Session::default());
        app.hooks.open_url = Some(Box::new(move |u: &str| {
            rec.borrow_mut().push(u.to_string());
            Ok(())
        }));
        let ctx = egui::Context::default();
        for (id, _, url) in ALL {
            let r = crate::menus::invoke(&mut app, &ctx, id, serde_json::json!({})).unwrap();
            assert_eq!(r["url"], url, "{id}");
            assert_eq!(opened.borrow().last().map(String::as_str), Some(url), "{id}");
        }
        open(&mut app, &ctx, WEBSITE);
        assert_eq!(opened.borrow().last().map(String::as_str), Some(WEBSITE));
        assert_eq!(opened.borrow().len(), ALL.len() + 1);

        // A failing opener falls back to `ctx.open_url` instead of losing the click.
        app.hooks.open_url = Some(Box::new(|_: &str| Err("no browser".into())));
        open(&mut app, &ctx, ISSUES);
        let fallback: Vec<String> =
            ctx.output(|o| o.commands.iter().filter_map(|c| if let egui::OutputCommand::OpenUrl(u) = c { Some(u.url.clone()) } else { None }).collect());
        assert_eq!(fallback, [ISSUES]);
        // ... and keeps the address on screen with the reason until the dialog is closed
        assert_eq!(app.ui.failed_link, Some(FailedLink { url: ISSUES.into(), error: "no browser".into() }));
    }

    #[test]
    fn import_screen_resource_links_render_declared_labels_and_open_matching_urls() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        struct RestoreLanguage(crate::i18n::Language);
        impl Drop for RestoreLanguage {
            fn drop(&mut self) {
                crate::i18n::set_current(self.0);
            }
        }
        let _language = RestoreLanguage(crate::i18n::current());
        crate::i18n::set_current(crate::i18n::Language::En);
        let opened: Rc<RefCell<Vec<String>>> = Rc::default();
        let record = opened.clone();
        let mut app = crate::FilmcraftApp::new(filmcraft_engine::Session::default());
        app.hooks.open_url = Some(Box::new(move |url: &str| {
            record.borrow_mut().push(url.to_string());
            Ok(())
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx, &crate::theme::Tokens::for_kind(Default::default()));
        let time = Cell::new(0.0);
        let frame = |app: &mut crate::FilmcraftApp, events: Vec<egui::Event>| {
            app.auto.begin_frame();
            time.set(time.get() + 0.05);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 760.0))),
                time: Some(time.get()),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let rect = ui.max_rect();
                crate::panels::import_mode::show(app, ui, rect);
            });
            out.textures_delta.clear();
            out
        };
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        let expected = [
            ("website", "MIXEL Website", "https://www.mixel.ch"),
            ("appPage", "MIXEL Film project page", "https://github.com/jangopeopleV2/mixel-film"),
            ("github", "MIXEL Film on GitHub", "https://github.com/jangopeopleV2/mixel-film"),
            ("reportIssue", "Report an Issue…", "https://github.com/jangopeopleV2/mixel-film/issues"),
        ];
        let labels: Vec<(String, String)> = app
            .auto
            .elements
            .iter()
            .filter(|element| element.id.starts_with("import.link."))
            .map(|element| (element.id.clone(), element.label.clone()))
            .collect();
        assert_eq!(labels, expected.map(|(id, label, _)| (format!("import.link.{id}"), label.to_string())));
        fn painted_text(shape: &egui::Shape, labels: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => labels.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| painted_text(shape, labels)),
                _ => {}
            }
        }
        let mut painted = Vec::new();
        out.shapes.iter().for_each(|shape| painted_text(&shape.shape, &mut painted));
        for (_, label, _) in expected {
            assert!(painted.iter().any(|text| text == label), "missing painted label {label}: {painted:?}");
        }
        assert!(painted.iter().all(|text| !text.contains("Discord")), "{painted:?}");
        for (index, (id, _, url)) in expected.into_iter().enumerate() {
            let element = app.auto.elements.iter().find(|element| element.id == format!("import.link.{id}")).unwrap();
            let at = egui::pos2(element.rect[0] + element.rect[2] / 2.0, element.rect[1] + element.rect[3] / 2.0);
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                let out = frame(&mut app, events);
                assert!(!out.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(_))));
            }
            assert_eq!(opened.borrow().len(), index + 1, "one host-hook call for {id}");
            assert_eq!(opened.borrow().last().map(String::as_str), Some(url), "{id}");
            assert!(app.ui.failed_link.is_none());
        }
    }

    /// The failed-link dialog shows the address until it is closed, and Copy copies it.
    #[test]
    fn failed_link_dialog_keeps_the_address_until_closed() {
        let mut app = crate::FilmcraftApp::new(filmcraft_engine::Session::default());
        app.hooks.open_url = Some(Box::new(|_: &str| Err("no browser".into())));
        let ctx = egui::Context::default();
        let time = std::cell::Cell::new(0.0);
        let frame = |app: &mut crate::FilmcraftApp, events: Vec<egui::Event>| {
            app.auto.begin_frame();
            time.set(time.get() + 0.05);
            let mut out = ctx.run_ui(egui::RawInput { time: Some(time.get()), events, ..Default::default() }, |ui| show_failed(app, ui.ctx()));
            out.textures_delta.clear();
            out
        };
        open(&mut app, &egui::Context::default(), GITHUB);
        for _ in 0..3 {
            frame(&mut app, Vec::new());
        }
        assert!(app.auto.elements.iter().find(|e| e.id == "failedLink.url").is_some_and(|e| e.label == GITHUB), "address shown");
        let at = |app: &crate::FilmcraftApp, id: &str| {
            app.auto.elements.iter().find(|e| e.id == id).map(|e| egui::pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)).unwrap()
        };
        // press and release on separate frames; returns what was copied
        let click = |app: &mut crate::FilmcraftApp, p: egui::Pos2| {
            let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            let mut copied: Vec<String> = Vec::new();
            for events in [vec![egui::Event::PointerMoved(p)], vec![b(true)], vec![b(false)], Vec::new()] {
                let out = frame(app, events);
                copied
                    .extend(out.platform_output.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None }));
            }
            copied
        };
        let copy = at(&app, "failedLink.copy");
        assert_eq!(click(&mut app, copy), [GITHUB]);
        // other frames (another window in front) don't close it; Close does
        for _ in 0..3 {
            frame(&mut app, Vec::new());
        }
        assert!(app.ui.failed_link.is_some());
        let close = at(&app, "failedLink.close");
        click(&mut app, close);
        assert_eq!(app.ui.failed_link, None);
    }

    #[test]
    fn help_menu_items_say_where_they_go() {
        // links name their address; the other Help items say what they do instead
        for (id, _, url) in ALL {
            let hint = menu_hint(id).unwrap();
            assert!(hint.contains(url) && hint.contains("web browser"), "{id}: {hint}");
        }
        assert!(menu_hint("help.filmcraftHelp").unwrap().contains(crate::panels::keyboard::HELP_URL));
        let help_items: Vec<String> = crate::menus::menu_items(&crate::FilmcraftApp::new(filmcraft_engine::Session::default()))
            .into_iter()
            .filter(|it| it.path.first().map(String::as_str) == Some("Help"))
            .map(|it| it.id)
            .collect();
        assert!(help_items.len() >= ALL.len() + 3, "{help_items:?}");
        for id in &help_items {
            let hint = menu_hint(id).unwrap_or_else(|| panic!("{id} has no hover hint"));
            let link = url_for(id).is_some() || id == "help.filmcraftHelp";
            assert_eq!(hint.contains("web browser"), link, "{id}: {hint}");
        }
        assert_eq!(menu_hint("edit.undo"), None);
    }
}
