//! Which language the game speaks, and where its catalog comes from.
//!
//! The catalogs in the repository's `lang/` folder are compiled into the
//! executable, so every platform build carries every language with nothing
//! to package. A translator testing a correction drops their edited file in
//! a `lang/` folder beside the executable and restarts: a file there
//! replaces the built-in catalog for that language. A Mac app, or a game in
//! a folder it cannot write to, uses the data folder's `lang/` (next to
//! settings.json) instead; when both have a file, the game folder's wins.
//! The translation itself runs in `ff_core::l10n`; this module only picks
//! and loads.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ff_core::l10n::{self, po, Catalog, Translator, ENGLISH};
use ff_core::models::profile::{data_dir, data_dir_if_allowed};
use ff_core::settings::packaged_game_root;

use super::GameContext;

/// Forces a language for one run, whatever the settings say: `es`, `en`.
/// The playtest harness and tests run in English unless this is set.
pub const LANG_ENV: &str = "FREIGHT_FATE_LANG";
/// A file every line the catalog does not translate is appended to, once
/// each: the to-do list a translator works from.
pub const MISSES_ENV: &str = "FREIGHT_FATE_L10N_MISSES";

/// Every built-in catalog, by code: each `lang/*.po`, listed by `build.rs`.
const BUILT_IN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// A built-in catalog's text.
pub fn built_in_catalog(code: &str) -> Option<&'static str> {
    BUILT_IN
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, text)| *text)
}

/// The folders a translator's file is looked for in, first match wins: the
/// `lang` folder beside the executable in a packaged build, then the data
/// folder's. A run from source skips the first, since the repository's own
/// `lang/` is what the built-in catalogs were compiled from.
fn override_folders(data_dir: Option<&Path>) -> Vec<PathBuf> {
    override_folders_in(packaged_game_root().as_deref(), data_dir)
}

/// [`override_folders`] with the game folder given.
fn override_folders_in(game_root: Option<&Path>, data_dir: Option<&Path>) -> Vec<PathBuf> {
    game_root
        .into_iter()
        .chain(data_dir)
        .map(|dir| dir.join("lang"))
        .collect()
}

/// The catalog text for `code`: the first override in `folders`, else the
/// built-in copy.
fn catalog_text(folders: &[PathBuf], code: &str) -> Option<Cow<'static, str>> {
    for folder in folders {
        let path = folder.join(format!("{code}.po"));
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                log::info!("Using the translation file {}", path.display());
                return Some(Cow::Owned(text));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => log::warn!("Could not read {}: {err}", path.display()),
        }
    }
    built_in_catalog(code).map(Cow::Borrowed)
}

/// How many lines a language's catalog translates (a translator's copy
/// when there is one); a language with none is not offered in Settings.
pub fn translated_count(code: &str) -> usize {
    if code == ENGLISH {
        return usize::MAX;
    }
    let text = match data_dir_if_allowed() {
        Some(dir) => catalog_text(&override_folders(Some(&dir)), code),
        None => built_in_catalog(code).map(Cow::Borrowed),
    };
    text.and_then(|text| Catalog::from_po(code, &text).ok())
        .map_or(0, |catalog| catalog.len())
}

/// The `(code, name, autonym)` of every catalog that `l10n::LANGUAGES`
/// does not list: a language a translator added as a file alone, either
/// built in from `lang/` or dropped into the game folder's or `dir`'s
/// `lang/` folder. The name is
/// the file's `X-Language-Name` header and the autonym its
/// `X-Language-Autonym`; either stands in for a missing other, and the code
/// for both.
pub fn added_languages(dir: &Path) -> Vec<(String, String, String)> {
    added_languages_in(&override_folders(Some(dir)))
}

/// [`added_languages`], looking in `folders` for dropped-in files.
fn added_languages_in(folders: &[PathBuf]) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = Vec::new();
    let mut consider = |code: &str, text: &str| {
        let plausible = !code.is_empty()
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !plausible
            || l10n::LANGUAGES.iter().any(|lang| lang.code == code)
            || found.iter().any(|(known, _, _)| known == code)
        {
            return;
        }
        let name = po::header_field(text, "X-Language-Name");
        let autonym = po::header_field(text, "X-Language-Autonym");
        let (name, autonym) = match (name, autonym) {
            (Some(name), Some(autonym)) => (name, autonym),
            (Some(one), None) | (None, Some(one)) => (one.clone(), one),
            (None, None) => (code.to_string(), code.to_string()),
        };
        found.push((code.to_string(), name, autonym));
    };
    for (code, text) in BUILT_IN {
        consider(code, text);
    }
    for folder in folders {
        consider_folder(folder, &mut consider);
    }
    found
}

/// Offer every `.po` file in `folder` to `consider`, in name order.
fn consider_folder(folder: &Path, consider: &mut impl FnMut(&str, &str)) {
    let mut dropped: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "po"))
        .collect();
    dropped.sort();
    for path in dropped {
        let Some(code) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => consider(code, &text),
            Err(err) => log::warn!("Could not read {}: {err}", path.display()),
        }
    }
}

/// Hand the added languages to `l10n` once, before any setting resolves.
/// A process with no data folder of its own (a test that pinned none) gets
/// the built-in ones only.
fn register_added_languages() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let added = added_languages_in(&override_folders(data_dir_if_allowed().as_deref()));
        for (code, _, autonym) in &added {
            log::info!("Offering {autonym} ({code}) from its translation file");
        }
        l10n::add_languages(added);
    });
}

/// The languages the Settings row cycles through: English, then every
/// language whose catalog translates something. Worked out once a launch.
pub fn offered_languages() -> &'static [&'static l10n::Language] {
    static OFFERED: OnceLock<Vec<&'static l10n::Language>> = OnceLock::new();
    OFFERED.get_or_init(|| {
        register_added_languages();
        l10n::languages()
            .filter(|lang| translated_count(lang.code) > 0)
            .collect()
    })
}

/// The folder downloads from October 13 to 17, 2026 carried the translator's
/// kit in, beside the program, as `translations`. The game never read it,
/// and updates leave it in place, so a translator who found it edited files
/// that did nothing.
fn old_kit_dir(game: &Path) -> PathBuf {
    game.join("translations")
}

/// Move a packaged game's leftover kit folder out of the game folder
/// and into the data folder, as `old-translations`, so the folder that
/// works (`lang`) is the only one a translator finds there. It is moved, not
/// deleted, in case a translator's edits are in it.
pub fn retire_old_translation_kit() {
    if let (Some(game), Some(data)) = (packaged_game_root(), data_dir_if_allowed()) {
        retire_old_translation_kit_in(&game, &data);
    }
}

/// [`retire_old_translation_kit`] with both folders given; the new path,
/// when something was moved.
fn retire_old_translation_kit_in(game: &Path, data: &Path) -> Option<PathBuf> {
    let old = old_kit_dir(game);
    // Only the kit: it always has the template in it.
    if !old.join("freight-fate.pot").is_file() {
        return None;
    }
    let mut target = data.join("old-translations");
    let mut number = 2;
    while target.exists() {
        target = data.join(format!("old-translations-{number}"));
        number += 1;
    }
    let moved = std::fs::create_dir_all(data).and_then(|()| std::fs::rename(&old, &target));
    match moved {
        Ok(()) => {
            log::info!(
                "Moved the unused translation folder {} to {}",
                old.display(),
                target.display()
            );
            Some(target)
        }
        Err(err) => {
            log::warn!(
                "Could not move the unused translation folder {}: {err}",
                old.display()
            );
            None
        }
    }
}

/// A translator for `code`, falling back to English when there is no
/// usable catalog. `dir` is the data folder; the game folder's `lang/` is
/// looked in first.
pub fn load_translator(code: &str, dir: &Path) -> Translator {
    if code == ENGLISH {
        return Translator::english();
    }
    let Some(text) = catalog_text(&override_folders(Some(dir)), code) else {
        log::warn!("No translation for {code}; speaking English");
        return Translator::english();
    };
    let catalog = match Catalog::from_po(code, &text) {
        Ok(catalog) => catalog,
        Err(err) => {
            log::error!("The {code} translation could not be read ({err}); speaking English");
            return Translator::english();
        }
    };
    for problem in &catalog.problems {
        log::warn!("{code} translation, {problem}");
    }
    log::info!("Speaking {code}: {} translated lines", catalog.len());
    let translator = Translator::new(catalog);
    match std::env::var_os(MISSES_ENV).filter(|path| !path.is_empty()) {
        Some(path) => translator.log_misses_to(PathBuf::from(path)),
        None => translator,
    }
}

/// The operating system's language tag, e.g. "es-MX".
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}

impl GameContext {
    /// Load the catalog the language setting asks for (or `FREIGHT_FATE_LANG`
    /// when it is set). Called once at launch by the windowed game, and
    /// whenever the player changes the setting.
    pub fn apply_language(&mut self) {
        let wanted = std::env::var(LANG_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.settings.language.clone());
        self.set_language(&wanted);
    }

    /// Speak `setting` (a code, or `auto`) from now on.
    pub fn set_language(&mut self, setting: &str) {
        register_added_languages();
        let code = l10n::resolve(setting, system_locale().as_deref());
        if code != self.translator.language() {
            self.translator = std::sync::Arc::new(load_translator(code, &data_dir()));
        }
    }

    /// `text` in the player's language. Everything `say` and `say_event`
    /// speak already passes through here; a line that reaches the speech
    /// sink or message review another way calls it itself.
    pub fn tr(&self, text: &str) -> String {
        self.translator.translate(text).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_catalog(folder: &Path, code: &str, line: &str) {
        std::fs::create_dir_all(folder).expect("lang folder");
        std::fs::write(
            folder.join(format!("{code}.po")),
            format!("msgid \"New screen.\"\nmsgstr \"{line}\"\n"),
        )
        .expect("catalog written");
    }

    #[test]
    fn the_game_folder_is_looked_in_before_the_data_folder() {
        let game = tempfile::tempdir().expect("game folder");
        let data = tempfile::tempdir().expect("data folder");
        let folders = override_folders_in(Some(game.path()), Some(data.path()));
        assert_eq!(
            folders,
            vec![game.path().join("lang"), data.path().join("lang")]
        );

        write_catalog(&data.path().join("lang"), "es", "Desde los datos.");
        let text = catalog_text(&folders, "es").expect("catalog");
        assert!(text.contains("Desde los datos."), "{text}");

        write_catalog(&game.path().join("lang"), "es", "Desde el juego.");
        let text = catalog_text(&folders, "es").expect("catalog");
        assert!(text.contains("Desde el juego."), "{text}");
    }

    #[test]
    fn the_old_kit_folder_moves_to_the_data_folder_with_its_files() {
        let game = tempfile::tempdir().expect("game folder");
        let data = tempfile::tempdir().expect("data folder");
        let old = old_kit_dir(game.path());
        std::fs::create_dir_all(&old).expect("old kit");
        std::fs::write(old.join("freight-fate.pot"), "").expect("template");
        write_catalog(&old, "pl", "Poprawione.");
        std::fs::create_dir_all(data.path().join("old-translations")).expect("earlier move");

        let moved = retire_old_translation_kit_in(game.path(), data.path()).expect("moved");
        assert_eq!(moved, data.path().join("old-translations-2"));
        assert!(!old.exists());
        let kept = std::fs::read_to_string(moved.join("pl.po")).expect("edit kept");
        assert!(kept.contains("Poprawione."), "{kept}");
    }

    #[test]
    fn a_translations_folder_that_is_not_the_kit_stays() {
        let game = tempfile::tempdir().expect("game folder");
        let data = tempfile::tempdir().expect("data folder");
        let other = old_kit_dir(game.path());
        std::fs::create_dir_all(&other).expect("folder");
        assert_eq!(
            retire_old_translation_kit_in(game.path(), data.path()),
            None
        );
        assert!(other.exists());
    }

    #[test]
    fn a_language_dropped_beside_the_game_is_offered() {
        let game = tempfile::tempdir().expect("game folder");
        std::fs::create_dir_all(game.path().join("lang")).expect("lang folder");
        std::fs::write(
            game.path().join("lang").join("nl.po"),
            "msgid \"\"\nmsgstr \"X-Language-Name: Dutch\\nX-Language-Autonym: Nederlands\\n\"\n",
        )
        .expect("file written");
        let added = added_languages_in(&override_folders_in(Some(game.path()), None));
        assert!(added.contains(&(
            "nl".to_string(),
            "Dutch".to_string(),
            "Nederlands".to_string()
        )));
    }
}
