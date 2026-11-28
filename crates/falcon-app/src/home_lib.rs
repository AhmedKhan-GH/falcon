//! Home Dashboard personalization kept per library (H4): a custom Home cover
//! per folder and a manually chosen Quick Access Hero.
//!
//! These are Falcon-only overrides stored in falcon's own saved state, keyed by
//! the library path and stable folder / asset ids. Nothing here touches the
//! library's `metadata.json`, so Eagle compatibility is untouched.

use std::collections::HashMap;

use crate::app::App;

#[derive(serde::Serialize, serde::Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct HomeLib {
    /// Quick Access entry id chosen as the primary Hero; `None` is Automatic.
    pub(crate) hero: Option<String>,
    /// Folder id -> asset id used as that folder's Home cover.
    pub(crate) covers: HashMap<String, String>,
}

impl HomeLib {
    fn is_empty(&self) -> bool {
        self.hero.is_none() && self.covers.is_empty()
    }
}

/// Moves the chosen Hero to the front, keeping every other entry in its
/// relative order. An unknown or missing choice leaves the list as is
/// (Automatic: the first eligible entry stays the Hero). Returns whether a
/// manual choice was applied.
pub(crate) fn promote_hero<T>(cards: &mut Vec<T>, hero: Option<&str>, id: impl Fn(&T) -> &str) -> bool {
    let Some(hero) = hero else { return false };
    match cards.iter().position(|c| id(c) == hero) {
        Some(0) => true,
        Some(i) => {
            let c = cards.remove(i);
            cards.insert(0, c);
            true
        }
        None => false,
    }
}

impl App {
    fn home_lib(&self) -> Option<&HomeLib> {
        self.home_libs.get(&self.root_key())
    }

    /// The manually chosen Hero id of the shown library, if any.
    pub(crate) fn home_hero_choice(&self) -> Option<&str> {
        self.home_lib().and_then(|h| h.hero.as_deref())
    }

    /// The Home cover asset id set for a folder of the shown library.
    pub(crate) fn home_cover_choice(&self, folder_id: &str) -> Option<&str> {
        self.home_lib().and_then(|h| h.covers.get(folder_id)).map(String::as_str)
    }

    fn home_lib_mut(&mut self) -> &mut HomeLib {
        let key = self.root_key();
        self.home_libs.entry(key).or_default()
    }

    fn prune_home_lib(&mut self) {
        let key = self.root_key();
        if self.home_libs.get(&key).is_some_and(HomeLib::is_empty) {
            self.home_libs.remove(&key);
        }
    }

    pub(crate) fn set_home_hero(&mut self, id: &str) {
        self.home_lib_mut().hero = Some(id.to_string());
    }

    pub(crate) fn reset_home_hero(&mut self) {
        self.home_lib_mut().hero = None;
        self.prune_home_lib();
    }

    pub(crate) fn set_home_cover(&mut self, folder_id: &str, item_id: &str) {
        self.home_lib_mut().covers.insert(folder_id.to_string(), item_id.to_string());
        self.ws.home_covers.clear();
    }

    pub(crate) fn reset_home_cover(&mut self, folder_id: &str) {
        self.home_lib_mut().covers.remove(folder_id);
        self.prune_home_lib();
        self.ws.home_covers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn promote_keeps_relative_order_of_the_rest() {
        let mut c = ids(&["a", "b", "c", "d"]);
        assert!(promote_hero(&mut c, Some("c"), |s| s.as_str()));
        assert_eq!(c, ids(&["c", "a", "b", "d"]));
    }

    #[test]
    fn promote_falls_back_to_automatic() {
        let mut c = ids(&["a", "b"]);
        assert!(!promote_hero(&mut c, Some("gone"), |s| s.as_str()));
        assert!(!promote_hero(&mut c, None, |s| s.as_str()));
        assert_eq!(c, ids(&["a", "b"]));
        assert!(promote_hero(&mut c, Some("a"), |s| s.as_str()));
        assert_eq!(c, ids(&["a", "b"]));
    }

    #[test]
    fn serde_is_backward_compatible_and_library_scoped() {
        let h: HomeLib = serde_json::from_str("{}").unwrap();
        assert!(h.is_empty());
        let mut m: HashMap<String, HomeLib> = HashMap::new();
        let mut a = HomeLib { hero: Some("F1".into()), ..Default::default() };
        a.covers.insert("F2".into(), "I9".into());
        m.insert("/lib/a".into(), a.clone());
        let back: HashMap<String, HomeLib> = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back["/lib/a"], a);
        assert!(!back.contains_key("/lib/b"));
    }
}
