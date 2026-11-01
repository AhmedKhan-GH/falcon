//! Hand-arranged ("manual") item order inside a folder.
//!
//! Eagle keeps the order in each item's `order` map: `order[<folder id>]` is
//! a decimal string. The folder's `MANUAL` sort compares these strings as
//! strings, largest first, and an item without an entry sorts by its
//! `modificationTime` (as a string) instead. Dragging items writes new
//! values halfway between the neighbours they land between, computed in
//! exact decimal arithmetic (division rounded to 20 places) and written in
//! the shortest form, as a big-number library's `toPrecision()` does.

use std::cmp::Ordering;

use num_bigint::BigInt;
use serde_json::Value;

use crate::error::Result;
use crate::library::Library;
use crate::model::{FolderOrder, Item};

/// Decimal places kept by a division, as Eagle's big-number library does.
const DIV_PLACES: u32 = 20;

/// An exact decimal: `m / 10^s`, kept without trailing zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dec {
    m: BigInt,
    s: u32,
}

fn pow10(n: u32) -> BigInt {
    BigInt::from(10u8).pow(n)
}

impl Dec {
    fn new(m: BigInt, s: u32) -> Dec {
        let mut d = Dec { m, s };
        let ten = BigInt::from(10u8);
        let zero = BigInt::default();
        while d.s > 0 && &d.m % &ten == zero {
            d.m /= &ten;
            d.s -= 1;
        }
        if d.m == zero {
            d.s = 0;
        }
        d
    }

    pub(crate) fn from_int(v: i64) -> Dec {
        Dec::new(BigInt::from(v), 0)
    }

    /// Parses `-1.5`, `.5`, `12`, `3e-7` and the like.
    pub(crate) fn parse(text: &str) -> Option<Dec> {
        let t = text.trim();
        let (neg, t) = match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let (num, exp) = match t.find(['e', 'E']) {
            Some(i) => (&t[..i], t[i + 1..].parse::<i64>().ok()?),
            None => (t, 0),
        };
        let (int, frac) = num.split_once('.').unwrap_or((num, ""));
        if int.is_empty() && frac.is_empty() || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let digits = format!("{int}{frac}");
        let mut m: BigInt = digits.parse().ok()?;
        if neg {
            m = -m;
        }
        let scale = frac.len() as i64 - exp;
        if scale >= 0 {
            Some(Dec::new(m, u32::try_from(scale).ok()?))
        } else {
            Some(Dec::new(m * pow10(u32::try_from(-scale).ok()?), 0))
        }
    }

    /// A JavaScript number as `new BigNumber(n)` reads it: through its
    /// shortest decimal form, float noise included.
    pub(crate) fn from_js_number(v: f64) -> Option<Dec> {
        v.is_finite().then(|| Dec::parse(&format!("{v}"))).flatten()
    }

    fn aligned(&self, o: &Dec) -> (BigInt, BigInt, u32) {
        let s = self.s.max(o.s);
        (&self.m * pow10(s - self.s), &o.m * pow10(s - o.s), s)
    }

    pub(crate) fn plus(&self, o: &Dec) -> Dec {
        let (a, b, s) = self.aligned(o);
        Dec::new(a + b, s)
    }

    pub(crate) fn minus(&self, o: &Dec) -> Dec {
        let (a, b, s) = self.aligned(o);
        Dec::new(a - b, s)
    }

    pub(crate) fn times_int(&self, k: i64) -> Dec {
        Dec::new(&self.m * BigInt::from(k), self.s)
    }

    /// Division by a positive integer, rounded to 20 places, halves away
    /// from zero.
    pub(crate) fn div_int(&self, n: u64) -> Dec {
        let num = &self.m * pow10(DIV_PLACES.saturating_sub(self.s));
        let den = BigInt::from(n) * pow10(self.s.saturating_sub(DIV_PLACES));
        let neg = num < BigInt::default();
        let abs = if neg { -num } else { num };
        let (q, r) = (&abs / &den, &abs % &den);
        let q = if r * 2u8 >= den { q + 1u8 } else { q };
        Dec::new(if neg { -q } else { q }, DIV_PLACES)
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.m == BigInt::default()
    }

    /// The shortest string, in exponent form when the leading digit sits at
    /// 10^-7 or below or 10^21 or above (the library's defaults).
    pub(crate) fn to_precision(&self) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let neg = self.m < BigInt::default();
        let digits = self.m.magnitude().to_string();
        let e = digits.len() as i64 - 1 - i64::from(self.s);
        let body = if e <= -7 || e >= 21 {
            let (first, rest) = digits.split_at(1);
            let rest = rest.trim_end_matches('0');
            let dot = if rest.is_empty() { String::new() } else { format!(".{rest}") };
            format!("{first}{dot}e{}{}", if e < 0 { "-" } else { "+" }, e.abs())
        } else if self.s == 0 {
            digits
        } else if digits.len() > self.s as usize {
            let (int, frac) = digits.split_at(digits.len() - self.s as usize);
            format!("{int}.{frac}")
        } else {
            format!("0.{}{digits}", "0".repeat(self.s as usize - digits.len()))
        };
        if neg { format!("-{body}") } else { body }
    }
}

/// An `order` entry as the text Eagle's sort compares: strings as they are,
/// numbers in JavaScript's shortest form. Empty, zero and non-scalar
/// entries count as missing, as they are falsy in JavaScript.
fn entry_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.as_f64() != Some(0.0) => Some(match n.as_f64() {
            Some(f) if n.is_f64() => format!("{f}"),
            _ => n.to_string(),
        }),
        _ => None,
    }
}

impl Item {
    /// The item's raw `order[folder]` text, if it has a usable one.
    pub fn manual_order_text(&self, folder: &str) -> Option<String> {
        self.extra.get("order").and_then(|o| o.get(folder)).and_then(entry_text)
    }

    /// The item's manual position value in `folder` (`order[folder]`), as a
    /// number; `None` when it has no entry. Larger values come first in an
    /// ascending manual sort.
    pub fn manual_order(&self, folder: &str) -> Option<f64> {
        self.manual_order_text(folder).and_then(|t| t.trim().parse::<f64>().ok())
    }

    /// The text Eagle's `MANUAL` sort compares for this item: its entry, or
    /// `modificationTime` when it has none.
    pub fn manual_sort_key(&self, folder: &str) -> String {
        self.manual_order_text(folder).unwrap_or_else(|| self.modification_time.to_string())
    }

    /// Sets `order[folder]`. A new `order` map goes at the end of the
    /// record, a new folder key at the end of the map; existing keys keep
    /// their place.
    pub fn set_manual_order(&mut self, folder: &str, value: &str) {
        let slot = self.extra.entry("order").or_insert_with(|| Value::Object(Default::default()));
        if !slot.is_object() {
            *slot = Value::Object(Default::default());
        }
        if let Value::Object(m) = slot {
            m.insert(folder.to_string(), Value::String(value.to_string()));
        }
    }

    /// The decimal this item stands at in `folder` for drag arithmetic.
    fn manual_dec(&self, folder: &str) -> Dec {
        self.manual_order_text(folder).and_then(|t| Dec::parse(&t)).unwrap_or_else(|| Dec::from_int(self.modification_time as i64))
    }
}

/// Eagle's `MANUAL` comparison: the sort keys compared as strings, larger
/// first. Reverse the result for a descending folder.
pub fn manual_cmp(a: &Item, b: &Item, folder: &str) -> Ordering {
    b.manual_sort_key(folder).cmp(&a.manual_sort_key(folder))
}

impl Library {
    /// Moves `moved` items, as one block in their current manual order, to
    /// just before `before` (or to the end with `None`) in `folder`'s manual
    /// order, as dragging in Eagle's grid does. New `order` values sit
    /// between the neighbours; if two other items share a value, all values
    /// are first nudged apart. Changed items are saved with
    /// [`Library::update_item`], and a folder not yet sorted by hand is
    /// switched to an ascending manual sort.
    ///
    /// The order is worked out over all untrashed items in the folder (Eagle
    /// uses the items currently shown). Moving into an otherwise empty
    /// folder changes nothing, as Eagle has no neighbour to place against.
    pub fn move_items_manually(&mut self, folder: &str, moved: &[String], before: Option<&str>) -> Result<()> {
        let Some(f) = self.all_folders().into_iter().find(|f| f.id == folder) else {
            return Err(self.invalid(format!("no folder {folder}")));
        };
        let is_manual = f.order() == Some(FolderOrder::Manual);
        let increase = if is_manual { f.sort_increase.unwrap_or(true) } else { true };
        for id in moved {
            match self.item(id) {
                Some(i) if i.folders.iter().any(|x| x == folder) && !i.is_deleted => {}
                _ => return Err(self.invalid(format!("item {id} is not in folder {folder}"))),
            }
        }
        if before.is_some_and(|b| moved.iter().any(|m| m == b)) {
            return Err(self.invalid("cannot move items before one of themselves"));
        }

        let mut view: Vec<&Item> = self.items().iter().filter(|i| !i.is_deleted && i.folders.iter().any(|x| x == folder)).collect();
        crate::search::sort_manual(&mut view, folder, increase);
        let (selected, rest): (Vec<&Item>, Vec<&Item>) = view.into_iter().partition(|i| moved.contains(&i.id));
        let mut selected: Vec<Item> = selected.into_iter().cloned().collect();
        let mut rest: Vec<Item> = rest.into_iter().cloned().collect();
        let at = match before {
            Some(b) => Some(rest.iter().position(|i| i.id == b).ok_or_else(|| self.invalid(format!("item {b} is not in folder {folder}")))?),
            None => None,
        };
        if selected.is_empty() || rest.is_empty() {
            return Ok(());
        }

        // Two items with the same value: nudge every value apart first.
        let mut changed: Vec<Item> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if rest.iter().filter_map(|i| i.manual_order_text(folder)).any(|t| !seen.insert(t)) {
            let precision = rest.len().to_string().len() as i32 + 1;
            let step = 1.0 / 10f64.powi(precision);
            for (i, item) in rest.iter_mut().enumerate() {
                let Some(old) = item.manual_order_text(folder).and_then(|t| Dec::parse(&t)) else { continue };
                // Eagle reads its direction flag before setting it here, so
                // values always step down.
                let Some(delta) = Dec::from_js_number(step * i as f64) else { continue };
                item.set_manual_order(folder, &old.minus(&delta).to_precision());
                changed.push(item.clone());
            }
        }

        let one = Dec::from_int(1);
        let n = selected.len() as u64;
        let (middle, step) = match at {
            Some(0) => {
                let target = rest[0].manual_dec(folder);
                let prev = if increase { target.plus(&one) } else { target.minus(&one) };
                let middle = prev.plus(&target).div_int(2);
                let step = middle.minus(&target).div_int(n);
                (middle, step)
            }
            Some(k) => {
                let (prev, target) = (rest[k - 1].manual_dec(folder), rest[k].manual_dec(folder));
                let middle = prev.plus(&target).div_int(2);
                let step = middle.minus(&target).div_int(n);
                (middle, step)
            }
            None => {
                let prev = rest[rest.len() - 1].manual_dec(folder);
                let target = if increase { prev.minus(&one) } else { prev.plus(&one) };
                let middle = target.plus(&prev).div_int(2);
                let step = middle.minus(&target).div_int(n);
                (middle, step)
            }
        };
        if !step.is_zero() {
            for (index, item) in selected.iter_mut().rev().enumerate() {
                item.set_manual_order(folder, &middle.plus(&step.times_int(index as i64)).to_precision());
            }
            changed.extend(selected);
        }
        for item in changed {
            self.update_item(item)?;
        }
        if !is_manual {
            self.set_folder_order(folder, Some(FolderOrder::Manual), true)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Dec {
        Dec::parse(s).unwrap()
    }

    #[test]
    fn decimals_print_like_to_precision() {
        assert_eq!(d("1789479235636").to_precision(), "1789479235636");
        assert_eq!(d("1789479235636.50").to_precision(), "1789479235636.5");
        assert_eq!(d("-0.25").to_precision(), "-0.25");
        assert_eq!(d("0.0000001").to_precision(), "1e-7");
        assert_eq!(d("0.0000012").to_precision(), "0.0000012");
        assert_eq!(d("1.5e-8").to_precision(), "1.5e-8");
        assert_eq!(d("123e19").to_precision(), "1.23e+21");
        assert_eq!(d("0").to_precision(), "0");
        assert_eq!(Dec::parse("abc"), None);
        assert_eq!(Dec::parse(""), None);
    }

    #[test]
    fn division_keeps_twenty_places_rounding_half_away() {
        assert_eq!(d("1").div_int(3).to_precision(), "0.33333333333333333333");
        assert_eq!(d("2").div_int(3).to_precision(), "0.66666666666666666667");
        assert_eq!(d("-2").div_int(3).to_precision(), "-0.66666666666666666667");
        assert_eq!(d("3579000000001").div_int(2).to_precision(), "1789500000000.5");
    }

    #[test]
    fn js_numbers_bring_their_float_noise() {
        assert_eq!(Dec::from_js_number(0.01 * 57.0).unwrap().to_precision(), "0.5700000000000001");
        assert_eq!(Dec::from_js_number(0.01 * 3.0).unwrap().to_precision(), "0.03");
    }

    fn record(id: &str, mtime: u64, extra: &str) -> Item {
        Item::from_json_str(&format!(
            r#"{{"id":"{id}","name":"{id}","size":1,"btime":1,"mtime":1,"ext":"tex","tags":[],"folders":["F"],"isDeleted":false,"url":"","annotation":"","modificationTime":{mtime},"lastModified":1{extra}}}"#
        ))
        .unwrap()
    }

    /// A library with folder `F` holding the given 0×0 records.
    fn library(dir: &std::path::Path, items: Vec<Item>) -> (Library, String) {
        let mut lib = Library::create(dir.join("T.library")).unwrap();
        let f = lib.create_folder(None, "F").unwrap();
        for mut item in items {
            item.folders = vec![f.clone()];
            std::fs::create_dir_all(item.info_dir(lib.root())).unwrap();
            lib.register_item(item).unwrap();
        }
        (lib, f)
    }

    fn view(lib: &Library, f: &str) -> Vec<String> {
        let mut v: Vec<&Item> = lib.items().iter().filter(|i| i.folders.iter().any(|x| x == f)).collect();
        crate::search::sort_manual(&mut v, f, true);
        v.iter().map(|i| i.id.clone()).collect()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn manual_sort_compares_text_largest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut b = record("B", 1000, "");
        b.set_manual_order("X", "999");
        let (lib, f) = library(tmp.path(), vec![record("A", 2000, ""), b]);
        // B has no entry for F, so its modificationTime counts.
        assert_eq!(view(&lib, &f), ids(&["A", "B"]));
        let b = lib.item("B").unwrap();
        assert_eq!((b.manual_order("X"), b.manual_order(&f)), (Some(999.0), None));
        // As text, "999" sorts above "1000".
        let mut x: Vec<&Item> = lib.items().iter().collect();
        crate::search::sort_manual(&mut x, "X", true);
        assert_eq!(x[0].id, "B");
    }

    #[test]
    fn dropping_between_neighbours_takes_the_midpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, f) = library(tmp.path(), vec![record("A", 1000, ""), record("B", 2000, ""), record("C", 3000, "")]);
        assert_eq!(view(&lib, &f), ids(&["C", "B", "A"]));
        lib.move_items_manually(&f, &ids(&["A"]), Some("B")).unwrap();
        assert_eq!(lib.item("A").unwrap().manual_order_text(&f).as_deref(), Some("2500"));
        assert_eq!(view(&lib, &f), ids(&["C", "A", "B"]));
        let folder = lib.all_folders().into_iter().find(|x| x.id == f).unwrap();
        assert_eq!((folder.order(), folder.sort_increase), (Some(FolderOrder::Manual), Some(true)));

        // Two items to the front: above C by halves of the gap to C + 1.
        lib.move_items_manually(&f, &ids(&["B", "A"]), Some("C")).unwrap();
        assert_eq!(view(&lib, &f), ids(&["A", "B", "C"]));
        assert_eq!(lib.item("B").unwrap().manual_order_text(&f).as_deref(), Some("3000.5"));
        assert_eq!(lib.item("A").unwrap().manual_order_text(&f).as_deref(), Some("3000.75"));

        // To the end: below the last item.
        lib.move_items_manually(&f, &ids(&["A"]), None).unwrap();
        assert_eq!(view(&lib, &f), ids(&["B", "C", "A"]));
        assert_eq!(lib.item("A").unwrap().manual_order_text(&f).as_deref(), Some("2999.5"));

        // The values land on disk, and the record round-trips.
        let reopened = Library::open(lib.root()).unwrap();
        assert_eq!(view(&reopened, &f), ids(&["B", "C", "A"]));
        let text = std::fs::read_to_string(reopened.item("A").unwrap().info_dir(reopened.root()).join("metadata.json")).unwrap();
        assert!(text.ends_with(&format!(r#","order":{{"{f}":"2999.5"}}}}"#)), "{text}");
        assert!(!text.contains("width"), "0×0 records stay without dimensions: {text}");

        assert!(lib.move_items_manually(&f, &ids(&["A"]), Some("A")).is_err());
        assert!(lib.move_items_manually(&f, &ids(&["nope"]), None).is_err());
    }

    #[test]
    fn three_way_split_rounds_to_twenty_places() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, f) = library(tmp.path(), vec![record("A", 1000, ""), record("B", 1001, ""), record("C", 3000, ""), record("D", 3001, ""), record("E", 3002, "")]);
        // View: E D C B A. Put C, D, E between B and A.
        lib.move_items_manually(&f, &ids(&["C", "D", "E"]), Some("A")).unwrap();
        assert_eq!(view(&lib, &f), ids(&["B", "E", "D", "C", "A"]));
        let text = |id: &str| lib.item(id).unwrap().manual_order_text(&f).unwrap();
        assert_eq!(text("C"), "1000.5");
        assert_eq!(text("D"), "1000.66666666666666666667");
        assert_eq!(text("E"), "1000.83333333333333333334");
    }

    #[test]
    fn duplicate_values_are_nudged_apart_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut a = record("A", 1, "");
        let mut b = record("B", 2, "");
        a.set_manual_order("F0", "5000");
        b.set_manual_order("F0", "5000");
        let (mut lib, f) = library(tmp.path(), vec![a, b, record("C", 9000, ""), record("D", 1000, "")]);
        for id in ["A", "B"] {
            let mut item = lib.item(id).unwrap().clone();
            item.extra.remove("order");
            item.set_manual_order(&f, "5000");
            lib.update_item(item).unwrap();
        }
        // View: C(9000) A B(5000, tie keeps id order) D(1000). Move D first.
        assert_eq!(view(&lib, &f), ids(&["C", "A", "B", "D"]));
        lib.move_items_manually(&f, &ids(&["D"]), Some("C")).unwrap();
        // Rest is C, A, B; three items -> steps of 0.01 times the index.
        let text = |id: &str| lib.item(id).unwrap().manual_order_text(&f).unwrap();
        assert_eq!(text("A"), "4999.99");
        assert_eq!(text("B"), "4999.98");
        assert_eq!(text("D"), "9000.5");
        assert_eq!(view(&lib, &f), ids(&["D", "C", "A", "B"]));
    }

    #[test]
    fn descending_folders_place_against_the_other_side() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut lib, f) = library(tmp.path(), vec![record("A", 1000, ""), record("B", 2000, "")]);
        lib.set_folder_order(&f, Some(FolderOrder::Manual), false).unwrap();
        // Shown ascending by value: A B. Move B in front of A.
        lib.move_items_manually(&f, &ids(&["B"]), Some("A")).unwrap();
        assert_eq!(lib.item("B").unwrap().manual_order_text(&f).as_deref(), Some("999.5"));
        let folder = lib.all_folders().into_iter().find(|x| x.id == f).unwrap();
        assert_eq!(folder.sort_increase, Some(false), "a manual folder keeps its direction");
    }

    #[test]
    fn existing_order_maps_keep_their_place() {
        let json = r#"{"id":"A","name":"a","size":1,"btime":1,"mtime":1,"ext":"md","tags":[],"folders":["F1"],"isDeleted":false,"url":"","annotation":"","modificationTime":3,"order":{"F1":"1789479235636.5"},"lastModified":4}"#;
        let mut item = Item::from_json_str(json).unwrap();
        assert_eq!(serde_json::to_string(&item).unwrap(), json);
        assert_eq!(item.manual_order("F1"), Some(1789479235636.5));
        item.set_manual_order("F1", "7");
        item.set_manual_order("F2", "8");
        let out = serde_json::to_string(&item).unwrap();
        assert!(out.contains(r#""modificationTime":3,"order":{"F1":"7","F2":"8"},"lastModified":4}"#), "{out}");
    }
}
