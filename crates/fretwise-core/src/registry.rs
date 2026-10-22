//! What the scale, chord and deck registries have in common: JSON files
//! compiled in plus a user folder, parsed with bad files skipped, and
//! sorted into menu groups.

use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

/// One file to parse: a name for messages, and its JSON.
pub(crate) type Source = (String, String);

/// The compiled-in files (from `build.rs`) as sources.
pub(crate) fn builtin_sources(files: &'static [(&str, &str)]) -> impl Iterator<Item = Source> {
    files.iter().map(|(name, json)| (format!("built-in {name}"), json.to_string()))
}

/// The compiled-in files, then every `*.json` in `user_dir` (if it exists)
/// in name order. Files or a folder that can't be read get a message each.
pub(crate) fn sources(files: &'static [(&str, &str)], user_dir: Option<&Path>) -> (Vec<Source>, Vec<String>) {
    let mut sources: Vec<Source> = builtin_sources(files).collect();
    let mut errors = Vec::new();
    if let Some(dir) = user_dir {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                let mut paths: Vec<_> = entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
                    .collect();
                paths.sort();
                for path in paths {
                    match std::fs::read_to_string(&path) {
                        Ok(json) => sources.push((path.display().to_string(), json)),
                        Err(e) => errors.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("{}: {e}", dir.display())),
        }
    }
    (sources, errors)
}

/// Parses every source, skipping (with a message) files that don't parse
/// and ids already taken by an earlier file.
pub(crate) fn parse_all<T>(
    sources: impl IntoIterator<Item = Source>,
    parse: impl Fn(&str, &str) -> Result<T, String>,
    id: impl Fn(&T) -> &str,
) -> (Vec<T>, Vec<String>) {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();
    let mut defs = Vec::new();
    for (source, json) in sources {
        match parse(&source, &json) {
            Ok(def) if !ids.insert(id(&def).to_string()) => {
                errors.push(format!("{source}: id \"{}\" is already taken; skipped", id(&def)))
            }
            Ok(def) => defs.push(def),
            Err(e) => errors.push(e),
        }
    }
    (defs, errors)
}

/// How a grouped file sorts: its group, `order`, name and id.
pub(crate) struct SortKey<'a> {
    pub group: &'a str,
    pub order: i32,
    pub name: &'a str,
    pub id: &'a str,
}

/// Sorts into menu order: groups by the smallest `order` among their
/// members, then by name; within a group by `order`, name, then id.
pub(crate) fn sort_by_group<T>(defs: Vec<T>, key: impl Fn(&T) -> SortKey<'_>) -> Vec<T> {
    let group_order = |group: &str| defs.iter().map(&key).filter(|k| k.group == group).map(|k| k.order).min();
    let group_orders: Vec<i32> = defs.iter().map(|d| group_order(key(d).group).unwrap_or(i32::MAX)).collect();
    let mut keyed: Vec<(i32, T)> = group_orders.into_iter().zip(defs).collect();
    keyed.sort_by(|(ga, a), (gb, b)| {
        let (a, b) = (key(a), key(b));
        (ga, a.group, a.order, a.name, a.id).cmp(&(gb, b.group, b.order, b.name, b.id))
    });
    keyed.into_iter().map(|(_, d)| d).collect()
}

/// Adds the entry about to be pushed at `index` to `groups`, extending the
/// last group if it's the same one.
pub(crate) fn add_to_groups(groups: &mut Vec<(String, Range<usize>)>, group: &str, index: usize) {
    match groups.last_mut() {
        Some((name, range)) if name == group => range.end += 1,
        _ => groups.push((group.to_string(), index..index + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_and_bad_files_are_skipped() {
        let sources = [("a", "1"), ("b", "x"), ("c", "1"), ("d", "2")].map(|(s, j)| (s.to_string(), j.to_string()));
        let parse = |source: &str, json: &str| json.parse::<u32>().map_err(|e| format!("{source}: {e}"));
        let (defs, errors) = parse_all(sources, parse, |_| "same");
        assert_eq!(defs, [1]);
        assert_eq!(errors.len(), 3);
        assert!(errors[1].starts_with("c: id \"same\" is already taken"), "{errors:?}");
    }

    #[test]
    fn missing_user_folder_is_fine() {
        let (sources, errors) = sources(&[("x.json", "{}")], Some(Path::new("no/such/folder")));
        assert_eq!(sources, [("built-in x.json".to_string(), "{}".to_string())]);
        assert!(errors.is_empty());
    }
}
