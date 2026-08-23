use std::collections::HashMap;

pub struct MatchResult {
    pub score: u32,
    pub is_prefix: bool,
}

pub fn search(query: &str, target: &str) -> Option<MatchResult> {
    if query.is_empty() {
        return Some(MatchResult {
            score: 0,
            is_prefix: false,
        });
    }

    // Lowered once for standalone callers. The keystroke path goes through
    // [`filter_and_rank`], which lowers the query once per call and matches
    // against precomputed `name_lower`s via [`search_lower`] — no allocation
    // per app.
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();
    search_lower(&query_lower, &target_lower)
}

/// Match an already-lowered query against an already-lowered target.
fn search_lower(query_lower: &str, target_lower: &str) -> Option<MatchResult> {
    let prefix_score = prefix_match(query_lower, target_lower);
    let fuzzy_score = fuzzy_match(query_lower, target_lower);

    if prefix_score.is_some() || fuzzy_score.is_some() {
        let (score, is_prefix) = match (prefix_score, fuzzy_score) {
            (Some(ps), Some(fs)) => {
                if ps > fs {
                    (ps, true)
                } else {
                    (fs, false)
                }
            }
            (Some(ps), None) => (ps, true),
            (None, Some(fs)) => (fs, false),
            (None, None) => unreachable!(),
        };
        Some(MatchResult { score, is_prefix })
    } else {
        None
    }
}

fn prefix_match(query: &str, target: &str) -> Option<u32> {
    if let Some(pos) = target.find(query) {
        let base_score = 100u32;
        let position_bonus = if pos == 0 { 50 } else { 0 };
        let word_boundary_bonus = if pos == 0
            || target
                .chars()
                .nth(pos - 1)
                .is_some_and(|c| c.is_whitespace() || c == '-')
        {
            25
        } else {
            0
        };
        Some(base_score + position_bonus + word_boundary_bonus)
    } else {
        None
    }
}

fn fuzzy_match(query: &str, target: &str) -> Option<u32> {
    let mut query_chars = query.chars().peekable();
    let mut target_chars = target.char_indices().peekable();
    let mut matched_chars = 0;
    let mut score = 0u32;
    let mut last_match_pos = None;

    while let Some(&q_char) = query_chars.peek() {
        let mut found = false;
        while let Some((idx, t_char)) = target_chars.peek() {
            let idx = *idx;
            let t_char = *t_char;
            if q_char == t_char {
                matched_chars += 1;
                let consecutive_bonus = if last_match_pos.is_some_and(|last| last + 1 == idx) {
                    10
                } else {
                    0
                };
                let camel_case_bonus = if t_char.is_uppercase()
                    && idx > 0
                    && target
                        .chars()
                        .nth(idx - 1)
                        .is_some_and(|c| c.is_lowercase())
                {
                    5
                } else {
                    0
                };
                score += 1 + consecutive_bonus + camel_case_bonus;
                last_match_pos = Some(idx);
                query_chars.next();
                target_chars.next();
                found = true;
                break;
            }
            target_chars.next();
        }
        if !found {
            return None;
        }
    }

    if matched_chars == query.chars().count() {
        Some(score)
    } else {
        None
    }
}

/// Filter `apps` against `query` and rank the matches. Returns `(index, score)`
/// pairs indexing back into `apps` — sorted best-first — so callers never have
/// to recover positions (no pointer-identity rescans). Allocation per call: one
/// lowercase of the query plus the results vector.
pub fn filter_and_rank(
    query: &str,
    apps: &[crate::domain::desktop::DesktopEntry],
    history_scores: &HashMap<String, f64>,
) -> Vec<(usize, u32)> {
    if query.is_empty() {
        let mut results: Vec<_> = apps
            .iter()
            .enumerate()
            .map(|(idx, app)| {
                let hs = history_scores.get(&app.id).copied().unwrap_or(0.0);
                let score = (hs * 100.0).round().min(u32::MAX as f64) as u32;
                (idx, score)
            })
            .collect();
        results.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| apps[a.0].name_lower.cmp(&apps[b.0].name_lower))
        });
        return results;
    }

    // Lowered once per keystroke, not once per app; targets are the entries'
    // precomputed `name_lower` fields, so matching allocates nothing.
    let query_lower = query.to_lowercase();
    let mut results: Vec<_> = apps
        .iter()
        .enumerate()
        .filter_map(|(idx, app)| {
            search_lower(&query_lower, &app.name_lower).map(|m| {
                let base = if m.is_prefix { m.score + 200 } else { m.score };
                let hs = history_scores.get(&app.id).copied().unwrap_or(0.0);
                let boost = (hs * 10.0).round().min(u32::MAX as f64) as u32;
                (idx, base + boost)
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| apps[a.0].name_lower.cmp(&apps[b.0].name_lower))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::desktop::DesktopEntry;

    fn entry(id: &str, name: &str) -> DesktopEntry {
        DesktopEntry::new(id, name, "", "")
    }

    #[test]
    fn empty_query_returns_all() {
        let apps = vec![entry("firefox", "Firefox"), entry("chrome", "Chrome")];
        let results = filter_and_rank("", &apps, &HashMap::new());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn prefix_match_scores_higher() {
        let apps = vec![
            entry("firefox", "Firefox"),
            entry("my-firefox", "My Firefox"),
        ];
        let results = filter_and_rank("fire", &apps, &HashMap::new());
        assert_eq!(results.len(), 2);
        assert_eq!(apps[results[0].0].name, "Firefox");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn fuzzy_match_finds_partial() {
        let apps = vec![entry("vscode", "Visual Studio Code")];
        let results = filter_and_rank("vsc", &apps, &HashMap::new());
        assert_eq!(results.len(), 1);
        assert_eq!(apps[results[0].0].name, "Visual Studio Code");
    }

    #[test]
    fn no_match_returns_empty() {
        let apps = vec![entry("firefox", "Firefox")];
        let results = filter_and_rank("chrome", &apps, &HashMap::new());
        assert!(results.is_empty());
    }

    #[test]
    fn case_insensitive_matching() {
        let apps = vec![entry("firefox", "Firefox")];
        let results = filter_and_rank("FIRE", &apps, &HashMap::new());
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn prefix_at_start_scores_highest() {
        let result = search("fire", "Firefox").unwrap();
        assert!(result.is_prefix);
        assert!(result.score > 100);
    }

    #[test]
    fn prefix_in_middle_scores_lower() {
        let result = search("fox", "Firefox").unwrap();
        assert!(result.is_prefix);
        let start_result = search("fire", "Firefox").unwrap();
        assert!(start_result.score > result.score);
    }

    #[test]
    fn fuzzy_consecutive_bonus() {
        let result1 = search("fire", "Firefox").unwrap();
        let result2 = search("fiox", "Firefox").unwrap();
        assert!(result1.score > result2.score);
    }

    #[test]
    fn results_ranked_by_score() {
        let apps = vec![
            entry("zebra", "Zebra"),
            entry("apple", "Apple"),
            entry("app-store", "App Store"),
        ];
        let results = filter_and_rank("app", &apps, &HashMap::new());
        assert_eq!(results.len(), 2);
        assert!(results[0].1 >= results[1].1);
    }

    #[test]
    fn fuzzy_match_returns_none_on_no_match() {
        assert!(search("xyz", "Firefox").is_none());
    }

    #[test]
    fn search_performance_under_50ms() {
        let apps: Vec<_> = (0..1000)
            .map(|i| DesktopEntry::new(format!("app-{}", i), format!("Application {}", i), "", ""))
            .collect();

        let start = std::time::Instant::now();
        let _results = filter_and_rank("app", &apps, &HashMap::new());
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "Filtering took {:?}, should be under 50ms",
            elapsed
        );
    }

    #[test]
    fn results_index_back_into_apps() {
        // filter_and_rank returns indices, not references — they must resolve
        // to the right apps.
        let apps = vec![entry("b", "Bravo"), entry("a", "Alpha")];
        let results = filter_and_rank("", &apps, &HashMap::new());
        assert_eq!(results.len(), 2);
        for &(idx, _) in &results {
            assert!(idx < apps.len());
        }
        assert_eq!(
            apps[results[0].0].name_lower, "alpha",
            "alphabetical tie-break with no history"
        );
    }

    #[test]
    fn empty_query_ranks_by_history() {
        let apps = vec![
            entry("a", "Alpha"),
            entry("b", "Bravo"),
            entry("c", "Charlie"),
        ];
        let mut scores = HashMap::new();
        scores.insert("b".to_string(), 10.0);
        scores.insert("a".to_string(), 1.0);

        let results = filter_and_rank("", &apps, &scores);
        assert_eq!(results.len(), 3);
        assert_eq!(apps[results[0].0].id, "b");
        assert_eq!(apps[results[1].0].id, "a");
        assert_eq!(apps[results[2].0].id, "c");
    }

    #[test]
    fn empty_query_empty_history_sorts_alphabetically() {
        let apps = vec![entry("z", "Zebra"), entry("a", "Apple")];
        let results = filter_and_rank("", &apps, &HashMap::new());
        assert_eq!(apps[results[0].0].name, "Apple");
        assert_eq!(apps[results[1].0].name, "Zebra");
    }

    #[test]
    fn history_boosts_matching_apps() {
        let apps = vec![
            entry("fire-a", "Firefox"),
            entry("fire-b", "Firefox Developer"),
        ];
        let no_history = filter_and_rank("fire", &apps, &HashMap::new());
        let top_no_hist = apps[no_history[0].0].id.clone();

        let mut scores = HashMap::new();
        let other = if top_no_hist == "fire-a" {
            "fire-b"
        } else {
            "fire-a"
        };
        scores.insert(other.to_string(), 50.0);

        let with_history = filter_and_rank("fire", &apps, &scores);
        assert_eq!(apps[with_history[0].0].id, other);
    }

    #[test]
    fn history_does_not_resurrect_non_matches() {
        let apps = vec![entry("firefox", "Firefox")];
        let mut scores = HashMap::new();
        scores.insert("firefox".to_string(), 1000.0);

        let results = filter_and_rank("chrome", &apps, &scores);
        assert!(results.is_empty());
    }
}
