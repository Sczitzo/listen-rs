//! Pure-Rust native egui renderers for the Local Meeting Notetaker and History screens.

use crate::meeting::MeetingNote;
use crate::storage::HistoryEntry;
use crate::theme::{blend, Palette};
use crate::ui::widgets::{self, Icon};
use crate::ui::{HistoryGrouping, HistoryUiState, MeetingUiState, UiAction, UiView};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, ComboBox, CornerRadius, FontFamily, FontId, Frame,
    Id, Label, Layout, Margin, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui,
};
use std::time::Duration;

const HISTORY_PAGE_SIZE: usize = 60;
const LONG_ENTRY_CHAR_LIMIT: usize = 420;
const LONG_ENTRY_PREVIEW_CHARS: usize = 380;

/// Render the local-first Meeting Notetaker screen.
pub fn render_meetings(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut MeetingUiState,
    actions: &mut Vec<UiAction>,
) {
    if state.active || state.summarizing {
        ui.ctx().request_repaint_after(Duration::from_millis(80));
    }

    // Top hero card
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Header row: status dot + heading on left; audio source pill + timer badge on right
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Furthest right: crisp monospace MM:SS timer badge
                let timer_text = format_timer(state.elapsed_secs);
                let timer_fill = if state.active {
                    blend(p.surface, p.rec, 0.16)
                } else {
                    p.surface2
                };
                let timer_stroke = if state.active {
                    p.rec.gamma_multiply(0.55)
                } else {
                    p.line
                };
                let timer_fg = if state.active { p.rec } else { p.fg };
                Frame::default()
                    .fill(timer_fill)
                    .stroke(Stroke::new(1.0_f32, timer_stroke))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(timer_text)
                                .font(FontId::monospace(13.5))
                                .strong()
                                .color(timer_fg),
                        );
                    });

                // Interactive audio source pill
                let can_toggle = !state.active && !state.summarizing;
                let source_label = if state.include_system_audio {
                    "Mic + System audio"
                } else {
                    "Mic only"
                };
                let pill_resp = audio_source_pill(ui, p, source_label, state.include_system_audio, can_toggle);
                if pill_resp.clicked() && can_toggle {
                    let next = !state.include_system_audio;
                    state.include_system_audio = next;
                    actions.push(UiAction::MeetingToggleSystemAudio(next));
                }

                // Remaining left width: live status dot + bold heading
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    status_dot(ui, p, state.active, state.summarizing, view.time_secs);
                    ui.label(
                        RichText::new("Meeting notes")
                            .font(FontId::new(19.0, FontFamily::Name("humanist".into())))
                            .strong()
                            .color(p.fg),
                    );
                });
            });
        });

        ui.add_space(6.0);
        ui.add(
            Label::new(
                RichText::new(
                    "Capture your mic and system audio locally, then generate a summary and action items.",
                )
                .size(13.5)
                .color(p.muted),
            )
            .wrap(),
        );

        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            if state.summarizing {
                ui.add(egui::Spinner::new().size(15.0).color(p.amber));
                widgets::secondary_btn(ui, p, "Transcribing & summarizing locally…", false);
            } else if state.active {
                if widgets::primary_btn(ui, p, "Stop & summarize", true).clicked() {
                    actions.push(UiAction::MeetingStopAndSummarize);
                }
                if widgets::danger_btn(ui, p, "Cancel", true).clicked() {
                    actions.push(UiAction::MeetingCancel);
                }
            } else if widgets::primary_btn(ui, p, "Start meeting", true).clicked() {
                actions.push(UiAction::MeetingStart);
            }

            let status = state.status_text.trim();
            if !status.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(status).size(12.5).color(p.muted));
            }
        });
    });

    // Section header row
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("SAVED NOTES").size(11.5).strong().color(p.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let count = state.notes.len();
            let count_lbl = if count == 1 {
                "1 note".to_owned()
            } else {
                format!("{count} notes")
            };
            ui.label(RichText::new(count_lbl).size(12.0).color(p.faint));
        });
    });
    ui.add_space(8.0);

    // Empty state or saved notes cards
    if state.notes.is_empty() {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                let (icon_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                widgets::draw_icon(ui.painter(), icon_rect, Icon::Meetings, p.faint);
                ui.add_space(8.0);
                ui.label(RichText::new("No saved meeting notes yet").size(15.0).strong().color(p.fg));
                ui.add_space(4.0);
                ui.add(
                    Label::new(
                        RichText::new(
                            "Completed meetings appear here with private local summaries, action items, and transcripts.",
                        )
                        .size(13.0)
                        .color(p.muted),
                    )
                    .wrap(),
                );
                ui.add_space(12.0);
            });
        });
        return;
    }

    for note in &state.notes {
        ui.push_id(note.id, |ui| {
            render_meeting_note_card(ui, p, note, &mut state.expanded_transcripts, actions);
        });
        ui.add_space(10.0);
    }
}

fn render_meeting_note_card(
    ui: &mut Ui,
    p: &Palette,
    note: &MeetingNote,
    expanded_transcripts: &mut std::collections::HashSet<String>,
    actions: &mut Vec<UiAction>,
) {
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_min_width(ui.available_width());

        // Top row: title on left; date, duration badge, and audio source badge on right
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (src_lbl, src_tint) = if note.loopback_used {
                    ("Mic + System", Some(p.accent))
                } else {
                    ("Mic only", None)
                };
                widgets::badge(ui, p, src_lbl, src_tint);
                widgets::badge(ui, p, &format_duration_secs(note.duration_secs), None);
                let formatted_ts = format_meeting_ts(&note.ts);
                if !formatted_ts.is_empty() {
                    ui.label(RichText::new(formatted_ts).size(12.0).color(p.muted));
                }

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let title = if note.title.trim().is_empty() {
                        "Untitled meeting"
                    } else {
                        note.title.trim()
                    };
                    ui.add(
                        Label::new(RichText::new(title).size(15.5).strong().color(p.fg)).wrap(),
                    );
                });
            });
        });

        // Summary section
        ui.add_space(10.0);
        ui.label(RichText::new("SUMMARY").size(10.5).strong().color(p.faint));
        ui.add_space(3.0);
        if note.summary.trim().is_empty() {
            ui.label(RichText::new("No summary generated.").size(13.5).italics().color(p.muted));
        } else {
            ui.add(
                Label::new(RichText::new(note.summary.trim()).size(13.8).color(p.fg))
                    .wrap()
                    .selectable(true),
            );
        }

        // Action items section
        if !note.action_items.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("ACTION ITEMS").size(10.5).strong().color(p.faint));
            ui.add_space(4.0);
            for item in &note.action_items {
                let clean = item.trim();
                if clean.is_empty() {
                    continue;
                }
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").size(13.5).strong().color(p.accent));
                    ui.add(
                        Label::new(RichText::new(clean).size(13.5).color(p.fg))
                            .wrap()
                            .selectable(true),
                    );
                });
            }
        }

        ui.add_space(10.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Collapsible Transcript toggle
        let key = note.id.to_string();
        let is_open = expanded_transcripts.contains(&key);
        let word_count = note.transcript.split_whitespace().count();
        let toggle_lbl = if word_count > 0 {
            format!("Transcript ({word_count} words)")
        } else {
            "Transcript".to_owned()
        };
        if widgets::disclosure_btn(ui, p, &toggle_lbl, is_open, 12.5, false, p.accent).clicked() {
            if is_open {
                expanded_transcripts.remove(&key);
            } else {
                expanded_transcripts.insert(key.clone());
            }
        }

        if is_open {
            ui.add_space(6.0);
            Frame::default()
                .fill(p.bg)
                .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if note.transcript.trim().is_empty() {
                        ui.label(
                            RichText::new("No transcript recorded.")
                                .size(12.8)
                                .italics()
                                .color(p.muted),
                        );
                    } else {
                        ui.add(
                            Label::new(
                                RichText::new(note.transcript.trim())
                                    .size(13.0)
                                    .color(p.muted),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    }
                });
        }

        // Footer actions row
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::secondary_btn(ui, p, "Copy summary", true).clicked() {
                actions.push(UiAction::CopyText(
                    format_meeting_summary_copy(note),
                    "Copied summary",
                ));
            }
            let has_transcript = !note.transcript.trim().is_empty();
            if widgets::ghost_btn(ui, p, "Copy transcript", has_transcript).clicked() {
                actions.push(UiAction::CopyText(
                    note.transcript.clone(),
                    "Copied transcript",
                ));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if danger_ghost_btn(ui, p, "Delete", true).clicked() {
                    actions.push(UiAction::MeetingDelete(key));
                }
            });
        });
    });
}

/// Render the History & 4-week stats screen with an interactive Jump-to timeline rail.
pub fn render_history(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut HistoryUiState,
    actions: &mut Vec<UiAction>,
) {
    // Top 4-card KPI stats strip (4-column on >= 640px, 2x2 on narrow)
    render_kpi_strip(ui, p, view);

    // Tools row: search box on left, Group by segmented control on right
    ui.add_space(14.0);
    let prev_grouping = state.grouping;
    render_history_tools_row(ui, p, state);
    if state.display_limit == 0 || state.filter != state.last_filter || state.grouping != prev_grouping {
        state.display_limit = HISTORY_PAGE_SIZE;
        state.last_filter = state.filter.clone();
        if state.grouping != prev_grouping {
            state.selected_group = None;
            state.jump_target = None;
        }
    }

    // Filter history entries newest-first across ALL stored entries
    let query = state.filter.trim().to_ascii_lowercase();
    let matched: Vec<&HistoryEntry> = view
        .history
        .iter()
        .rev()
        .filter(|entry| entry_matches_query(entry, &query))
        .collect();
    let total_matches = matched.len();

    ui.add_space(12.0);

    // Empty states
    if view.history.is_empty() {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                let (icon_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                widgets::draw_icon(ui.painter(), icon_rect, Icon::History, p.faint);
                ui.add_space(8.0);
                ui.label(RichText::new("No dictation history yet").size(15.0).strong().color(p.fg));
                ui.add_space(4.0);
                ui.add(
                    Label::new(
                        RichText::new(
                            "Completed dictations appear here with private local search, transcripts, and usage trends.",
                        )
                        .size(13.0)
                        .color(p.muted),
                    )
                    .wrap(),
                );
                ui.add_space(12.0);
            });
        });
    } else if matched.is_empty() {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!("No matches for \"{}\"", state.filter.trim()))
                        .size(14.5)
                        .strong()
                        .color(p.fg),
                );
                ui.add_space(6.0);
                if widgets::secondary_btn(ui, p, "Clear search", true).clicked() {
                    state.filter.clear();
                    state.display_limit = HISTORY_PAGE_SIZE;
                }
                ui.add_space(10.0);
            });
        });
    } else {
        // Group ALL matched entries first so the Jump-to timeline covers every single day/app
        let groups = group_entries(&matched, state.grouping);
        let mut group_starts: Vec<usize> = Vec::with_capacity(groups.len());
        let mut running = 0usize;
        for (_, entries) in &groups {
            group_starts.push(running);
            running += entries.len();
        }
        if state.selected_group.as_deref().is_none_or(|sel| !groups.iter().any(|(k, _)| *k == sel)) {
            state.selected_group = groups.first().map(|(k, _)| (*k).to_owned());
        }

        let avail_w = ui.available_width();
        let show_side_rail = avail_w >= 660.0 && groups.len() > 1;
        let rail_w = 152.0_f32;
        let gap = 14.0_f32;

        if show_side_rail {
            let list_w = (avail_w - rail_w - gap).max(320.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                // Left "Jump to" timeline rail
                ui.allocate_ui_with_layout(vec2(rail_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(rail_w);
                    ui.set_max_width(rail_w);
                    render_jump_rail(ui, p, &groups, &group_starts, state, rail_w);
                });
                // Right history entries column
                ui.allocate_ui_with_layout(vec2(list_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(list_w);
                    ui.set_max_width(list_w);
                    render_history_groups_column(ui, p, &groups, &group_starts, total_matches, state, actions);
                });
            });
        } else {
            if groups.len() > 1 {
                render_compact_jump_bar(ui, p, &groups, &group_starts, state);
                ui.add_space(10.0);
            }
            render_history_groups_column(ui, p, &groups, &group_starts, total_matches, state, actions);
        }
    }

    // Footer bar with two-step clear history confirmation
    ui.add_space(12.0);
    widgets::hline(ui, p);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let total = view.history.len();
        let footer_note = if total == 0 {
            "History stays private on your device.".to_owned()
        } else if total == 1 {
            "1 dictation stored locally.".to_owned()
        } else {
            format!("{} dictations stored locally.", format_count(total))
        };
        ui.label(RichText::new(footer_note).size(12.0).color(p.faint));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if state.confirm_clear {
                if widgets::ghost_btn(ui, p, "Cancel", true).clicked() {
                    state.confirm_clear = false;
                }
                if widgets::danger_btn(ui, p, "Confirm clear", true).clicked() {
                    state.confirm_clear = false;
                    actions.push(UiAction::ClearHistory);
                }
            } else if danger_ghost_btn(ui, p, "Clear history", total > 0).clicked() {
                state.confirm_clear = true;
            }
        });
    });
}

fn render_jump_rail(
    ui: &mut Ui,
    p: &Palette,
    groups: &[(&str, Vec<&HistoryEntry>)],
    group_starts: &[usize],
    state: &mut HistoryUiState,
    rail_w: f32,
) {
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 10))
        .show(ui, |ui| {
            let inner_w = (rail_w - 20.0).max(90.0);
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            let title = match state.grouping {
                HistoryGrouping::Day => "JUMP TO DATE",
                HistoryGrouping::App => "JUMP TO APP",
            };
            ui.label(RichText::new(title).size(10.5).strong().color(p.faint));
            ui.add_space(6.0);

            ScrollArea::vertical()
                .id_salt("history_jump_rail_scroll")
                .max_height(460.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let row_w = (ui.available_width() - 4.0).max(80.0);
                    for (idx, (key, entries)) in groups.iter().enumerate() {
                        let active = state.selected_group.as_deref() == Some(*key);
                        let short_lbl = match state.grouping {
                            HistoryGrouping::Day => format_day_short(key),
                            HistoryGrouping::App => (*key).to_owned(),
                        };
                        let count_str = format_count(entries.len());
                        let (rect, resp) = ui.allocate_exact_size(vec2(row_w, 26.0), Sense::click());
                        let painter = ui.painter();
                        let fill = if active {
                            blend(p.surface, p.accent, 0.16)
                        } else if resp.hovered() {
                            p.surface2
                        } else {
                            Color32::TRANSPARENT
                        };
                        if fill != Color32::TRANSPARENT {
                            painter.rect_filled(rect, CornerRadius::same(7), fill);
                        }
                        if active {
                            painter.rect_filled(
                                Rect::from_min_size(pos2(rect.left() + 2.0, rect.top() + 5.0), vec2(2.5, rect.height() - 10.0)),
                                CornerRadius::same(2),
                                p.accent,
                            );
                        }
                        let lbl_col = if active || resp.hovered() { p.fg } else { p.muted };
                        let count_gal = painter.layout_no_wrap(count_str, FontId::proportional(11.0), p.faint);
                        let max_lbl_w = (rect.width() - count_gal.size().x - 18.0).max(30.0);
                        let truncated = truncate_label(&short_lbl, 16);
                        let lbl_gal = painter.layout_no_wrap(truncated, FontId::proportional(12.0), lbl_col);
                        let _ = max_lbl_w;
                        painter.galley(
                            pos2(rect.left() + 9.0, rect.center().y - lbl_gal.size().y * 0.5),
                            lbl_gal,
                            lbl_col,
                        );
                        painter.galley(
                            pos2(rect.right() - count_gal.size().x - 6.0, rect.center().y - count_gal.size().y * 0.5),
                            count_gal,
                            p.faint,
                        );
                        if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(match state.grouping {
                                HistoryGrouping::Day => format!("{} ({} dictations)", format_day_heading(key), entries.len()),
                                HistoryGrouping::App => format!("{key} ({} dictations)", entries.len()),
                            })
                            .clicked()
                        {
                            let start = group_starts[idx];
                            if start >= state.display_limit {
                                let pages = (start / HISTORY_PAGE_SIZE) + 1;
                                state.display_limit = pages * HISTORY_PAGE_SIZE;
                            }
                            state.collapsed_groups.remove(*key);
                            state.selected_group = Some((*key).to_owned());
                            state.jump_target = Some((*key).to_owned());
                        }
                        ui.add_space(2.0);
                    }
                });
        });
}

fn render_compact_jump_bar(
    ui: &mut Ui,
    p: &Palette,
    groups: &[(&str, Vec<&HistoryEntry>)],
    group_starts: &[usize],
    state: &mut HistoryUiState,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Jump to:").size(12.5).color(p.muted));
        let selected_text = state
            .selected_group
            .as_deref()
            .map(|k| match state.grouping {
                HistoryGrouping::Day => format_day_heading(k),
                HistoryGrouping::App => k.to_owned(),
            })
            .unwrap_or_else(|| "Select…".into());
        ComboBox::from_id_salt("history_compact_jump")
            .width((ui.available_width() - 70.0).clamp(150.0, 280.0))
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for (idx, (key, entries)) in groups.iter().enumerate() {
                    let lbl = match state.grouping {
                        HistoryGrouping::Day => format!("{} ({})", format_day_heading(key), entries.len()),
                        HistoryGrouping::App => format!("{key} ({})", entries.len()),
                    };
                    let selected = state.selected_group.as_deref() == Some(*key);
                    if ui.selectable_label(selected, lbl).clicked() {
                        let start = group_starts[idx];
                        if start >= state.display_limit {
                            let pages = (start / HISTORY_PAGE_SIZE) + 1;
                            state.display_limit = pages * HISTORY_PAGE_SIZE;
                        }
                        state.collapsed_groups.remove(*key);
                        state.selected_group = Some((*key).to_owned());
                        state.jump_target = Some((*key).to_owned());
                    }
                }
            });
    });
}

fn render_history_groups_column(
    ui: &mut Ui,
    p: &Palette,
    groups: &[(&str, Vec<&HistoryEntry>)],
    group_starts: &[usize],
    total_matches: usize,
    state: &mut HistoryUiState,
    actions: &mut Vec<UiAction>,
) {
    let shown_count = total_matches.min(state.display_limit);
    let summary = if shown_count < total_matches {
        format!(
            "Showing {} of {} dictations",
            format_count(shown_count),
            format_count(total_matches)
        )
    } else if total_matches == 1 {
        "1 dictation".to_owned()
    } else {
        format!("{} dictations", format_count(total_matches))
    };
    ui.label(RichText::new(summary).size(12.0).color(p.faint));
    ui.add_space(6.0);

    let mut rendered_entries = 0usize;
    for (group_idx, (key, entries)) in groups.iter().enumerate() {
        let start = group_starts[group_idx];
        if start >= state.display_limit {
            break;
        }
        let budget = state.display_limit.saturating_sub(start);
        let slice = &entries[..entries.len().min(budget)];
        if slice.is_empty() {
            continue;
        }
        if group_idx > 0 {
            ui.add_space(10.0);
        }

        let heading = match state.grouping {
            HistoryGrouping::Day => format_day_heading(key),
            HistoryGrouping::App => (*key).to_owned(),
        };
        let is_open = !state.collapsed_groups.contains(*key);
        let header_resp = ui.horizontal(|ui| {
            let toggle = widgets::disclosure_btn(ui, p, &heading, is_open, 13.5, true, p.fg);
            if toggle.clicked() {
                if is_open {
                    state.collapsed_groups.insert((*key).to_owned());
                } else {
                    state.collapsed_groups.remove(*key);
                }
                state.selected_group = Some((*key).to_owned());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = entries.len();
                let count_str = if n == 1 {
                    "1 dictation".to_owned()
                } else {
                    format!("{} dictations", format_count(n))
                };
                ui.label(RichText::new(count_str).size(11.5).color(p.faint));
            });
            toggle
        });
        if state.jump_target.as_deref() == Some(*key) {
            header_resp.response.scroll_to_me(Some(Align::TOP));
            state.jump_target = None;
        }

        if is_open {
            ui.add_space(6.0);
            for (entry_idx, entry) in slice.iter().enumerate() {
                ui.push_id((group_idx, entry.unix, entry_idx), |ui| {
                    render_history_entry_card(ui, p, entry, &mut state.open_ids, actions);
                });
                ui.add_space(8.0);
                rendered_entries += 1;
            }
        } else {
            rendered_entries += slice.len();
        }
    }

    if total_matches > state.display_limit {
        let remaining = total_matches - state.display_limit;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_btn(
                ui,
                p,
                &format!("Show more ({} remaining)", format_count(remaining)),
                true,
            )
            .clicked()
            {
                state.display_limit += HISTORY_PAGE_SIZE;
            }
            if remaining > HISTORY_PAGE_SIZE
                && widgets::ghost_btn(ui, p, "Load +300", true).clicked()
            {
                state.display_limit += HISTORY_PAGE_SIZE * 5;
            }
        });
    }
    let _ = rendered_entries;
}

fn render_kpi_strip(ui: &mut Ui, p: &Palette, view: &UiView<'_>) {
    let stats = view.history_stats;
    let trend_today: Vec<Option<f64>> = stats.trend_today.iter().map(|v| v.map(|n| n as f64)).collect();
    let trend_words: Vec<Option<f64>> = stats.trend_words.iter().map(|v| v.map(|n| n as f64)).collect();
    let trend_wpm: Vec<Option<f64>> = stats.trend_wpm.iter().map(|v| v.map(|n| n as f64)).collect();

    let today_value = format_count(stats.today);
    let today_sub = if stats.today == 1 {
        "dictation".to_owned()
    } else {
        "dictations".to_owned()
    };

    let week_value = format!("{} words", format_count(stats.words_week));
    let week_sub = if stats.week_count == 1 {
        format!("1 dictation · {:.1} min", stats.minutes_week)
    } else {
        format!("{} dictations · {:.1} min", format_count(stats.week_count), stats.minutes_week)
    };

    let pace_value = if stats.wpm > 0 {
        format!("{} WPM", stats.wpm)
    } else {
        "—".to_owned()
    };
    let pace_sub = if stats.wpm > 0 {
        "speaking pace"
    } else {
        "no recent data"
    };

    let speed_value = if stats.latency > 0.0 {
        format!("{:.1}s", stats.latency)
    } else {
        "—".to_owned()
    };
    let speed_sub = if stats.latency > 0.0 {
        "avg turnaround"
    } else {
        "no recent data"
    };

    let cards: [(&str, String, &str, &[Option<f64>]); 4] = [
        ("TODAY", today_value, &today_sub, &trend_today),
        ("THIS WEEK", week_value, &week_sub, &trend_words),
        ("PACE", pace_value, pace_sub, &trend_wpm),
        ("AVG SPEED", speed_value, speed_sub, &stats.trend_latency),
    ];

    let avail_w = ui.available_width();
    let gap = 10.0;
    let cols = if avail_w >= 640.0 { 4 } else { 2 };
    let card_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(120.0);

    for (row_idx, row) in cards.chunks(cols).enumerate() {
        if row_idx > 0 {
            ui.add_space(gap);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (label, value, subtitle, trend) in row {
                render_kpi_card(ui, p, card_w, label, value, subtitle, trend);
            }
        });
    }
}

fn render_kpi_card(
    ui: &mut Ui,
    p: &Palette,
    width: f32,
    label: &str,
    value: &str,
    subtitle: &str,
    trend: &[Option<f64>],
) {
    let inner_w = (width - 28.0).max(80.0);
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(vec2(inner_w, 92.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(inner_w);
                ui.label(RichText::new(label).size(10.5).strong().color(p.faint));
                ui.add_space(4.0);
                ui.label(RichText::new(value).size(19.0).strong().color(p.fg));
                ui.add_space(2.0);
                ui.label(RichText::new(subtitle).size(11.5).color(p.muted));
                ui.add_space(6.0);
                widgets::sparkline(ui, p, trend, vec2(inner_w, 20.0));
            });
        });
}

fn render_history_tools_row(ui: &mut Ui, p: &Palette, state: &mut HistoryUiState) {
    let avail_w = ui.available_width().max(260.0);
    let group_controls_w = 186.0_f32;
    let gap = 12.0_f32;
    let search_w = (avail_w - group_controls_w - gap).max(160.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;

        // Search box on the left
        Frame::default()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.line))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 5))
            .show(ui, |ui| {
                let inner_w = (search_w - 20.0).max(100.0);
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.horizontal(|ui| {
                    let (icon_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    widgets::draw_icon(ui.painter(), icon_rect, Icon::Search, p.faint);

                    let clear_reserve = if state.filter.is_empty() { 4.0 } else { 26.0 };
                    let input_w = (ui.available_width() - clear_reserve).max(60.0);
                    ui.add(
                        TextEdit::singleline(&mut state.filter)
                            .hint_text("Find words or an app...")
                            .frame(false)
                            .desired_width(input_w),
                    );

                    if !state.filter.is_empty() {
                        let clear_btn = egui::Button::new(
                            RichText::new("×").size(15.0).strong().color(p.muted),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .min_size(vec2(20.0, 20.0));
                        if ui
                            .add(clear_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Clear search")
                            .clicked()
                        {
                            state.filter.clear();
                        }
                    }
                });
            });

        // Right side: "Group by:" + segmented Day | App pill
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Group by:").size(12.5).color(p.muted));
            Frame::default()
                .fill(p.surface2)
                .stroke(Stroke::new(1.0_f32, p.line))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if segment_btn(ui, p, "Day", state.grouping == HistoryGrouping::Day).clicked() {
                            state.grouping = HistoryGrouping::Day;
                        }
                        if segment_btn(ui, p, "App", state.grouping == HistoryGrouping::App).clicked() {
                            state.grouping = HistoryGrouping::App;
                        }
                    });
                });
        });
    });
}

fn render_history_entry_card(
    ui: &mut Ui,
    p: &Palette,
    entry: &HistoryEntry,
    open_ids: &mut std::collections::HashSet<u64>,
    actions: &mut Vec<UiAction>,
) {
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // Top metadata line: time (HH:MM), app badge, duration, outcome badge
            ui.horizontal_wrapped(|ui| {
                let time_str = extract_time_hhmm(&entry.when);
                if !time_str.is_empty() {
                    ui.label(
                        RichText::new(time_str)
                            .font(FontId::monospace(12.0))
                            .color(p.faint),
                    );
                }

                let app = entry.app_name.trim();
                if !app.is_empty() {
                    widgets::badge(ui, p, app, None);
                }

                if entry.seconds > 0.0 {
                    ui.label(
                        RichText::new(format!("{:.1}s", entry.seconds))
                            .size(11.5)
                            .color(p.muted),
                    );
                }

                let outcome = entry.outcome.trim();
                if !outcome.is_empty() {
                    widgets::badge(ui, p, outcome, outcome_tint(p, outcome));
                }
            });

            // Main cleaned text (readable wrapped Label, expandable when long)
            ui.add_space(6.0);
            let main_text = entry.text();
            let expand_id = ui.make_persistent_id(Id::new("hist_expand_text"));
            let char_count = main_text.chars().count();
            if char_count > LONG_ENTRY_CHAR_LIMIT {
                let expanded = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(expand_id))
                    .unwrap_or(false);
                if expanded {
                    ui.add(
                        Label::new(RichText::new(main_text).size(14.0).color(p.fg))
                            .wrap()
                            .selectable(true),
                    );
                    if disclosure_btn(ui, p, "Show less").clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(expand_id, false));
                    }
                } else {
                    let preview: String = main_text
                        .chars()
                        .take(LONG_ENTRY_PREVIEW_CHARS)
                        .collect();
                    ui.add(
                        Label::new(RichText::new(format!("{preview}…")).size(14.0).color(p.fg))
                            .wrap()
                            .selectable(true),
                    );
                    if disclosure_btn(ui, p, "Show full text").clicked() {
                        ui.ctx().data_mut(|d| d.insert_temp(expand_id, true));
                    }
                }
            } else {
                ui.add(
                    Label::new(RichText::new(main_text).size(14.0).color(p.fg))
                        .wrap()
                        .selectable(true),
                );
            }

            // Optional collapsible "Original transcript" when raw differs from cleaned text
            let has_distinct_raw = !entry.raw.trim().is_empty() && entry.raw != main_text;
            if has_distinct_raw {
                ui.add_space(6.0);
                let is_open = open_ids.contains(&entry.unix);
                if widgets::disclosure_btn(ui, p, "Original transcript", is_open, 12.0, false, p.muted).clicked() {
                    if is_open {
                        open_ids.remove(&entry.unix);
                    } else {
                        open_ids.insert(entry.unix);
                    }
                }
                if is_open {
                    ui.add_space(4.0);
                    Frame::default()
                        .fill(p.bg)
                        .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.45)))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(
                                Label::new(RichText::new(&entry.raw).size(12.8).color(p.muted))
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                }
            }

            // Action buttons row
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::secondary_btn(ui, p, "Copy", !main_text.is_empty()).clicked() {
                    actions.push(UiAction::CopyText(main_text.to_owned(), "Copied"));
                }
                if has_distinct_raw
                    && widgets::ghost_btn(ui, p, "Copy original", true).clicked()
                {
                    actions.push(UiAction::CopyText(entry.raw.clone(), "Copied original"));
                }
            });
        });
}

// ---------------------------------------------------------------------------
// Local micro-widgets
// ---------------------------------------------------------------------------

fn status_dot(ui: &mut Ui, p: &Palette, active: bool, summarizing: bool, time_secs: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let base_color = if active {
        p.rec
    } else if summarizing {
        p.amber
    } else {
        p.green
    };
    if active || summarizing {
        let pulse = ((time_secs * 3.6).sin() as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        painter.circle_filled(center, 4.5 + pulse * 2.5, base_color.gamma_multiply(0.28));
    }
    painter.circle_filled(center, 4.2, base_color);
}

fn audio_source_pill(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    system_on: bool,
    enabled: bool,
) -> Response {
    let fill = if system_on {
        blend(p.surface, p.accent, 0.14)
    } else {
        p.surface2
    };
    let fg = if !enabled {
        p.faint
    } else if system_on {
        p.fg
    } else {
        p.muted
    };
    let stroke_col = if system_on && enabled {
        blend(p.line, p.accent, 0.55)
    } else {
        p.line
    };
    let btn = egui::Button::new(RichText::new(label).size(12.0).strong().color(fg))
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, stroke_col))
        .corner_radius(CornerRadius::same(99))
        .min_size(vec2(0.0, 26.0));
    let resp = ui.add_enabled(enabled, btn);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to toggle system audio loopback capture")
    } else {
        resp.on_hover_text("Audio source is locked while a meeting is in progress")
    }
}

fn disclosure_btn(ui: &mut Ui, p: &Palette, label: &str) -> Response {
    let btn = egui::Button::new(RichText::new(label).size(12.5).color(p.muted))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(0.0, 24.0));
    ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn danger_ghost_btn(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> Response {
    let id = ui.make_persistent_id(Id::new(("danger_ghost", text)));
    let was_hovered = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let fg = if !enabled {
        p.faint.gamma_multiply(0.6)
    } else if was_hovered {
        p.rec
    } else {
        p.muted
    };
    let fill = if enabled && was_hovered {
        blend(p.surface, p.rec, 0.14)
    } else {
        Color32::TRANSPARENT
    };
    let b = egui::Button::new(RichText::new(text).size(13.0).color(fg))
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(0.0, 32.0));
    let resp = ui.add_enabled(enabled, b);
    ui.ctx().data_mut(|d| d.insert_temp(id, resp.hovered()));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn segment_btn(ui: &mut Ui, p: &Palette, label: &str, active: bool) -> Response {
    let fill = if active { p.surface } else { Color32::TRANSPARENT };
    let stroke = if active {
        Stroke::new(1.0_f32, blend(p.line, p.accent, 0.55))
    } else {
        Stroke::NONE
    };
    let text = if active {
        RichText::new(label).size(12.5).strong().color(p.fg)
    } else {
        RichText::new(label).size(12.5).color(p.muted)
    };
    let btn = egui::Button::new(text)
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(48.0, 26.0));
    ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Formatting & grouping helpers
// ---------------------------------------------------------------------------

fn format_timer(elapsed_secs: f32) -> String {
    let total = elapsed_secs.max(0.0).floor() as u64;
    let mins = total / 60;
    let secs = total % 60;
    format!("{mins:02}:{secs:02}")
}

fn format_duration_secs(duration_secs: u64) -> String {
    let mins = duration_secs / 60;
    let secs = duration_secs % 60;
    format!("{mins}m {secs:02}s")
}

fn format_meeting_ts(ts: &str) -> String {
    let trimmed = ts.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Convert "YYYY-MM-DDTHH:MM:SS..." or "YYYY-MM-DD HH:MM:SS" to "YYYY-MM-DD HH:MM"
    if trimmed.len() >= 16 && trimmed.is_char_boundary(16) {
        let prefix = &trimmed[..16];
        if prefix.as_bytes().get(4) == Some(&b'-') && prefix.as_bytes().get(7) == Some(&b'-') {
            return prefix.replace('T', " ");
        }
    }
    trimmed.to_owned()
}

fn format_meeting_summary_copy(note: &MeetingNote) -> String {
    let mut out = String::new();
    let title = if note.title.trim().is_empty() {
        "Meeting notes"
    } else {
        note.title.trim()
    };
    out.push_str(title);
    if !note.summary.trim().is_empty() {
        out.push_str("\n\nSummary:\n");
        out.push_str(note.summary.trim());
    }
    if !note.action_items.is_empty() {
        out.push_str("\n\nAction items:\n");
        for item in &note.action_items {
            let clean = item.trim();
            if !clean.is_empty() {
                out.push_str("- ");
                out.push_str(clean);
                out.push('\n');
            }
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }
    out
}

fn entry_matches_query(entry: &HistoryEntry, lower_query: &str) -> bool {
    if lower_query.is_empty() {
        return true;
    }
    entry.text().to_ascii_lowercase().contains(lower_query)
        || entry.raw.to_ascii_lowercase().contains(lower_query)
        || entry.app_name.to_ascii_lowercase().contains(lower_query)
}

fn group_entries<'a>(
    entries: &[&'a HistoryEntry],
    grouping: HistoryGrouping,
) -> Vec<(&'a str, Vec<&'a HistoryEntry>)> {
    let mut groups: Vec<(&'a str, Vec<&'a HistoryEntry>)> = Vec::new();
    for &entry in entries {
        let key = match grouping {
            HistoryGrouping::Day => entry.when.get(..10).unwrap_or("Unknown"),
            HistoryGrouping::App => {
                let app = entry.app_name.trim();
                if app.is_empty() {
                    "Unknown app"
                } else {
                    app
                }
            }
        };
        if let Some((_, list)) = groups.iter_mut().find(|(k, _)| *k == key) {
            list.push(entry);
        } else {
            groups.push((key, vec![entry]));
        }
    }
    groups
}

fn extract_time_hhmm(when: &str) -> &str {
    let trimmed = when.trim();
    if trimmed.len() >= 16 && trimmed.is_char_boundary(11) && trimmed.is_char_boundary(16) {
        return &trimmed[11..16];
    }
    trimmed
}

fn outcome_tint(p: &Palette, outcome: &str) -> Option<Color32> {
    let lower = outcome.to_ascii_lowercase();
    if lower.contains("fail") || lower.contains("error") || lower.contains("cancel") {
        Some(p.rec)
    } else if lower.contains("copy")
        || lower.contains("retain")
        || lower.contains("fallback")
        || lower.contains("clip")
    {
        Some(p.amber)
    } else if lower.contains("paste")
        || lower.contains("typed")
        || lower.contains("insert")
        || lower.contains("ok")
        || lower.contains("sent")
    {
        Some(p.green)
    } else {
        None
    }
}

fn month_short(m: u32) -> Option<&'static str> {
    match m {
        1 => Some("Jan"),
        2 => Some("Feb"),
        3 => Some("Mar"),
        4 => Some("Apr"),
        5 => Some("May"),
        6 => Some("Jun"),
        7 => Some("Jul"),
        8 => Some("Aug"),
        9 => Some("Sep"),
        10 => Some("Oct"),
        11 => Some("Nov"),
        12 => Some("Dec"),
        _ => None,
    }
}

fn parse_ymd(key: &str) -> Option<(u32, u32, u32)> {
    let mut parts = key.split('-');
    let y = parts.next()?.parse::<u32>().ok()?;
    let m = parts.next()?.parse::<u32>().ok()?;
    let d = parts.next()?.parse::<u32>().ok()?;
    if (1..=12).contains(&m) && (1..=31).contains(&d) {
        Some((y, m, d))
    } else {
        None
    }
}

fn format_day_heading(key: &str) -> String {
    if let Some((y, m, d)) = parse_ymd(key) {
        if let Some(mon) = month_short(m) {
            return format!("{mon} {d}, {y}");
        }
    }
    key.to_owned()
}

fn format_day_short(key: &str) -> String {
    if let Some((_, m, d)) = parse_ymd(key) {
        if let Some(mon) = month_short(m) {
            return format!("{mon} {d}");
        }
    }
    key.to_owned()
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn format_count(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_helpers_handle_edge_cases() {
        assert_eq!(format_timer(-3.0), "00:00");
        assert_eq!(format_timer(65.9), "01:05");
        assert_eq!(format_duration_secs(0), "0m 00s");
        assert_eq!(format_duration_secs(125), "2m 05s");
        assert_eq!(format_meeting_ts("2026-11-25T14:32:09Z"), "2026-11-25 14:32");
        assert_eq!(extract_time_hhmm("2026-11-25 09:41:22"), "09:41");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(2500), "2,500");
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_day_heading("2026-11-25"), "Nov 25, 2026");
        assert_eq!(format_day_short("2026-11-25"), "Nov 25");
        assert_eq!(format_day_short("Unknown"), "Unknown");
    }

    #[test]
    fn grouping_preserves_most_recent_order() {
        let e1 = HistoryEntry {
            when: "2026-11-25 14:00:00".into(),
            app_name: "Slack".into(),
            ..Default::default()
        };
        let e2 = HistoryEntry {
            when: "2026-11-25 13:00:00".into(),
            app_name: "VS Code".into(),
            ..Default::default()
        };
        let e3 = HistoryEntry {
            when: "2026-11-24 18:00:00".into(),
            app_name: "Slack".into(),
            ..Default::default()
        };
        let slice = [&e1, &e2, &e3];
        let by_day = group_entries(&slice, HistoryGrouping::Day);
        assert_eq!(by_day.len(), 2);
        assert_eq!(by_day[0].0, "2026-11-25");
        assert_eq!(by_day[0].1.len(), 2);
        assert_eq!(by_day[1].0, "2026-11-24");

        let by_app = group_entries(&slice, HistoryGrouping::App);
        assert_eq!(by_app.len(), 2);
        assert_eq!(by_app[0].0, "Slack");
        assert_eq!(by_app[0].1.len(), 2);
        assert_eq!(by_app[1].0, "VS Code");
    }
}
