use std::{cmp::Ordering, ops::Range};

const CHANGED_PRIOR: u32 = 1 << 10;
const BASENAME_PRIOR: u32 = 1 << 8;
const MAX_SHALLOWNESS: u32 = 64;

fn config(query: &str) -> neo_frizbee::Config {
    neo_frizbee::Config {
        max_typos: Some(u16::try_from(query.chars().count() / 4).unwrap_or(6).min(6)),
        casing: neo_frizbee::CaseMatching::Smart,
        sort: false,
        ..neo_frizbee::Config::default()
    }
}

pub fn rank(
    query: &str,
    labels: &[&str],
    prior: impl Fn(usize) -> u32,
    limit: usize,
) -> Vec<usize> {
    let mut ranked = if query.is_empty() {
        (0..labels.len())
            .map(|index| (index, 0, prior(index)))
            .collect::<Vec<_>>()
    } else {
        neo_frizbee::match_list(query, labels, &config(query))
            .into_iter()
            .map(|matched| {
                let index = matched.index as usize;
                (index, matched.score, prior(index))
            })
            .collect()
    };
    ranked.sort_unstable_by(
        |(left, left_score, left_prior), (right, right_score, right_prior)| {
            right_score
                .cmp(left_score)
                .then(right_prior.cmp(left_prior))
                .then_with(|| labels[*left].cmp(labels[*right]))
        },
    );
    ranked.truncate(limit);
    ranked.into_iter().map(|(index, _, _)| index).collect()
}

pub fn highlights(query: &str, kept: &[&str]) -> Vec<Vec<Range<usize>>> {
    let mut rows = vec![Vec::new(); kept.len()];
    if query.is_empty() {
        return rows;
    }
    for matched in neo_frizbee::match_list_indices(query, kept, &config(query)) {
        let index = matched.index as usize;
        let Some(label) = kept.get(index) else {
            continue;
        };
        rows[index] = byte_ranges(label, matched.indices);
    }
    rows
}

fn byte_ranges(label: &str, mut indices: Vec<usize>) -> Vec<Range<usize>> {
    indices.sort_unstable();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for index in indices {
        if index >= label.len() {
            continue;
        }
        let start = (0..=index)
            .rev()
            .find(|start| label.is_char_boundary(*start))
            .unwrap_or(0);
        let end = start + label[start..].chars().next().map_or(1, char::len_utf8);
        match ranges.last_mut() {
            Some(last) if last.end >= start => last.end = last.end.max(end),
            _ => ranges.push(start..end),
        }
    }
    ranges
}

pub fn depth(label: &str) -> usize {
    label.trim_end_matches('/').matches('/').count()
}

pub fn in_basename(query: &str, label: &str) -> bool {
    if query.is_empty() || query.contains('/') {
        return false;
    }
    let trimmed = label.trim_end_matches('/');
    let basename = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let case_sensitive = query.chars().any(char::is_uppercase);
    let fold = |character: char| {
        if case_sensitive {
            character
        } else {
            character.to_lowercase().next().unwrap_or(character)
        }
    };
    let mut remaining = query.chars().map(fold).peekable();
    for character in basename.chars().map(fold) {
        if remaining.peek() == Some(&character) {
            remaining.next();
        }
    }
    remaining.peek().is_none()
}

pub fn path_prior(query: &str, label: &str, changed: bool) -> u32 {
    let shallowness =
        u32::try_from(depth(label)).map_or(0, |depth| MAX_SHALLOWNESS.saturating_sub(depth));
    let changed = if changed { CHANGED_PRIOR } else { 0 };
    let basename = if in_basename(query, label) {
        BASENAME_PRIOR
    } else {
        0
    };
    changed + basename + shallowness
}

pub fn browse_order(left: (&str, bool), right: (&str, bool)) -> Ordering {
    right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked<'a>(query: &str, labels: &[&'a str], prior: impl Fn(usize) -> u32) -> Vec<&'a str> {
        rank(query, labels, prior, usize::MAX)
            .into_iter()
            .map(|index| labels[index])
            .collect()
    }

    #[test]
    fn empty_query_orders_by_prior_then_label() {
        let labels = ["b", "a", "c"];
        assert_eq!(
            ranked("", &labels, |index| u32::from(index == 2)),
            ["c", "a", "b"]
        );
        assert_eq!(rank("", &labels, |_| 0, 2), [1, 0]);
    }

    #[test]
    fn prior_breaks_score_ties_and_label_breaks_prior_ties() {
        let labels = ["x/foo/b", "x/foo/a", "x/foo/c"];
        assert_eq!(
            ranked("foo", &labels, |index| u32::from(index == 2)),
            ["x/foo/c", "x/foo/a", "x/foo/b"]
        );
    }

    #[test]
    fn score_outranks_prior() {
        let labels = ["src/m_a_i_n.rs", "src/main.rs"];
        let order = ranked("main", &labels, |index| if index == 0 { 1000 } else { 0 });
        assert_eq!(order, ["src/main.rs", "src/m_a_i_n.rs"]);
    }

    #[test]
    fn misses_are_dropped_and_typos_are_budgeted() {
        let labels = ["alpha", "beta"];
        assert_eq!(ranked("zzz", &labels, |_| 0), Vec::<&str>::new());
        assert_eq!(ranked("alx", &labels, |_| 0), Vec::<&str>::new());
        assert_eq!(ranked("alphx_be", &["alpha_beta"], |_| 0), ["alpha_beta"]);
    }

    #[test]
    fn basename_matches_win_over_directory_matches_on_equal_scores() {
        let labels = ["x/foo/a", "x/a/foo"];
        let query = "foo";
        let order = ranked(query, &labels, |index| {
            path_prior(query, labels[index], false)
        });
        assert_eq!(order, ["x/a/foo", "x/foo/a"]);
        assert!(in_basename("foo", "x/a/foo"));
        assert!(in_basename("fo", "x/a/foo/"));
        assert!(!in_basename("foo", "x/foo/a"));
        assert!(!in_basename("a/foo", "x/a/foo"));
        assert!(in_basename("REA", "docs/README.md"));
        assert!(!in_basename("Rea", "docs/readme.md"));
    }

    #[test]
    fn path_prior_puts_changed_before_basename_before_shallow() {
        assert!(path_prior("q", "deep/a/b/c/x", true) > path_prior("q", "q", false));
        assert!(path_prior("q", "a/b/q", false) > path_prior("q", "q/a", false));
        assert!(path_prior("", "a", false) > path_prior("", "a/b", false));
        assert_eq!(depth("a/b/"), 1);
    }

    #[test]
    fn highlights_are_byte_ranges_on_the_kept_rows() {
        let kept = ["src/main.rs", "héllo/wörld.txt", "nothing"];
        let rows = highlights("main", &kept);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec![4..8]);
        assert!(rows[2].is_empty());
        let rows = highlights("wrld", &kept);
        let label = kept[1];
        for range in &rows[1] {
            assert!(label.is_char_boundary(range.start) && label.is_char_boundary(range.end));
        }
        let matched = rows[1]
            .iter()
            .map(|range| &label[range.clone()])
            .collect::<String>();
        assert_eq!(matched, "wrld");
    }

    #[test]
    fn multibyte_query_characters_cover_whole_characters() {
        let kept = ["dir/wörld"];
        let rows = highlights("ö", &kept);
        assert_eq!(rows[0], vec![5..7]);
        let label = kept[0];
        let matched = rows[0]
            .iter()
            .map(|range| &label[range.clone()])
            .collect::<String>();
        assert_eq!(matched, "ö");
        assert_eq!(byte_ranges("héllo", vec![2, 0, 9, 3]), vec![0..4]);
    }

    #[test]
    fn browse_order_puts_dirs_first_then_names() {
        let mut entries = [("b", false), ("z", true), ("a", false), ("c", true)];
        entries.sort_by(|left, right| browse_order(*left, *right));
        assert_eq!(
            entries,
            [("c", true), ("z", true), ("a", false), ("b", false)]
        );
    }
}
