//! Small case-insensitive FSX configuration reader with quote-aware comments.
use std::collections::BTreeMap;
pub type Section = BTreeMap<String, String>;
#[derive(Debug, Default)]
pub struct Config {
    sections: BTreeMap<String, Section>,
}

fn uncomment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted && (c == ';' || line[i..].starts_with("//")) {
            return &line[..i];
        }
    }
    line
}
impl Config {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut config = Self::default();
        let mut section = String::new();
        for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = uncomment(line).trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                if !line.ends_with(']') || line.len() < 3 {
                    return Err(format!("Invalid CFG section at line {}", number + 1));
                }
                section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("Expected CFG key=value at line {}", number + 1))?;
            if key.trim().is_empty() {
                return Err(format!("Empty CFG key at line {}", number + 1));
            }
            let value = value.trim();
            let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                &value[1..value.len() - 1]
            } else {
                value
            };
            config
                .sections
                .entry(section.clone())
                .or_default()
                .insert(key.trim().to_ascii_lowercase(), value.to_owned());
        }
        Ok(config)
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(&section.to_ascii_lowercase())?
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn section(&self, section: &str) -> Option<&Section> {
        self.sections.get(&section.to_ascii_lowercase())
    }
    pub fn number(&self, section: &str, key: &str) -> Result<f64, String> {
        let value = self
            .get(section, key)
            .ok_or_else(|| format!("Missing [{section}] {key}"))?;
        let n = value
            .parse::<f64>()
            .map_err(|_| format!("Invalid [{section}] {key}: {value}"))?;
        if !n.is_finite() {
            return Err(format!("Non-finite [{section}] {key}"));
        }
        Ok(n)
    }
    pub fn optional_number(&self, section: &str, key: &str, default: f64) -> Result<f64, String> {
        if self.get(section, key).is_some() {
            self.number(section, key)
        } else {
            Ok(default)
        }
    }
}
pub fn decode(bytes: &[u8], windows_1252: bool) -> Result<String, String> {
    if !windows_1252 {
        return std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| {
            "Non-UTF-8 CFG: explicitly select Windows-1252 for a Western legacy installation".into()
        });
    }
    const EXTRA: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    Ok(bytes
        .iter()
        .map(|&n| {
            if (128..160).contains(&n) {
                EXTRA[(n - 128) as usize]
            } else {
                char::from(n)
            }
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_comments_case_and_last_value() {
        let c=Config::parse("\u{feff}[FLTSIM.0] // variant\nTitle=\"A; B // C\" ; trailing\n[Geometry]\narea=1\nAREA=2 // changed").unwrap();
        assert_eq!(c.get("fltsim.0", "title"), Some("A; B // C"));
        assert_eq!(c.number("GEOMETRY", "area").unwrap(), 2.);
        assert!(Config::parse("[broken\narea=2").is_err());
        assert!(Config::parse("[x]\nnot a value").is_err());
    }
    #[test]
    fn invalid_numbers_are_not_defaults() {
        let c = Config::parse("[x]\na=NaN\nb=bad").unwrap();
        assert!(c.optional_number("x", "a", 1.).is_err());
        assert!(c.number("x", "b").is_err());
        assert_eq!(c.optional_number("x", "absent", 1.).unwrap(), 1.);
    }
}
