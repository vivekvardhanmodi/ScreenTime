use crate::domain::{TitleRule, WindowInfo};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRule {
    pub target: String,
    pub target_lower: String,
}

#[derive(Debug, Clone, Default)]
pub struct TitleRulesCache {
    rules: HashMap<String, Vec<TargetRule>>,
    last_fetched: Option<Instant>,
}

impl TitleRulesCache {
    pub fn new(raw_rules: HashMap<String, Vec<String>>) -> Self {
        let transformed = raw_rules
            .into_iter()
            .map(|(app_class, targets)| {
                let parsed_targets = targets
                    .into_iter()
                    .map(|t| {
                        let lower = t.to_lowercase();
                        TargetRule {
                            target: t,
                            target_lower: lower,
                        }
                    })
                    .collect();
                (app_class, parsed_targets)
            })
            .collect();

        Self {
            rules: transformed,
            last_fetched: Some(Instant::now()),
        }
    }

    pub fn from_rules(title_rules: &[TitleRule]) -> Self {
        let mut raw: HashMap<String, Vec<String>> = HashMap::new();
        for rule in title_rules {
            raw.entry(rule.app_class.clone())
                .or_default()
                .push(rule.target_title.clone());
        }
        Self::new(raw)
    }

    pub fn empty() -> Self {
        Self {
            rules: HashMap::new(),
            last_fetched: None,
        }
    }

    pub fn is_stale(&self, ttl: Duration) -> bool {
        self.last_fetched.map(|t| t.elapsed() > ttl).unwrap_or(true)
    }

    pub fn last_fetched(&self) -> Option<Instant> {
        self.last_fetched
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn update(&mut self, raw_rules: HashMap<String, Vec<String>>) {
        *self = Self::new(raw_rules);
    }

    pub fn resolve_app_class(&self, raw_class: &str, window_title: Option<&str>) -> String {
        let Some(targets) = self.rules.get(raw_class) else {
            return raw_class.to_string();
        };

        let Some(title) = window_title else {
            return raw_class.to_string();
        };

        let title_lower = title.to_lowercase();
        for rule in targets {
            if title_lower.starts_with(&rule.target_lower) {
                return rule.target.clone();
            }
        }

        raw_class.to_string()
    }

    pub fn resolve_window(&self, window: &WindowInfo) -> String {
        self.resolve_app_class(&window.app_class, Some(&window.title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_rule_resolution() {
        let mut map = HashMap::new();
        map.insert(
            "kitty".to_string(),
            vec!["nvim".to_string(), "cargo".to_string()],
        );
        map.insert(
            "google-chrome".to_string(),
            vec!["GitHub".to_string(), "Stack Overflow".to_string()],
        );

        let cache = TitleRulesCache::new(map);

        assert_eq!(
            cache.resolve_app_class("kitty", Some("nvim src/main.rs")),
            "nvim"
        );
        assert_eq!(
            cache.resolve_app_class("kitty", Some("NVIM — ScreenTime")),
            "nvim"
        );
        assert_eq!(
            cache.resolve_app_class("kitty", Some("cargo test --all")),
            "cargo"
        );
        assert_eq!(cache.resolve_app_class("kitty", Some("bash")), "kitty");
        assert_eq!(cache.resolve_app_class("kitty", None), "kitty");
        assert_eq!(
            cache.resolve_app_class("alacritty", Some("nvim")),
            "alacritty"
        );

        assert_eq!(
            cache.resolve_app_class(
                "google-chrome",
                Some("github - Pull Requests - Google Chrome")
            ),
            "GitHub"
        );
    }

    #[test]
    fn test_staleness_check() {
        let cache = TitleRulesCache::empty();
        assert!(cache.is_stale(Duration::from_secs(60)));

        let mut fresh = TitleRulesCache::new(HashMap::new());
        assert!(!fresh.is_stale(Duration::from_secs(60)));
        fresh.last_fetched = Some(Instant::now() - Duration::from_secs(61));
        assert!(fresh.is_stale(Duration::from_secs(60)));
    }
}
