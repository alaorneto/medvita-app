//! Assets: the app's own files (`assets/` at the repository root) first, then GPUI Kit's icons.
//! GPUI Kit's source answers `Err` for an unknown path, so it is the last one asked.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "brand/*.svg"]
#[include = "icons/*.svg"]
struct Own;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(file) = Own::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut entries: Vec<SharedString> = Own::iter()
            .filter(|name| name.starts_with(path))
            .map(|name| SharedString::from(name.to_string()))
            .collect();
        for entry in gpui_kit::assets::Assets.list(path)? {
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
}

pub const LOGO: &str = "brand/medvita.svg";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_assets_come_first_and_kit_icons_still_load() {
        assert!(Assets.load(LOGO).unwrap().is_some());
        assert!(Assets.load("icons/calendar.svg").unwrap().is_some());
        assert!(Assets.load("").unwrap().is_none());
    }

    #[test]
    fn list_has_no_duplicates() {
        let listed = Assets.list("").unwrap();
        let mut unique = listed.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(listed.len(), unique.len());
        assert!(listed.iter().any(|entry| entry.as_ref() == LOGO));
    }
}
