//! Pure-Rust egui screens for Listen's Dictate and Prompt Lab views,
//! including the side-by-side Compare suggestion modal.

use crate::app::Page;
use crate::engine::{EngineState, Phase};
use crate::storage::PromptProvider;
use crate::theme::{blend, Palette};
use crate::ui::{widgets, LabAssessmentView, LabUiState, UiAction, UiView};
use eframe::egui::{
    self, vec2, Align, Align2, Color32, ComboBox, CornerRadius, FontFamily, FontId, Frame, Key,
    Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
};

const DEMO_LIVE_TEXT: &str = "Okay so for the release notes I want three short sections, what changed, what got faster, and what people should try first. Keep it friendly and";

/// Render the Dictate landing page.
pub fn render_dictate(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    vocab_buf: &mut String,
    show_how: &mut bool,
    recent_open: &mut bool,
    actions: &mut Vec<UiAction>,
) {
    render_recovery_banners(ui, p, view, actions);

    let recording = view.snap.phase == Phase::Recording || view.demo_recording;
    let working = !recording && view.snap.phase == Phase::Working;
    let loading = !recording && !working && matches!(view.snap.engine, EngineState::Loading);
    let unavailable = !recording && !working && matches!(view.snap.engine, EngineState::Unavailable(_));

    // Hero section: horizontal on wide (>= 560px), stacked cleanly on compact (< 560px).
    let wide = ui.available_width() >= 560.0;
    if wide {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            if widgets::hotkey_card(ui, p, view.hotkey_parts, recording).clicked() {
                actions.push(UiAction::OpenHotkeyPicker);
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                render_hero_details(
                    ui, p, view, recording, working, loading, unavailable, show_how, actions,
                );
            });
        });
    } else {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            if widgets::hotkey_card(ui, p, view.hotkey_parts, recording).clicked() {
                actions.push(UiAction::OpenHotkeyPicker);
            }
            ui.add_space(12.0);
            render_hero_details(
                ui, p, view, recording, working, loading, unavailable, show_how, actions,
            );
        });
    }

    ui.add_space(16.0);
    widgets::hline(ui, p);

    // Live provisional transcript box (shown while recording or in demo mode).
    if recording {
        ui.add_space(14.0);
        render_live_transcript(ui, p, view);
    }

    // Vocabulary section.
    ui.add_space(14.0);
    render_vocabulary_card(ui, p, view, vocab_buf, actions);

    // Recent collapsible card.
    ui.add_space(14.0);
    render_recent_card(ui, p, view, recent_open, actions);
}

fn render_recovery_banners(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    actions: &mut Vec<UiAction>,
) {
    if view.clipboard_recovery {
        Frame::default()
            .fill(blend(p.surface, p.amber, 0.12))
            .stroke(Stroke::new(1.0_f32, blend(p.surface, p.amber, 0.55)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Previous clipboard available")
                        .size(13.5)
                        .strong()
                        .color(p.amber),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new("Listen saved your earlier clipboard before pasting a dictation.")
                        .size(12.5)
                        .color(p.muted),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if widgets::secondary_btn(ui, p, "Restore previous clipboard", true).clicked() {
                        actions.push(UiAction::RestoreClipboard);
                    }
                    if widgets::ghost_btn(ui, p, "Keep current clipboard", true).clicked() {
                        actions.push(UiAction::DiscardClipboard);
                    }
                });
            });
        ui.add_space(12.0);
    }

    for (idx, (text, reason)) in view.retained.iter().enumerate() {
        Frame::default()
            .fill(blend(p.surface, p.amber, 0.10))
            .stroke(Stroke::new(1.0_f32, blend(p.surface, p.amber, 0.55)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let heading = if reason.trim().is_empty() {
                    "Kept here — could not paste".to_owned()
                } else {
                    format!("Kept here — {reason}")
                };
                ui.label(RichText::new(heading).size(13.5).strong().color(p.amber));
                ui.add_space(4.0);
                ui.label(RichText::new(text).size(14.0).color(p.fg));
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if widgets::secondary_btn(ui, p, "Copy", true).clicked() {
                        actions.push(UiAction::CopyText(text.clone(), "Copied"));
                    }
                    if widgets::ghost_btn(ui, p, "Dismiss", true).clicked() {
                        actions.push(UiAction::DismissRetained(idx));
                    }
                });
            });
        ui.add_space(10.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_hero_details(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    recording: bool,
    working: bool,
    loading: bool,
    unavailable: bool,
    show_how: &mut bool,
    actions: &mut Vec<UiAction>,
) {
    let humanist = FontFamily::Name("humanist".into());
    let headline = if recording {
        "Listening"
    } else if working {
        "Working…"
    } else if loading {
        "Getting ready…"
    } else {
        "Ready when you are"
    };

    let (status_text, status_color, pulse) = if recording {
        ("listening", p.rec, true)
    } else if working {
        ("working", p.amber, false)
    } else if loading {
        ("loading", p.amber, false)
    } else if unavailable {
        ("speech offline", p.rec, false)
    } else if view.settings.cleanup && !view.ollama_available {
        ("idle · AI formatting offline", p.amber, false)
    } else {
        ("idle", p.green, false)
    };

    // Top row: waveform instrument + big headline + right-aligned status dot & text.
    ui.horizontal(|ui| {
        widgets::quiet_instrument(ui, p, recording, view.time_secs);
        ui.add_space(4.0);
        ui.label(
            RichText::new(headline)
                .font(FontId::new(26.0, humanist))
                .color(p.fg),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            ui.label(RichText::new(status_text).size(12.0).color(p.muted));
            draw_status_dot(ui, status_color, pulse, view.time_secs);
            if recording {
                if let Some(started) = view.snap.started {
                    let secs = started.elapsed().as_secs();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("{}:{:02}", secs / 60, secs % 60))
                            .font(FontId::monospace(13.0))
                            .color(p.rec),
                    );
                }
            }
        });
    });

    ui.add_space(6.0);

    // Subtitle instruction.
    let subtitle = if recording {
        "Release the hotkey to finish. Esc cancels.".to_owned()
    } else if working {
        if view.snap.status.trim().is_empty() {
            "Turning your speech into tidy text.".to_owned()
        } else {
            view.snap.status.clone()
        }
    } else if loading {
        "Loading local speech engine…".to_owned()
    } else if let EngineState::Unavailable(reason) = &view.snap.engine {
        if reason.trim().is_empty() {
            "Speech engine unavailable. Check Settings → Voice & AI.".to_owned()
        } else {
            format!("Speech engine unavailable ({reason}). Check Settings → Voice & AI.")
        }
    } else {
        format!(
            "Press {} to start. Press it again to finish.",
            view.hotkey_display
        )
    };
    ui.label(
        RichText::new(subtitle)
            .size(14.0)
            .line_height(Some(21.0))
            .color(p.muted),
    );

    ui.add_space(12.0);

    // Action row: Finish recording, Cancel recording, How it works disclosure, and live meter bars.
    let mut bars_drawn = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if widgets::primary_btn(ui, p, "Finish recording", recording).clicked() {
            actions.push(UiAction::FinishDictation);
        }
        if widgets::ghost_btn(ui, p, "Cancel recording", recording).clicked() {
            actions.push(UiAction::CancelDictation);
        }
        if widgets::disclosure_btn(ui, p, "How it works", *show_how, 13.0, false, p.accent).clicked() {
            *show_how = !*show_how;
        }

        if recording && ui.available_width() >= 90.0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::hero_bars(ui, p, view.levels, true, view.time_secs);
            });
            bars_drawn = true;
        }
    });
    if recording && !bars_drawn {
        ui.add_space(6.0);
        widgets::hero_bars(ui, p, view.levels, true, view.time_secs);
    }

    if *show_how {
        ui.add_space(8.0);
        Frame::default()
            .fill(p.surface2)
            .stroke(Stroke::new(1.0_f32, p.line_soft()))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let hk = view.hotkey_display;
                ui.label(
                    RichText::new(format!(
                        "Hold {hk} to dictate and release to finish, or tap {hk} once for hands-free recording and tap again when done. Press Esc at any time to cancel and discard. Begin with \"Hey, prompt engineer this\" followed by your request to turn spoken ideas directly into an engineered prompt."
                    ))
                    .size(12.5)
                    .line_height(Some(19.0))
                    .color(p.muted),
                );
            });
    }
}

fn render_live_transcript(ui: &mut Ui, p: &Palette, view: &UiView<'_>) {
    widgets::frame_titled(
        ui,
        p,
        "Recording transcript · provisional",
        None,
        true,
        |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Provisional. Final text is generated when you finish.")
                        .size(12.0)
                        .color(p.faint),
                );
                if !view.snap.live_note.trim().is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&view.snap.live_note).size(12.0).color(p.muted));
                    });
                }
            });
            ui.add_space(8.0);
            Frame::default()
                .fill(p.surface2)
                .stroke(Stroke::new(1.0_f32, p.line_faint()))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ScrollArea::vertical()
                        .id_salt("dictate_live_transcript")
                        .min_scrolled_height(76.0)
                        .max_height(180.0)
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let live_text = if !view.snap.live_text.is_empty() {
                                Some(view.snap.live_text.as_str())
                            } else if view.demo_recording {
                                Some(DEMO_LIVE_TEXT)
                            } else {
                                None
                            };

                            if let Some(text) = live_text {
                                ui.label(
                                    RichText::new(text)
                                        .size(16.0)
                                        .line_height(Some(23.0))
                                        .color(p.fg),
                                );
                            } else {
                                let placeholder = if !view.settings.live_preview {
                                    "Live preview is off. Turn it on in Settings → General."
                                } else {
                                    "Words appear here a couple of seconds after you start speaking…"
                                };
                                ui.label(
                                    RichText::new(placeholder)
                                        .italics()
                                        .size(15.0)
                                        .color(p.faint),
                                );
                            }
                        });
                });
        },
    );
}

fn render_vocabulary_card(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    vocab_buf: &mut String,
    actions: &mut Vec<UiAction>,
) {
    let vocab_id = egui::Id::new("dictate_vocab_editor");
    let focused = ui.ctx().memory(|m| m.has_focus(vocab_id));

    widgets::frame_titled(ui, p, "Vocabulary", None, focused, |ui| {
        ui.label(
            RichText::new("Names and terms to spell accurately, comma or line separated.")
                .size(12.5)
                .color(p.muted),
        );
        ui.add_space(6.0);
        let resp = ui.add(
            TextEdit::multiline(vocab_buf)
                .id(vocab_id)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 8))
                .hint_text("e.g. Parakeet, Ollama, Kubernetes"),
        );
        if resp.changed() {
            actions.push(UiAction::UpdateVocab(vocab_buf.clone()));
        }

        let term_count = vocab_buf
            .split([',', '\n'])
            .filter(|w| !w.trim().is_empty())
            .count();
        if term_count > 0 {
            ui.add_space(4.0);
            let count_note = match term_count {
                1 => "1 term · saved automatically".to_owned(),
                n => format!("{n} terms · saved automatically"),
            };
            ui.label(RichText::new(count_note).size(12.0).color(p.faint));
        }

        if !view.vocab_suggestions.is_empty() {
            ui.add_space(10.0);
            ui.label(
                RichText::new("Seen often, not in your vocabulary:")
                    .size(12.0)
                    .color(p.muted),
            );
            ui.add_space(6.0);
            render_vocab_chips(ui, p, view.vocab_suggestions, actions);
        }
    });
}

/// Renders atomic, non-breaking suggestion pills wrapped cleanly into explicit horizontal rows.
fn render_vocab_chips(
    ui: &mut Ui,
    p: &Palette,
    words: &[String],
    actions: &mut Vec<UiAction>,
) {
    let avail_w = ui.available_width().max(120.0);
    let gap_x = 6.0_f32;
    let gap_y = 6.0_f32;
    let chip_h = 26.0_f32;
    let font = FontId::proportional(12.5);
    let btn_font = FontId::proportional(13.0);

    // Measure every pill atomically first so no chip ever breaks mid-word or overflows the card
    let measured: Vec<(&String, f32, f32)> = words
        .iter()
        .map(|w| {
            let tw = ui.fonts(|f| f.layout_no_wrap(w.clone(), font.clone(), p.fg).size().x);
            let chip_w = (tw + 52.0).min(avail_w);
            (w, tw, chip_w)
        })
        .collect();

    let mut rows: Vec<Vec<(&String, f32, f32)>> = Vec::new();
    let mut cur_row: Vec<(&String, f32, f32)> = Vec::new();
    let mut cur_w = 0.0_f32;
    for item in measured {
        let needed = if cur_row.is_empty() { item.2 } else { gap_x + item.2 };
        if !cur_row.is_empty() && cur_w + needed > avail_w {
            rows.push(std::mem::take(&mut cur_row));
            cur_w = 0.0;
        }
        cur_w += if cur_row.is_empty() { item.2 } else { gap_x + item.2 };
        cur_row.push(item);
    }
    if !cur_row.is_empty() {
        rows.push(cur_row);
    }

    for (r_idx, row) in rows.into_iter().enumerate() {
        if r_idx > 0 {
            ui.add_space(gap_y);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap_x;
            for (word, tw, chip_w) in row {
                let (rect, _) = ui.allocate_exact_size(vec2(chip_w, chip_h), Sense::hover());
                let add_rect = Rect::from_min_size(
                    egui::pos2(rect.right() - 40.0, rect.top()),
                    vec2(19.0, chip_h),
                );
                let dismiss_rect = Rect::from_min_size(
                    egui::pos2(rect.right() - 21.0, rect.top()),
                    vec2(19.0, chip_h),
                );
                let add_resp = ui
                    .interact(add_rect, ui.make_persistent_id(("vocab_add", word)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Add to vocabulary");
                let dismiss_resp = ui
                    .interact(dismiss_rect, ui.make_persistent_id(("vocab_no", word)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Dismiss suggestion");

                let hovered = add_resp.hovered() || dismiss_resp.hovered();
                let painter = ui.painter();
                painter.rect_filled(
                    rect,
                    CornerRadius::same(99),
                    if hovered { blend(p.surface2, p.accent, 0.10) } else { p.surface2 },
                );
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(99),
                    Stroke::new(1.0_f32, if hovered { p.accent.gamma_multiply(0.75) } else { p.line }),
                    StrokeKind::Inside,
                );
                // Word text
                let text_x = rect.left() + 10.0;
                let max_text_w = (chip_w - 50.0).max(20.0);
                let galley = painter.layout_no_wrap(word.clone(), font.clone(), p.fg);
                let _ = (tw, max_text_w);
                painter.galley(
                    egui::pos2(text_x, rect.center().y - galley.size().y * 0.5),
                    galley,
                    p.fg,
                );
                // '+' action
                let plus_col = if add_resp.hovered() { p.fg } else { p.accent };
                painter.text(add_rect.center(), Align2::CENTER_CENTER, "+", btn_font.clone(), plus_col);
                // '×' action
                let x_col = if dismiss_resp.hovered() { p.rec } else { p.muted };
                painter.text(dismiss_rect.center(), Align2::CENTER_CENTER, "×", btn_font.clone(), x_col);

                if add_resp.clicked() {
                    actions.push(UiAction::AcceptVocabSuggestion(word.clone()));
                }
                if dismiss_resp.clicked() {
                    actions.push(UiAction::DismissVocabSuggestion(word.clone()));
                }
            }
        });
    }
}

fn render_recent_card(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    recent_open: &mut bool,
    actions: &mut Vec<UiAction>,
) {
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if widgets::disclosure_btn(ui, p, "Recent", *recent_open, 15.0, true, p.fg).clicked() {
                *recent_open = !*recent_open;
            }
            if !view.history.is_empty() {
                ui.label(
                    RichText::new(format!("({})", view.history.len().min(6)))
                        .size(12.0)
                        .color(p.faint),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let all_resp = ui
                    .add(
                        egui::Label::new(RichText::new("All history").size(13.0).color(p.accent))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if all_resp.clicked() {
                    actions.push(UiAction::GoPage(Page::History));
                }
            });
        });

        if !*recent_open {
            return;
        }

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(6.0);

        if view.history.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Nothing yet. Press {} and say something.",
                    view.hotkey_display
                ))
                .size(13.0)
                .color(p.muted),
            );
            return;
        }

        let icon_map = widgets::build_app_icon_map(view.history);
        let wide_rows = ui.available_width() >= 540.0;
        for (idx, entry) in view.history.iter().rev().take(6).enumerate() {
            if idx > 0 {
                ui.add_space(4.0);
                widgets::hline(ui, p);
                ui.add_space(4.0);
            }
            let text = entry.text();
            let clipped = clip_text(text, 180);
            let time = short_time(&entry.when);
            let app_icon = entry.app_icon.as_deref().or_else(|| {
                let key = entry.app_name.trim().to_ascii_lowercase();
                icon_map.get(&key).copied()
            });

            if wide_rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if !entry.app_name.trim().is_empty() {
                        widgets::app_icon(ui, p, &entry.app_name, app_icon, 20.0);
                    }
                    ui.label(
                        RichText::new(time)
                            .font(FontId::monospace(12.0))
                            .color(p.muted),
                    );
                    let text_w = (ui.available_width() - 68.0).max(80.0);
                    ui.allocate_ui(vec2(text_w, 0.0), |ui| {
                        ui.label(RichText::new(&clipped).size(14.0).color(p.fg));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::ghost_btn(ui, p, "Copy", true).clicked() {
                            actions.push(UiAction::CopyText(text.to_owned(), "Copied"));
                        }
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if !entry.app_name.trim().is_empty() {
                        widgets::app_icon(ui, p, &entry.app_name, app_icon, 20.0);
                    }
                    ui.label(
                        RichText::new(time)
                            .font(FontId::monospace(12.0))
                            .color(p.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::ghost_btn(ui, p, "Copy", true).clicked() {
                            actions.push(UiAction::CopyText(text.to_owned(), "Copied"));
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(RichText::new(&clipped).size(14.0).color(p.fg));
            }
        }
    });
}

/// Render the Prompt Lab screen and optional Compare suggestion modal.
pub fn render_lab(
    ui: &mut Ui,
    ctx: &egui::Context,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut LabUiState,
    actions: &mut Vec<UiAction>,
) {
    let busy = !state.busy_label.is_empty() || view.snap.lab_busy;
    let has_draft = !state.draft.trim().is_empty();

    // Header bar: active Prompt Target pill + model summary on left, New draft + Library on right.
    render_lab_header(ui, p, view, state, busy, actions);

    // Inline Favorites Library panel when toggled open.
    if state.show_library {
        ui.add_space(10.0);
        render_favorites_panel(ui, p, view, state, actions);
    }

    // Draft card with score badge, saved note, revisions selector, and multiline editor.
    ui.add_space(12.0);
    render_draft_card(ui, p, state, actions);

    // Primary action row.
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if widgets::primary_btn(ui, p, "Engineer", has_draft && !busy).clicked() {
            actions.push(UiAction::LabEngineer);
        }
        if widgets::secondary_btn(ui, p, "Suggest changes", has_draft && !busy).clicked() {
            actions.push(UiAction::LabSuggestChanges);
        }
        if widgets::secondary_btn(ui, p, "Rewrite", has_draft && !busy).clicked() {
            actions.push(UiAction::LabRewriteScratch);
        }
        if widgets::secondary_btn(ui, p, "Copy", has_draft).clicked() {
            actions.push(UiAction::CopyText(state.draft.clone(), "Copied draft"));
        }
        if widgets::secondary_btn(ui, p, "Paste", has_draft).clicked() {
            actions.push(UiAction::LabPasteDraft);
        }
        if widgets::ghost_btn(ui, p, "Save favorite", has_draft).clicked() {
            state.save_fav_open = !state.save_fav_open;
        }
    });

    // Inline Save Favorite prompt when toggled open.
    if state.save_fav_open {
        ui.add_space(10.0);
        render_save_favorite_prompt(ui, p, state, has_draft, actions);
    }

    // Busy / Error / Candidate bar.
    render_lab_status_bars(ui, p, state, busy, actions);

    // Collapsible Earlier draft tools section.
    ui.add_space(14.0);
    render_earlier_tools(ui, p, state, has_draft, busy, actions);

    // Side-by-side Compare suggestion modal.
    if state.compare_open && state.candidate.is_some() {
        render_compare_modal(ctx, p, state, busy, actions);
    }
}

fn render_lab_header(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut LabUiState,
    busy: bool,
    actions: &mut Vec<UiAction>,
) {
    let engine_local = if view.settings.engine_local_model.trim().is_empty() {
        view.settings.local_model.as_str()
    } else {
        view.settings.engine_local_model.as_str()
    };
    let judge_local = if view.settings.judge_local_model.trim().is_empty() {
        view.settings.local_model.as_str()
    } else {
        view.settings.judge_local_model.as_str()
    };
    let writing = provider_model_summary(
        view.settings.prompt_engine,
        engine_local,
        &view.settings.claude_model,
    );
    let checking = provider_model_summary(
        view.settings.prompt_judge,
        judge_local,
        &view.settings.claude_model,
    );
    let model_summary = format!("Writing: {writing} · Checking: {checking}");
    let lib_label = if view.favorites.is_empty() {
        "Library".to_owned()
    } else {
        format!("Library ({})", view.favorites.len())
    };

    let wide = ui.available_width() >= 580.0;
    if wide {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            widgets::badge(ui, p, view.settings.prompt_target.label(), Some(p.accent));
            ui.label(RichText::new(&model_summary).size(12.5).color(p.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let lib_btn = if state.show_library {
                    widgets::primary_btn(ui, p, &lib_label, true)
                } else {
                    widgets::secondary_btn(ui, p, &lib_label, true)
                };
                if lib_btn.clicked() {
                    state.show_library = !state.show_library;
                }
                if widgets::secondary_btn(ui, p, "New draft", !busy).clicked() {
                    actions.push(UiAction::LabNewDraft);
                }
            });
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            widgets::badge(ui, p, view.settings.prompt_target.label(), Some(p.accent));
            ui.label(RichText::new(&model_summary).size(12.5).color(p.muted));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_btn(ui, p, "New draft", !busy).clicked() {
                actions.push(UiAction::LabNewDraft);
            }
            let lib_btn = if state.show_library {
                widgets::primary_btn(ui, p, &lib_label, true)
            } else {
                widgets::secondary_btn(ui, p, &lib_label, true)
            };
            if lib_btn.clicked() {
                state.show_library = !state.show_library;
            }
        });
    }
}

fn render_favorites_panel(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut LabUiState,
    actions: &mut Vec<UiAction>,
) {
    let count_badge = format!("{} saved", view.favorites.len());
    widgets::frame_titled(ui, p, "Library", Some(&count_badge), false, |ui| {
        if view.favorites.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "No saved favorites yet. Use Save favorite below your draft to store reusable prompts.",
                    )
                    .size(13.0)
                    .color(p.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::ghost_btn(ui, p, "Close", true).clicked() {
                        state.show_library = false;
                    }
                });
            });
            return;
        }

        ScrollArea::vertical()
            .id_salt("lab_favorites_scroll")
            .max_height(240.0)
            .show(ui, |ui| {
                for (idx, fav) in view.favorites.iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(6.0);
                        widgets::hline(ui, p);
                        ui.add_space(6.0);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&fav.name).size(14.0).strong().color(p.fg));
                        if fav.uses > 0 {
                            widgets::badge(ui, p, &format!("{} uses", fav.uses), None);
                        }
                        if !fav.when.is_empty() {
                            ui.label(RichText::new(&fav.when).size(11.5).color(p.faint));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::danger_btn(ui, p, "Delete", true).clicked() {
                                actions.push(UiAction::LabDeleteFavorite(idx));
                            }
                            if widgets::secondary_btn(ui, p, "Copy", true).clicked() {
                                actions.push(UiAction::CopyText(
                                    fav.text.clone(),
                                    "Copied favorite",
                                ));
                            }
                            if widgets::secondary_btn(ui, p, "Load", true).clicked() {
                                actions.push(UiAction::LabLoadFavorite(idx));
                                state.show_library = false;
                            }
                        });
                    });
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(clip_text(&fav.text, 160))
                            .size(12.5)
                            .color(p.muted),
                    );
                }
            });
    });
}

fn render_draft_card(
    ui: &mut Ui,
    p: &Palette,
    state: &mut LabUiState,
    actions: &mut Vec<UiAction>,
) {
    let score_badge = match state.assessment.as_ref().and_then(|a| a.score) {
        Some(n) => format!("Score {n}/10"),
        None => "Not scored".to_owned(),
    };
    let draft_id = egui::Id::new("lab_draft_editor");
    let focused = ui.ctx().memory(|m| m.has_focus(draft_id));

    widgets::frame_titled(ui, p, "Draft", Some(&score_badge), focused, |ui| {
        ui.horizontal(|ui| {
            let saved_text = if state.saved_note.trim().is_empty() {
                "✓ Saved locally"
            } else {
                state.saved_note.as_str()
            };
            let saved_color = if saved_text.starts_with('✓') {
                p.green
            } else {
                p.muted
            };
            ui.label(RichText::new(saved_text).size(12.0).color(saved_color));

            let words = state.draft.split_whitespace().count();
            ui.label(
                RichText::new(format!("· {words} words"))
                    .size(12.0)
                    .color(p.faint),
            );

            if state.revisions.len() > 1 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let selected_label = state
                        .revisions
                        .iter()
                        .find(|(idx, _)| *idx == state.current_rev)
                        .map(|(_, lbl)| lbl.as_str())
                        .or_else(|| {
                            if state.version_label.is_empty() {
                                None
                            } else {
                                Some(state.version_label.as_str())
                            }
                        })
                        .unwrap_or("Current draft");

                    ComboBox::from_id_salt("lab_revisions_combo")
                        .width(190.0)
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (rev_idx, rev_label) in &state.revisions {
                                let is_cur = state.current_rev == *rev_idx;
                                if ui.selectable_label(is_cur, rev_label).clicked() && !is_cur {
                                    actions.push(UiAction::LabSelectRevision(*rev_idx));
                                }
                            }
                        });
                });
            } else if !state.version_label.trim().is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(&state.version_label)
                            .size(12.0)
                            .color(p.faint),
                    );
                });
            }
        });

        ui.add_space(8.0);
        let resp = ui.add(
            TextEdit::multiline(&mut state.draft)
                .id(draft_id)
                .desired_rows(9)
                .desired_width(f32::INFINITY)
                .margin(Margin::same(12))
                .hint_text("Dictate, paste, or type your prompt draft here…"),
        );
        if resp.changed() {
            actions.push(UiAction::LabDraftChanged(state.draft.clone()));
        }

        if let Some(assess) = &state.assessment {
            if assess.score.is_some()
                || !assess.summary.trim().is_empty()
                || !assess.gaps.is_empty()
                || !assess.criteria.is_empty()
            {
                ui.add_space(10.0);
                Frame::default()
                    .fill(p.surface2)
                    .stroke(Stroke::new(1.0_f32, p.line_faint()))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        render_assessment_summary(ui, p, assess, true);
                    });
            }
        }
    });
}

fn render_save_favorite_prompt(
    ui: &mut Ui,
    p: &Palette,
    state: &mut LabUiState,
    has_draft: bool,
    actions: &mut Vec<UiAction>,
) {
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Save draft to Library")
                .size(13.5)
                .strong()
                .color(p.fg),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let input_w = (ui.available_width() - 165.0).max(140.0);
            let resp = ui.add(
                TextEdit::singleline(&mut state.save_fav_name)
                    .desired_width(input_w)
                    .margin(Margin::symmetric(10, 7))
                    .hint_text("Give this prompt a short name…"),
            );
            let can_save = has_draft && !state.save_fav_name.trim().is_empty();
            let submitted = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (widgets::primary_btn(ui, p, "Save", can_save).clicked() || submitted) && can_save {
                actions.push(UiAction::LabSaveFavorite(
                    state.save_fav_name.trim().to_owned(),
                ));
                state.save_fav_name.clear();
                state.save_fav_open = false;
            }
            if widgets::ghost_btn(ui, p, "Cancel", true).clicked() {
                state.save_fav_open = false;
            }
        });
    });
}

fn render_lab_status_bars(
    ui: &mut Ui,
    p: &Palette,
    state: &mut LabUiState,
    busy: bool,
    actions: &mut Vec<UiAction>,
) {
    if busy {
        ui.add_space(10.0);
        Frame::default()
            .fill(p.surface2)
            .stroke(Stroke::new(1.0_f32, p.line_soft()))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(15.0).color(p.accent));
                    let msg = if state.busy_label.trim().is_empty() {
                        "Working on your draft…"
                    } else {
                        state.busy_label.as_str()
                    };
                    ui.label(RichText::new(msg).size(13.0).color(p.fg));
                });
            });
    }

    if let Some(err) = state.error.as_deref().filter(|e| !e.trim().is_empty()) {
        ui.add_space(10.0);
        Frame::default()
            .fill(blend(p.surface, p.rec, 0.12))
            .stroke(Stroke::new(1.0_f32, blend(p.surface, p.rec, 0.55)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(12, 9))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(err).size(13.0).color(p.rec));
            });
    }

    if let Some(cand) = &state.candidate {
        let mut open_compare = false;
        let mut discard_cand = false;

        ui.add_space(10.0);
        Frame::default()
            .fill(blend(p.surface, p.accent, 0.12))
            .stroke(Stroke::new(1.2_f32, blend(p.line, p.accent, 0.65)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 11))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                    let title = if cand.label.trim().is_empty() {
                        "Suggestion ready"
                    } else {
                        cand.label.as_str()
                    };
                    ui.label(RichText::new(title).size(14.0).strong().color(p.fg));
                    if !cand.model.trim().is_empty() {
                        widgets::badge(ui, p, &cand.model, None);
                    }
                    if let Some(score) = cand.assessment.as_ref().and_then(|a| a.score) {
                        widgets::badge(ui, p, &format!("Score {score}/10"), Some(p.green));
                    }
                    if !cand.note.trim().is_empty() {
                        ui.label(RichText::new(&cand.note).size(12.5).color(p.muted));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::primary_btn(ui, p, "Compare suggestion →", true).clicked() {
                            open_compare = true;
                        }
                        if widgets::ghost_btn(ui, p, "Discard", true).clicked() {
                            discard_cand = true;
                        }
                    });
                });
            });

        if open_compare {
            state.compare_open = true;
        }
        if discard_cand {
            state.compare_open = false;
            actions.push(UiAction::LabDiscardCandidate);
        }
    }
}

fn render_earlier_tools(
    ui: &mut Ui,
    p: &Palette,
    state: &mut LabUiState,
    has_draft: bool,
    busy: bool,
    actions: &mut Vec<UiAction>,
) {
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if widgets::disclosure_btn(ui, p, "Earlier draft tools", state.show_more_tools, 14.5, true, p.fg).clicked() {
            state.show_more_tools = !state.show_more_tools;
        }

        if !state.show_more_tools {
            return;
        }

        ui.add_space(10.0);
        widgets::hline(ui, p);
        ui.add_space(10.0);

        ui.label(RichText::new("Instruction").size(13.0).strong().color(p.fg));
        ui.add_space(4.0);
        ui.add(
            TextEdit::multiline(&mut state.instruction)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 8))
                .hint_text("Describe a targeted edit — e.g. \"Tighten the constraints and add an output schema\""),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let can_apply = has_draft && !state.instruction.trim().is_empty() && !busy;
            if widgets::secondary_btn(ui, p, "Apply instruction", can_apply).clicked() {
                actions.push(UiAction::LabApplyInstruction(
                    state.instruction.trim().to_owned(),
                ));
            }
            if widgets::secondary_btn(ui, p, "Get feedback", has_draft && !busy).clicked() {
                actions.push(UiAction::LabScoreDraft);
            }
        });

        if !state.archived.is_empty() {
            ui.add_space(12.0);
            widgets::hline(ui, p);
            ui.add_space(10.0);

            if !state
                .archived
                .iter()
                .any(|item| item.id == state.selected_archived)
            {
                state.selected_archived = state.archived[0].id.clone();
            }

            ui.label(
                RichText::new("Archived drafts")
                    .size(13.0)
                    .strong()
                    .color(p.fg),
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let selected_label = state
                    .archived
                    .iter()
                    .find(|item| item.id == state.selected_archived)
                    .map(|item| item.label.as_str())
                    .unwrap_or("Select archived draft");

                ComboBox::from_id_salt("lab_archived_drafts")
                    .width((ui.available_width() - 180.0).clamp(160.0, 340.0))
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for item in &state.archived {
                            ui.selectable_value(
                                &mut state.selected_archived,
                                item.id.clone(),
                                &item.label,
                            );
                        }
                    });

                let has_sel = !state.selected_archived.is_empty();
                if widgets::secondary_btn(ui, p, "Restore", has_sel && !busy).clicked() {
                    actions.push(UiAction::LabRestoreArchived(
                        state.selected_archived.clone(),
                    ));
                }
                if widgets::danger_btn(ui, p, "Delete", has_sel && !busy).clicked() {
                    actions.push(UiAction::LabDeleteArchived(
                        state.selected_archived.clone(),
                    ));
                }
            });
        }
    });
}

fn render_compare_modal(
    ctx: &egui::Context,
    p: &Palette,
    state: &mut LabUiState,
    busy: bool,
    actions: &mut Vec<UiAction>,
) {
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.compare_open = false;
        return;
    }

    let Some(cand) = state.candidate.as_ref() else {
        state.compare_open = false;
        return;
    };

    let screen = ctx.screen_rect();
    let modal_w = (screen.width() - 48.0).clamp(320.0, 880.0);
    let modal_max_h = (screen.height() - 64.0).clamp(300.0, 680.0);

    let mut close_modal = false;
    let mut show_original = state.compare_show_original;

    egui::Window::new("Compare suggestion")
        .id(egui::Id::new("lab_compare_modal"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .default_width(modal_w)
        .min_width(modal_w)
        .max_width(modal_w)
        .max_height(modal_max_h)
        .frame(
            Frame::default()
                .fill(p.surface)
                .stroke(Stroke::new(1.2_f32, p.line))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(18))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 14],
                    blur: 36,
                    spread: 0,
                    color: Color32::from_black_alpha(110),
                }),
        )
        .show(ctx, |ui| {
            // Top context strip.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                let title = if cand.label.trim().is_empty() {
                    "Suggested revision"
                } else {
                    cand.label.as_str()
                };
                ui.label(RichText::new(title).size(14.5).strong().color(p.fg));
                if !cand.model.trim().is_empty() {
                    widgets::badge(ui, p, &cand.model, None);
                }
                if let Some(score) = cand.assessment.as_ref().and_then(|a| a.score) {
                    widgets::badge(ui, p, &format!("Score {score}/10"), Some(p.green));
                }
                if !cand.note.trim().is_empty() {
                    ui.label(RichText::new(&cand.note).size(12.5).color(p.muted));
                }
            });

            ui.add_space(10.0);

            ScrollArea::vertical()
                .id_salt("lab_compare_modal_body")
                .max_height((modal_max_h - 110.0).max(180.0))
                .show(ui, |ui| {
                    let narrow = ui.available_width() < 600.0;
                    if narrow {
                        ui.horizontal(|ui| {
                            let sugg_btn = if !show_original {
                                widgets::primary_btn(ui, p, "Suggestion", true)
                            } else {
                                widgets::secondary_btn(ui, p, "Suggestion", true)
                            };
                            if sugg_btn.clicked() {
                                show_original = false;
                            }
                            let orig_btn = if show_original {
                                widgets::primary_btn(ui, p, "Original", true)
                            } else {
                                widgets::secondary_btn(ui, p, "Original", true)
                            };
                            if orig_btn.clicked() {
                                show_original = true;
                            }
                        });
                        ui.add_space(8.0);
                        if show_original {
                            render_readonly_pane(
                                ui,
                                p,
                                "Original",
                                &cand.original,
                                "compare_orig_narrow",
                                false,
                            );
                        } else {
                            render_readonly_pane(
                                ui,
                                p,
                                "Suggestion",
                                &cand.suggestion,
                                "compare_sugg_narrow",
                                true,
                            );
                        }
                    } else {
                        let col_w = ((ui.available_width() - 12.0) * 0.5).max(180.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                render_readonly_pane(
                                    ui,
                                    p,
                                    "Original",
                                    &cand.original,
                                    "compare_orig_wide",
                                    false,
                                );
                            });
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                render_readonly_pane(
                                    ui,
                                    p,
                                    "Suggestion",
                                    &cand.suggestion,
                                    "compare_sugg_wide",
                                    true,
                                );
                            });
                        });
                    }

                    if let Some(assess) = &cand.assessment {
                        ui.add_space(12.0);
                        widgets::hline(ui, p);
                        ui.add_space(10.0);
                        render_assessment_summary(ui, p, assess, true);
                    }
                });

            ui.add_space(12.0);
            widgets::hline(ui, p);
            ui.add_space(10.0);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                if widgets::secondary_btn(ui, p, "Review suggestion", !busy).clicked() {
                    actions.push(UiAction::LabReviewCandidate);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary_btn(ui, p, "Apply suggestion", true).clicked() {
                        actions.push(UiAction::LabApplyCandidate);
                        close_modal = true;
                    }
                    if widgets::danger_btn(ui, p, "Discard", true).clicked() {
                        actions.push(UiAction::LabDiscardCandidate);
                        close_modal = true;
                    }
                    if widgets::ghost_btn(ui, p, "Close", true).clicked() {
                        close_modal = true;
                    }
                });
            });
        });

    state.compare_show_original = show_original;
    if close_modal {
        state.compare_open = false;
    }
}

fn render_readonly_pane(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    content: &str,
    scroll_id: &str,
    highlight: bool,
) {
    ui.label(
        RichText::new(title)
            .size(12.5)
            .strong()
            .color(if highlight { p.accent } else { p.muted }),
    );
    ui.add_space(4.0);
    let border = if highlight {
        blend(p.line, p.accent, 0.55)
    } else {
        p.line_soft()
    };
    Frame::default()
        .fill(p.surface2)
        .stroke(Stroke::new(1.0_f32, border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical()
                .id_salt(scroll_id)
                .min_scrolled_height(150.0)
                .max_height(220.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut text_slice = content;
                    ui.add(
                        TextEdit::multiline(&mut text_slice)
                            .frame(false)
                            .desired_width(f32::INFINITY)
                            .font(FontId::proportional(13.5)),
                    );
                });
        });
}

fn render_assessment_summary(
    ui: &mut Ui,
    p: &Palette,
    assess: &LabAssessmentView,
    show_criteria: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
        if let Some(score) = assess.score {
            let tint = if score >= 8 {
                Some(p.green)
            } else if score >= 6 {
                Some(p.amber)
            } else {
                Some(p.rec)
            };
            widgets::badge(ui, p, &format!("Score {score}/10"), tint);
            if let Some(prev) = assess.prev_score {
                let delta = score as i64 - prev as i64;
                let (delta_str, delta_col) = if delta > 0 {
                    (format!("+{delta} vs previous ({prev}/10)"), p.green)
                } else if delta < 0 {
                    (format!("{delta} vs previous ({prev}/10)"), p.rec)
                } else {
                    (format!("unchanged ({prev}/10)"), p.muted)
                };
                ui.label(RichText::new(delta_str).size(12.0).color(delta_col));
            }
        }
        if !assess.by.trim().is_empty() {
            ui.label(
                RichText::new(format!("Checked by {}", assess.by))
                    .size(12.0)
                    .color(p.faint),
            );
        }
    });

    if !assess.summary.trim().is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(&assess.summary)
                .size(13.0)
                .line_height(Some(19.0))
                .color(p.fg),
        );
    }

    if show_criteria && !assess.criteria.is_empty() {
        ui.add_space(10.0);
        let cols = if ui.available_width() >= 480.0 { 2 } else { 1 };
        let col_w = if cols == 2 {
            ((ui.available_width() - 16.0) * 0.5).max(160.0)
        } else {
            ui.available_width()
        };
        for chunk in assess.criteria.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for (name, val) in chunk {
                    ui.vertical(|ui| {
                        ui.set_width(col_w);
                        render_criterion_bar(ui, p, name, *val);
                    });
                }
            });
            ui.add_space(6.0);
        }
    }

    if !assess.gaps.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new("Gaps to address")
                .size(12.5)
                .strong()
                .color(p.muted),
        );
        ui.add_space(4.0);
        for gap in &assess.gaps {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("•").size(13.0).color(p.accent));
                ui.label(
                    RichText::new(gap)
                        .size(12.5)
                        .line_height(Some(18.0))
                        .color(p.fg),
                );
            });
        }
    }
}

fn render_criterion_bar(ui: &mut Ui, p: &Palette, name: &str, value: u32) {
    let clamped = value.min(5);
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(12.5).color(p.fg));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{clamped}/5"))
                    .font(FontId::monospace(11.5))
                    .color(p.muted),
            );
        });
    });
    ui.add_space(2.0);
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 7.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), p.surface2);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, p.line_faint()),
        StrokeKind::Inside,
    );
    let frac = (clamped as f32 / 5.0).clamp(0.0, 1.0);
    if frac > 0.0 {
        let fill_col = if clamped >= 4 {
            p.green
        } else if clamped >= 3 {
            p.amber
        } else {
            p.rec
        };
        let fill_rect = Rect::from_min_size(rect.min, vec2((rect.width() * frac).max(6.0), rect.height()));
        painter.rect_filled(fill_rect, CornerRadius::same(4), fill_col);
    }
}

fn draw_status_dot(ui: &mut Ui, color: Color32, pulse: bool, time_secs: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    let alpha = if pulse {
        0.55 + 0.45 * (time_secs * 3.0).sin().abs() as f32
    } else {
        1.0
    };
    ui.painter()
        .circle_filled(rect.center(), 4.0, color.gamma_multiply(alpha));
}

fn provider_model_summary(
    provider: PromptProvider,
    local_model: &str,
    claude_model: &str,
) -> String {
    let model = provider.model(local_model, claude_model).trim();
    match provider {
        PromptProvider::Local => {
            if model.is_empty() {
                "Local".to_owned()
            } else {
                model.to_owned()
            }
        }
        _ => {
            if model.is_empty() {
                provider.label().to_owned()
            } else {
                format!("{} ({model})", provider.label())
            }
        }
    }
}

fn short_time(when: &str) -> &str {
    let trimmed = when.trim();
    trimmed.rsplit_once(' ').map_or(trimmed, |(_, time)| time)
}

fn clip_text(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (idx, ch) in normalized.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}
