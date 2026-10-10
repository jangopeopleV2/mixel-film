//! Community and project links (Help menu, About dialog, header Discord button, Home screen).
//!
//! MIXEL Film is part of the ArtCraft family: the community lives on the ArtCraft Discord, every
//! app has a page on the ArtCraft website and a public GitHub repository.

/// The app's short name, used in the website and repository URLs.
pub const APP: &str = "filmcraft";

/// The ArtCraft community Discord.
pub const DISCORD: &str = "https://discord.gg/artcraft";
/// The ArtCraft website.
pub const WEBSITE: &str = "https://getartcraft.com";
/// MIXEL Film's page on the ArtCraft website.
pub const APP_PAGE: &str = "https://github.com/jangopeopleV2/mixel-film";
/// MIXEL Film's source repository.
pub const GITHUB: &str = "https://github.com/jangopeopleV2/mixel-film";
/// New issue on MIXEL Film's repository.
pub const ISSUES: &str = "https://github.com/jangopeopleV2/mixel-film/issues";

/// (command id, label, url) for every link, in menu order.
pub const ALL: [(&str, &str, &str); 5] = [
    ("help.discord", "Join the ArtCraft Discord…", DISCORD),
    ("help.website", "ArtCraft Website", WEBSITE),
    ("help.appPage", "MIXEL Film project page", APP_PAGE),
    ("help.github", "MIXEL Film on GitHub", GITHUB),
    ("help.reportIssue", "Report an Issue…", ISSUES),
];

/// The URL a `help.*` link command opens.
pub fn url_for(command: &str) -> Option<&'static str> {
    ALL.iter().find(|(id, _, _)| *id == command).map(|(_, _, u)| *u)
}

/// Open `url` in the system browser (a new tab on the web).
pub fn open(ctx: &egui::Context, url: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_artcraft_scheme() {
        assert_eq!(APP_PAGE, "https://github.com/jangopeopleV2/mixel-film".to_string());
        assert_eq!(GITHUB, "https://github.com/jangopeopleV2/mixel-film".to_string());
        assert!(ALL.iter().all(|(id, _, u)| id.starts_with("help.") && u.starts_with("https://")));
        assert_eq!(url_for("help.discord"), Some(DISCORD));
        assert_eq!(url_for("help.nope"), None);
    }
}
