//! `translation-kit`: package the translator's kit as its own release
//! download.
//!
//! The kit is the repository's `lang/` folder: the template a new language
//! starts from, every language file to correct, the glossaries and the guide
//! (`README.md`, whose links are relative to the folder). The game reads the
//! catalogs compiled into it, so none of this ships inside the game
//! archives; a player who wants to translate downloads this one zip.
//!
//! `cargo tools translation-kit --label 1.9-tester-20261017 --out release-assets`
//! writes `release-assets/FreightFate-1.9-tester-20261017-translation-kit.zip`.

use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::cli::{Args, Spec};
use crate::util::{self, rel, Result};

/// The folder a player gets when they unzip the kit.
const KIT_DIR: &str = "FreightFate-translation-kit";

/// The kit's asset name. It ends in neither updater suffix
/// (`-windows-portable.zip`, `-macos.zip`, ...), so no updater picks it.
pub fn kit_name(label: &str) -> String {
    format!("FreightFate-{label}-translation-kit.zip")
}

/// Zip `lang` under [`KIT_DIR`] into `out_dir`; return the zip's path.
pub fn build_kit(label: &str, out_dir: &Path, lang: &Path) -> Result<PathBuf> {
    let template = lang.join("freight-fate.pot");
    if !template.is_file() {
        return Err(format!(
            "Translation template was not found: {}",
            template.display()
        ));
    }
    if !lang.join("README.md").is_file() {
        return Err(format!(
            "Translator's guide was not found: {}",
            lang.join("README.md").display()
        ));
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let destination = out_dir.join(kit_name(label));
    let file = std::fs::File::create(&destination)
        .map_err(|e| format!("cannot write {}: {e}", destination.display()))?;
    let mut archive = ZipWriter::new(file);
    // A fixed timestamp, so the same lang/ folder always zips to the same bytes.
    let date = DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).map_err(|e| e.to_string())?;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9))
        .last_modified_time(date)
        .unix_permissions(0o644);
    for path in util::walk_files(lang) {
        let name = format!("{KIT_DIR}/{}", rel(&path, lang));
        archive
            .start_file(name, options)
            .map_err(|e| format!("zip: {e}"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        archive.write_all(&bytes).map_err(|e| format!("zip: {e}"))?;
    }
    archive.finish().map_err(|e| format!("zip: {e}"))?;
    Ok(destination)
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["label", "out"],
            flags: &[],
        },
    )
    .map_err(|e| e.to_string())?;
    let root = util::repo_root();
    let label = args.required("label").map_err(|e| e.to_string())?;
    let out = args.path("out").unwrap_or_else(|| root.join("dist"));
    println!("{}", build_kit(&label, &out, &root.join("lang"))?.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_is_deterministic_and_rooted_in_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let lang = dir.path().join("lang");
        std::fs::create_dir_all(lang.join("glossary")).unwrap();
        std::fs::write(lang.join("freight-fate.pot"), "msgid \"\"\n").unwrap();
        std::fs::write(lang.join("README.md"), "guide").unwrap();
        std::fs::write(lang.join("glossary").join("es.md"), "x").unwrap();
        let a = build_kit("t1", &dir.path().join("a"), &lang).unwrap();
        let b = build_kit("t1", &dir.path().join("b"), &lang).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&a).unwrap()).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "FreightFate-translation-kit/README.md",
                "FreightFate-translation-kit/freight-fate.pot",
                "FreightFate-translation-kit/glossary/es.md",
            ]
        );
    }
}
