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

    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    let prefix_score = prefix_match(&query_lower, &target_lower);
    let fuzzy_score = fuzzy_match(&query_lower, &target_lower);

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

pub fn filter_and_rank<'a>(
    query: &str,
    apps: &'a [crate::desktop::DesktopEntry],
) -> Vec<(&'a crate::desktop::DesktopEntry, u32)> {
    if query.is_empty() {
        return apps.iter().map(|app| (app, 0u32)).collect();
    }

    let mut results: Vec<_> = apps
        .iter()
        .filter_map(|app| {
            search(query, &app.name).map(|m| {
                let score = if m.is_prefix { m.score + 200 } else { m.score };
                (app, score)
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::DesktopEntry;

    #[test]
    fn empty_query_returns_all() {
        let apps = vec![
            DesktopEntry {
                name: "Firefox".into(),
                icon: "firefox".into(),
                exec: "firefox".into(),
            },
            DesktopEntry {
                name: "Chrome".into(),
                icon: "chrome".into(),
                exec: "chrome".into(),
            },
        ];
        let results = filter_and_rank("", &apps);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn prefix_match_scores_higher() {
        let apps = vec![
            DesktopEntry {
                name: "Firefox".into(),
                icon: "".into(),
                exec: "".into(),
            },
            DesktopEntry {
                name: "My Firefox".into(),
                icon: "".into(),
                exec: "".into(),
            },
        ];
        let results = filter_and_rank("fire", &apps);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0.name, "Firefox");
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn fuzzy_match_finds_partial() {
        let apps = vec![DesktopEntry {
            name: "Visual Studio Code".into(),
            icon: "".into(),
            exec: "".into(),
        }];
        let results = filter_and_rank("vsc", &apps);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.name, "Visual Studio Code");
    }

    #[test]
    fn no_match_returns_empty() {
        let apps = vec![DesktopEntry {
            name: "Firefox".into(),
            icon: "".into(),
            exec: "".into(),
        }];
        let results = filter_and_rank("chrome", &apps);
        assert!(results.is_empty());
    }

    #[test]
    fn case_insensitive_matching() {
        let apps = vec![DesktopEntry {
            name: "Firefox".into(),
            icon: "".into(),
            exec: "".into(),
        }];
        let results = filter_and_rank("FIRE", &apps);
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
            DesktopEntry {
                name: "Zebra".into(),
                icon: "".into(),
                exec: "".into(),
            },
            DesktopEntry {
                name: "Apple".into(),
                icon: "".into(),
                exec: "".into(),
            },
            DesktopEntry {
                name: "App Store".into(),
                icon: "".into(),
                exec: "".into(),
            },
        ];
        let results = filter_and_rank("app", &apps);
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
            .map(|i| DesktopEntry {
                name: format!("Application {}", i),
                icon: "".into(),
                exec: "".into(),
            })
            .collect();

        let start = std::time::Instant::now();
        let _results = filter_and_rank("app", &apps);
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "Filtering took {:?}, should be under 50ms",
            elapsed
        );
    }
}
