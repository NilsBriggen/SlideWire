//! Translations. User-visible text goes through the `t!` macro with the
//! English text as the key, so a missing translation shows English.
//! `scripts/i18n-extract.py` collects the keys into `locales/en.json`; other
//! languages are `locales/<lang>.json` with the same keys.

/// Use the desktop's language, if we have it.
pub fn init() {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_default();
    // "de_CH.UTF-8" -> "de-CH"; rust-i18n falls back to "de", then English.
    let tag = lang.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    if !tag.is_empty() {
        rust_i18n::set_locale(&tag);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_key_is_in_en_json() {
        let root = env!("CARGO_MANIFEST_DIR");
        let en: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(format!("{root}/locales/en.json")).unwrap()).unwrap();
        // t!( but not format!( or print!(.
        let re = regex::Regex::new(r#"(?m)(?:^|[^A-Za-z0-9_])t!\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
        let mut missing = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(format!("{root}/src"))];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    for c in re.captures_iter(&text) {
                        let key: String = serde_json::from_str(&format!("\"{}\"", &c[1])).unwrap();
                        if !en.contains_key(&key) {
                            missing.push(key);
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "run scripts/i18n-extract.py; missing: {missing:?}");
    }

    /// Every translation has the same keys as English, and each value the
    /// same `%{…}` placeholders as its key.
    #[test]
    fn translations_match_english() {
        let root = env!("CARGO_MANIFEST_DIR");
        let load = |name: &str| -> serde_json::Map<String, serde_json::Value> {
            let text = std::fs::read_to_string(format!("{root}/locales/{name}")).unwrap();
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let en = load("en.json");
        let ph = regex::Regex::new(r"%\{\w+\}").unwrap();
        let placeholders = |s: &str| {
            let mut v: Vec<String> = ph.find_iter(s).map(|m| m.as_str().to_string()).collect();
            v.sort();
            v
        };
        let mut problems = Vec::new();
        for e in std::fs::read_dir(format!("{root}/locales")).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "en.json" {
                continue;
            }
            let tr = load(&name);
            for key in en.keys().filter(|k| !k.starts_with('_')) {
                match tr.get(key).and_then(|v| v.as_str()) {
                    None => problems.push(format!("{name}: missing {key:?}")),
                    Some(v) if placeholders(v) != placeholders(key) => {
                        problems.push(format!("{name}: placeholders differ in {key:?} -> {v:?}"))
                    }
                    Some(_) => {}
                }
            }
            for key in tr.keys().filter(|k| !k.starts_with('_') && !en.contains_key(*k)) {
                problems.push(format!("{name}: extra key {key:?}"));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn interpolation_and_fallback() {
        // A key that isn't translated comes back as itself, filled in.
        assert_eq!(rust_i18n::t!("Bank %{bank}", bank = 3), "Bank 3");
    }
}
