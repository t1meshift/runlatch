//! Rendering of entries as a grouped table or JSON.

use std::collections::BTreeMap;

use comfy_table::{Cell, Color, ContentArrangement, Table, presets::UTF8_FULL};
use runlatch_core::AutostartEntry;

/// Print entries as JSON (a flat array).
pub fn print_json(entries: &[AutostartEntry]) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(entries)?);
    Ok(())
}

/// Print entries as one UTF-8 table per source, in source order.
pub fn print_tables(entries: &[AutostartEntry]) {
    if entries.is_empty() {
        println!("No autostart entries found.");
        return;
    }

    // Group by source, preserving a stable (alphabetical) source order.
    let mut by_source: BTreeMap<&str, Vec<&AutostartEntry>> = BTreeMap::new();
    for entry in entries {
        by_source.entry(&entry.source).or_default().push(entry);
    }

    for (source, mut group) in by_source {
        group.sort_by(|a, b| a.id.cmp(&b.id));
        // Suppress columns that carry no new information in this group.
        let show_name = group.iter().any(|e| e.display_name != e.id);
        let show_command = group
            .iter()
            .any(|e| e.command != e.id && e.command != e.display_name && !e.command.is_empty());

        println!("\n{source}");
        let mut table = Table::new();
        let mut header = vec![Cell::new(""), Cell::new("ID")];
        if show_name {
            header.push(Cell::new("NAME"));
        }
        if show_command {
            header.push(Cell::new("COMMAND"));
        }
        table
            .load_preset(UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(header);

        for entry in group {
            let (mark, color) = if entry.enabled {
                ("●", Color::Green)
            } else {
                ("○", Color::DarkGrey)
            };
            let mut row = vec![Cell::new(mark).fg(color), Cell::new(&entry.id)];
            if show_name {
                row.push(Cell::new(&entry.display_name));
            }
            if show_command {
                row.push(Cell::new(truncate(&entry.command, 50)));
            }
            table.add_row(row);
        }
        println!("{table}");
    }
}

/// Truncate a string to `max` chars with an ellipsis, for table display.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}
