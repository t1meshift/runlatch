//! A small, order-preserving reader/writer for freedesktop Desktop Entry files.
//!
//! This is deliberately minimal: it models a `.desktop` file as a flat list of
//! lines (group headers, `key=value` pairs, and raw comment/blank lines) so that
//! [`set`](DesktopFile::set) and [`remove`](DesktopFile::remove) can edit a single
//! key while leaving the rest of the file — comments, ordering, unrelated keys —
//! byte-for-byte intact. That round-trip fidelity is what lets the XDG provider
//! toggle `Hidden` without rewriting the whole entry.
//!
//! It does not implement the full Desktop Entry spec (no locale strings, no value
//! escaping, no type coercion); it operates purely at the textual key/value level,
//! which is all the providers in this crate need.

/// The conventional main group of an autostart `.desktop` file.
pub const DESKTOP_ENTRY_GROUP: &str = "Desktop Entry";

/// One physical line of a desktop file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// A `[Group Name]` header.
    Group(String),
    /// A `key=value` entry.
    Pair { key: String, value: String },
    /// Anything else: comments, blank lines, or unparsable content, preserved verbatim.
    Raw(String),
}

/// An in-memory, order-preserving view of a `.desktop` file.
#[derive(Debug, Clone, Default)]
pub struct DesktopFile {
    lines: Vec<Line>,
}

impl DesktopFile {
    /// Parse desktop-file text into an editable, order-preserving model.
    pub fn parse(text: &str) -> Self {
        let mut lines = Vec::new();
        for raw in text.lines() {
            let trimmed = raw.trim();
            if let Some(rest) = trimmed.strip_prefix('[')
                && let Some(name) = rest.strip_suffix(']')
            {
                lines.push(Line::Group(name.to_string()));
                continue;
            }
            // A key/value line: `key=value`. Comments (`#`) and blanks fall through
            // to `Raw`. We only treat it as a pair when there's a non-empty key
            // before the first `=` and the line isn't a comment.
            if !trimmed.starts_with('#')
                && let Some((key, value)) = raw.split_once('=')
            {
                let key_trimmed = key.trim();
                if !key_trimmed.is_empty() && !key_trimmed.contains(char::is_whitespace) {
                    lines.push(Line::Pair {
                        key: key_trimmed.to_string(),
                        value: value.to_string(),
                    });
                    continue;
                }
            }
            lines.push(Line::Raw(raw.to_string()));
        }
        Self { lines }
    }

    /// Serialize back to desktop-file text (always newline-terminated).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Group(name) => {
                    out.push('[');
                    out.push_str(name);
                    out.push(']');
                }
                Line::Pair { key, value } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(value);
                }
                Line::Raw(raw) => out.push_str(raw),
            }
            out.push('\n');
        }
        out
    }

    /// Get the value of `key` within `group`, if present.
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        let (start, end) = self.group_range(group)?;
        self.lines[start..end].iter().find_map(|line| match line {
            Line::Pair { key: k, value } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    /// Set `key=value` within `group`, updating in place if the key exists,
    /// appending to the end of the group otherwise. The group is created at the end
    /// of the file if it does not yet exist.
    pub fn set(&mut self, group: &str, key: &str, value: &str) {
        let Some((start, end)) = self.group_range(group) else {
            // No such group: create it, then the key.
            self.lines.push(Line::Group(group.to_string()));
            self.lines.push(Line::Pair {
                key: key.to_string(),
                value: value.to_string(),
            });
            return;
        };

        for line in &mut self.lines[start..end] {
            if let Line::Pair { key: k, value: v } = line
                && k == key
            {
                *v = value.to_string();
                return;
            }
        }

        // Key not found in the group: insert just after the last non-blank line of
        // the group so it stays grouped, rather than after trailing blank lines.
        let insert_at = self.lines[start..end]
            .iter()
            .rposition(|l| !matches!(l, Line::Raw(r) if r.trim().is_empty()))
            .map(|rel| start + rel + 1)
            .unwrap_or(end);
        self.lines.insert(
            insert_at,
            Line::Pair {
                key: key.to_string(),
                value: value.to_string(),
            },
        );
    }

    /// Remove `key` from `group`. Returns `true` if a key was removed.
    pub fn remove(&mut self, group: &str, key: &str) -> bool {
        let Some((start, end)) = self.group_range(group) else {
            return false;
        };
        if let Some(rel) = self.lines[start..end]
            .iter()
            .position(|line| matches!(line, Line::Pair { key: k, .. } if k == key))
        {
            self.lines.remove(start + rel);
            true
        } else {
            false
        }
    }

    /// Find the half-open line range `[start, end)` covering the body of `group`,
    /// where `start` is the line just after the group header and `end` is the next
    /// group header (or end of file).
    fn group_range(&self, group: &str) -> Option<(usize, usize)> {
        let header = self
            .lines
            .iter()
            .position(|l| matches!(l, Line::Group(g) if g == group))?;
        let start = header + 1;
        let end = self.lines[start..]
            .iter()
            .position(|l| matches!(l, Line::Group(_)))
            .map(|rel| start + rel)
            .unwrap_or(self.lines.len());
        Some((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[Desktop Entry]
Type=Application
Name=Example
# a comment
Exec=example --flag
Icon=example
";

    #[test]
    fn parses_and_round_trips() {
        let f = DesktopFile::parse(SAMPLE);
        assert_eq!(f.get(DESKTOP_ENTRY_GROUP, "Name"), Some("Example"));
        assert_eq!(f.get(DESKTOP_ENTRY_GROUP, "Exec"), Some("example --flag"));
        assert_eq!(f.to_text(), SAMPLE);
    }

    #[test]
    fn set_preserves_unrelated_keys_and_order() {
        let mut f = DesktopFile::parse(SAMPLE);
        f.set(DESKTOP_ENTRY_GROUP, "Hidden", "true");
        let out = f.to_text();
        // The new key is appended within the group...
        assert!(out.contains("Hidden=true"));
        // ...and the comment and other keys survive in order.
        assert!(out.contains("# a comment"));
        assert!(out.contains("Name=Example"));
        assert!(out.find("Type=Application").unwrap() < out.find("Hidden=true").unwrap());
    }

    #[test]
    fn set_updates_existing_key_in_place() {
        let mut f = DesktopFile::parse(SAMPLE);
        f.set(DESKTOP_ENTRY_GROUP, "Name", "Renamed");
        assert_eq!(f.get(DESKTOP_ENTRY_GROUP, "Name"), Some("Renamed"));
        // No duplicate key was introduced.
        assert_eq!(f.to_text().matches("Name=").count(), 1);
    }

    #[test]
    fn remove_drops_only_target_key() {
        let mut f = DesktopFile::parse(SAMPLE);
        assert!(f.remove(DESKTOP_ENTRY_GROUP, "Icon"));
        assert_eq!(f.get(DESKTOP_ENTRY_GROUP, "Icon"), None);
        assert!(f.get(DESKTOP_ENTRY_GROUP, "Name").is_some());
        // Removing a missing key is a no-op returning false.
        assert!(!f.remove(DESKTOP_ENTRY_GROUP, "Nonexistent"));
    }

    #[test]
    fn set_creates_missing_group() {
        let mut f = DesktopFile::parse("");
        f.set(DESKTOP_ENTRY_GROUP, "Type", "Application");
        assert_eq!(f.get(DESKTOP_ENTRY_GROUP, "Type"), Some("Application"));
    }
}
