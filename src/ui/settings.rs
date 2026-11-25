//! Pure-Rust native egui Settings page (Direction A filterable split-pane)
//! and interactive Hotkey Picker modal.

use crate::app_tone::TonePreset;
use crate::models::{self, PARAKEET_ID, SPEECH_CATALOG};
use crate::storage::{Destination, Hotkey, PromptProvider, PromptTarget, SpeechChoice, Style};
use crate::theme::{blend, LayoutId, Palette, ThemeId};
use crate::ui::widgets;
use crate::ui::{HotkeyModalState, SettingsSection, SettingsUiState, UiAction, UiView};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, ComboBox, CornerRadius, Event, FontId, Frame, Id,
    Key, Layout, Margin, Modifiers, Order, ProgressBar, Rect, RichText, ScrollArea, Sense, Slider,
    Stroke, StrokeKind, TextEdit, Ui,
};

const RAIL_WIDTH: f32 = 185.0;
const SPLIT_MIN_WIDTH: f32 = 680.0;
const DEFAULT_MIC_LABEL: &str = "Use Windows default microphone";

/// Render the Direction A filterable split-pane Settings screen.
pub fn render_settings(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    actions: &mut Vec<UiAction>,
) {
    let query = state.filter.trim().to_ascii_lowercase();
    let matching: Vec<SettingsSection> = SettingsSection::ALL
        .into_iter()
        .filter(|&sec| section_matches(sec, &query))
        .collect();

    if !query.is_empty() && !matching.is_empty() && !matching.contains(&state.section) {
        state.section = matching[0];
    }

    let use_split =
        ui.available_width() >= SPLIT_MIN_WIDTH && view.settings.layout == LayoutId::Desk;

    if use_split {
        ui.horizontal_top(|ui| {
            let total_w = ui.available_width();
            let rail_w = RAIL_WIDTH;
            let content_w = (total_w - rail_w - 24.0).max(260.0);

            // Left 185px navigation rail
            ui.allocate_ui_with_layout(
                vec2(rail_w, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(rail_w);
                    render_filter_box(ui, p, &mut state.filter, rail_w);
                    ui.add_space(10.0);
                    for sec in SettingsSection::ALL {
                        let is_match = query.is_empty() || matching.contains(&sec);
                        let is_active = state.section == sec && !matching.is_empty();
                        if render_rail_button(ui, p, sec, is_active, is_match, !query.is_empty()) {
                            state.section = sec;
                        }
                        ui.add_space(3.0);
                    }
                },
            );

            ui.add_space(10.0);

            // Full-height right content pane (scrolled smoothly by the outer CentralPanel ScrollArea)
            ui.allocate_ui_with_layout(
                vec2(content_w, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(content_w);
                    render_section_content(ui, p, view, state, &query, &matching, actions);
                    ui.add_space(16.0);
                },
            );
        });
    } else {
        // Narrow / Sidebar / Compact layout: filter box + wrapped section pills above content
        let avail_w = ui.available_width().max(220.0);
        render_filter_box(ui, p, &mut state.filter, avail_w);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for sec in SettingsSection::ALL {
                let is_match = query.is_empty() || matching.contains(&sec);
                let is_active = state.section == sec && !matching.is_empty();
                if render_section_pill(ui, p, sec, is_active, is_match, !query.is_empty()) {
                    state.section = sec;
                }
            }
        });
        ui.add_space(6.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        ui.set_width(avail_w);
        render_section_content(ui, p, view, state, &query, &matching, actions);
        ui.add_space(16.0);
    }
}

fn section_matches(section: SettingsSection, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    let label = section.label().to_ascii_lowercase();
    let keywords = section.keywords().to_ascii_lowercase();
    query_lower
        .split_whitespace()
        .all(|tok| label.contains(tok) || keywords.contains(tok))
}

fn render_filter_box(ui: &mut Ui, p: &Palette, filter: &mut String, width: f32) {
    let has_text = !filter.is_empty();
    Frame::default()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width((width - 18.0).max(100.0));
            ui.horizontal(|ui| {
                let (icon_r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                widgets::draw_icon(ui.painter(), icon_r, widgets::Icon::Search, p.muted);
                let input_w = (ui.available_width() - if has_text { 22.0 } else { 2.0 }).max(60.0);
                ui.add(
                    TextEdit::singleline(filter)
                        .frame(false)
                        .hint_text(RichText::new("Filter settings...").color(p.faint))
                        .desired_width(input_w),
                );
                if has_text {
                    let clear = egui::Button::new(RichText::new("×").size(14.0).color(p.muted))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .min_size(vec2(18.0, 18.0));
                    if ui
                        .add(clear)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Clear filter")
                        .clicked()
                    {
                        filter.clear();
                    }
                }
            });
        });
}

fn render_rail_button(
    ui: &mut Ui,
    p: &Palette,
    section: SettingsSection,
    active: bool,
    matches_filter: bool,
    filtering: bool,
) -> bool {
    let desired = vec2(RAIL_WIDTH, 34.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let painter = ui.painter();

    let fill = if active {
        blend(p.surface, p.accent, 0.16)
    } else if resp.hovered() {
        p.surface2
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if active {
        Stroke::new(1.0_f32, blend(p.line, p.accent, 0.6))
    } else if filtering && matches_filter {
        Stroke::new(1.0_f32, p.accent.gamma_multiply(0.45))
    } else {
        Stroke::NONE
    };

    painter.rect_filled(rect, CornerRadius::same(8), fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, CornerRadius::same(8), stroke, StrokeKind::Inside);
    }
    if active {
        let bar = Rect::from_min_size(
            pos2(rect.left() + 3.0, rect.top() + 7.0),
            vec2(3.0, rect.height() - 14.0),
        );
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    }

    let text_col = if active {
        p.fg
    } else if !matches_filter {
        p.faint.gamma_multiply(0.65)
    } else if resp.hovered() {
        p.fg
    } else {
        p.muted
    };

    painter.text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        section.label(),
        FontId::proportional(13.5),
        text_col,
    );

    if filtering && matches_filter {
        painter.circle_filled(pos2(rect.right() - 12.0, rect.center().y), 3.0, p.accent);
    }

    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn render_section_pill(
    ui: &mut Ui,
    p: &Palette,
    section: SettingsSection,
    active: bool,
    matches_filter: bool,
    filtering: bool,
) -> bool {
    let fill = if active {
        p.accent
    } else if filtering && matches_filter {
        blend(p.surface, p.accent, 0.15)
    } else {
        p.surface
    };
    let fg = if active {
        p.accent_fg
    } else if !matches_filter {
        p.faint.gamma_multiply(0.65)
    } else {
        p.fg
    };
    let stroke = if active {
        Stroke::new(1.0_f32, p.accent)
    } else if filtering && matches_filter {
        Stroke::new(1.0_f32, blend(p.line, p.accent, 0.6))
    } else {
        Stroke::new(1.0_f32, p.line)
    };
    let btn = egui::Button::new(RichText::new(section.label()).size(12.5).color(fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(99))
        .min_size(vec2(0.0, 28.0));
    ui.add(btn)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn render_section_content(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    query: &str,
    matching: &[SettingsSection],
    actions: &mut Vec<UiAction>,
) {
    if !query.is_empty() && matching.is_empty() {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.label(RichText::new("No matching settings").size(15.0).strong().color(p.fg));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Try a different search term or clear the filter.")
                        .size(13.0)
                        .color(p.muted),
                );
                ui.add_space(10.0);
                if widgets::secondary_btn(ui, p, "Clear filter", true).clicked() {
                    state.filter.clear();
                }
                ui.add_space(12.0);
            });
        });
        return;
    }

    match state.section {
        SettingsSection::General => render_general(ui, p, view, actions),
        SettingsSection::VoiceAi => render_voice_ai(ui, p, view, state, actions),
        SettingsSection::Snippets => render_snippets(ui, p, view, state, actions),
        SettingsSection::Appearance => render_appearance(ui, p, view, actions),
        SettingsSection::PromptLab => render_prompt_lab(ui, p, view, state, actions),
        SettingsSection::System => render_system(ui, p, view, state, actions),
    }
}

fn section_overline(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).size(11.5).strong().color(p.muted));
}

fn setting_row(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    subtitle: Option<&str>,
    control: impl FnOnce(&mut Ui),
) {
    let narrow = ui.available_width() < 430.0;
    if narrow {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(14.0).strong().color(p.fg));
            if let Some(sub) = subtitle {
                ui.label(RichText::new(sub).size(12.0).color(p.muted));
            }
            ui.add_space(4.0);
            control(ui);
        });
    } else {
        ui.horizontal(|ui| {
            let left_w = (ui.available_width() * 0.56).clamp(160.0, 420.0);
            ui.allocate_ui_with_layout(
                vec2(left_w, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.label(RichText::new(title).size(14.0).strong().color(p.fg));
                    if let Some(sub) = subtitle {
                        ui.label(RichText::new(sub).size(12.0).color(p.muted));
                    }
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), control);
        });
    }
}

// ---------------------------------------------------------------------------
// Section 1: General
// ---------------------------------------------------------------------------

fn render_general(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    actions: &mut Vec<UiAction>,
) {
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // Dictation hotkey
        setting_row(
            ui,
            p,
            "Dictation hotkey",
            Some("Hold to talk or tap to lock hands-free dictation."),
            |ui| {
                let label = if view.hotkey_display.is_empty() {
                    "Right Ctrl"
                } else {
                    view.hotkey_display
                };
                if widgets::secondary_btn(ui, p, label, true)
                    .on_hover_text("Click to change dictation hotkey")
                    .clicked()
                {
                    actions.push(UiAction::OpenHotkeyPicker);
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Command Mode hotkey
        setting_row(
            ui,
            p,
            "Command Mode hotkey",
            Some("Highlight text in any app, press this key, and speak an edit."),
            |ui| {
                let mut cmd_hk = view.settings.command_hotkey;
                let selected_text = cmd_hk.map_or("Off (not set)", Hotkey::label);
                let primary_legacy = view
                    .settings
                    .registered_hotkey
                    .is_none()
                    .then_some(view.settings.hotkey);
                let mut changed = false;
                ComboBox::from_id_salt("settings_command_hotkey")
                    .width(190.0)
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut cmd_hk, None, "Off (not set)").changed() {
                            changed = true;
                        }
                        for hk in Hotkey::ALL {
                            let conflicts = primary_legacy == Some(hk);
                            let text = if conflicts {
                                format!("{} (dictation key)", hk.label())
                            } else {
                                hk.label().to_owned()
                            };
                            ui.add_enabled_ui(!conflicts, |ui| {
                                if ui.selectable_value(&mut cmd_hk, Some(hk), text).changed() {
                                    changed = true;
                                }
                            });
                        }
                    });
                if changed {
                    actions.push(UiAction::SetCommandHotkey(cmd_hk));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Microphone
        setting_row(
            ui,
            p,
            "Microphone",
            Some("Input device used for dictation and meeting notes."),
            |ui| {
                let mut selected = view.settings.microphone.clone();
                let current_label = if selected.is_empty() {
                    DEFAULT_MIC_LABEL.to_owned()
                } else {
                    selected.clone()
                };
                let mut changed = false;
                ComboBox::from_id_salt("settings_microphone")
                    .width(220.0)
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut selected, String::new(), DEFAULT_MIC_LABEL)
                            .changed()
                        {
                            changed = true;
                        }
                        for mic in view.mics {
                            if mic.is_empty() {
                                continue;
                            }
                            if ui.selectable_value(&mut selected, mic.clone(), mic).changed() {
                                changed = true;
                            }
                        }
                    });
                if changed {
                    actions.push(UiAction::SetMicrophone(selected));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // After dictation
        setting_row(
            ui,
            p,
            "After dictation",
            Some("Where completed text is delivered."),
            |ui| {
                let mut dest = view.settings.destination;
                let opts = [
                    (Destination::LastApp, Destination::LastApp.label()),
                    (Destination::Workspace, Destination::Workspace.label()),
                ];
                let _sel_lbl = dest.label();
                if widgets::styled_combo(
                    ui,
                    "settings_destination",
                    &mut dest,
                    _sel_lbl,
                    220.0,
                    &opts,
                ) {
                    actions.push(UiAction::SetDestination(dest));
                }
            },
        );
    });

    ui.add_space(14.0);
    section_overline(ui, p, "BEHAVIOR");
    ui.add_space(6.0);

    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let two_col = ui.available_width() >= 440.0;

        let mut live_preview = view.settings.live_preview;
        let mut floating_card = view.settings.floating_card;
        let mut sounds = view.settings.sounds;
        let mut restore_clipboard = view.settings.restore_clipboard;
        let mut quiet_mode = view.settings.quiet_mode;

        if two_col {
            let col_w = ((ui.available_width() - 20.0) * 0.5).max(160.0);
            ui.horizontal(|ui| {
                render_toggle_cell(ui, p, col_w, "Live preview", &mut live_preview);
                ui.add_space(20.0);
                render_toggle_cell(ui, p, col_w, "Floating card", &mut floating_card);
            });
            ui.add_space(8.0);
            widgets::hline(ui, p);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                render_toggle_cell(ui, p, col_w, "Sound cues", &mut sounds);
                ui.add_space(20.0);
                render_toggle_cell(ui, p, col_w, "Restore clipboard", &mut restore_clipboard);
            });
        } else {
            let w = ui.available_width();
            render_toggle_cell(ui, p, w, "Live preview", &mut live_preview);
            ui.add_space(6.0);
            widgets::hline(ui, p);
            ui.add_space(6.0);
            render_toggle_cell(ui, p, w, "Floating card", &mut floating_card);
            ui.add_space(6.0);
            widgets::hline(ui, p);
            ui.add_space(6.0);
            render_toggle_cell(ui, p, w, "Sound cues", &mut sounds);
            ui.add_space(6.0);
            widgets::hline(ui, p);
            ui.add_space(6.0);
            render_toggle_cell(ui, p, w, "Restore clipboard", &mut restore_clipboard);
        }

        if live_preview != view.settings.live_preview {
            actions.push(UiAction::SetLivePreview(live_preview));
        }
        if floating_card != view.settings.floating_card {
            actions.push(UiAction::SetFloatingCard(floating_card));
        }
        if sounds != view.settings.sounds {
            actions.push(UiAction::SetSounds(sounds));
        }
        if restore_clipboard != view.settings.restore_clipboard {
            actions.push(UiAction::SetRestoreClipboard(restore_clipboard));
        }

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Full-width Quiet / whisper mode row
        setting_row(
            ui,
            p,
            "Quiet / whisper mode",
            Some("Boosts soft speech and lowers the silence threshold."),
            |ui| {
                if widgets::pill_toggle(ui, p, &mut quiet_mode).changed() {
                    actions.push(UiAction::SetQuietMode(quiet_mode));
                }
            },
        );
    });
}

fn render_toggle_cell(ui: &mut Ui, p: &Palette, width: f32, label: &str, val: &mut bool) {
    ui.allocate_ui_with_layout(vec2(width, 30.0), Layout::left_to_right(Align::Center), |ui| {
        ui.label(RichText::new(label).size(13.5).color(p.fg));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::pill_toggle(ui, p, val);
        });
    });
}

// ---------------------------------------------------------------------------
// Section 2: Voice & AI
// ---------------------------------------------------------------------------

fn render_voice_ai(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    actions: &mut Vec<UiAction>,
) {
    // Card 1: Speech recognition + Speech Models catalog
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Speech recognition").size(15.0).strong().color(p.fg));
        ui.add_space(8.0);

        setting_row(
            ui,
            p,
            "Speech engine",
            Some("Choose how Listen runs speech-to-text locally."),
            |ui| {
                let mut choice = view.settings.speech_engine;
                let opts = [
                    (SpeechChoice::Auto, SpeechChoice::Auto.label()),
                    (SpeechChoice::Transcribe, SpeechChoice::Transcribe.label()),
                    (SpeechChoice::WhisperCli, SpeechChoice::WhisperCli.label()),
                ];
                let _sel_lbl = choice.label();
                if widgets::styled_combo(
                    ui,
                    "settings_speech_engine",
                    &mut choice,
                    _sel_lbl,
                    220.0,
                    &opts,
                ) {
                    actions.push(UiAction::SetSpeechEngine(choice));
                }
            },
        );

        ui.add_space(10.0);
        widgets::hline(ui, p);
        ui.add_space(10.0);

        section_overline(ui, p, "SPEECH MODELS");
        ui.add_space(6.0);

        let ms = view.model_status;
        if ms.install_active {
            Frame::default()
                .fill(blend(p.surface, p.accent, 0.08))
                .stroke(Stroke::new(1.0_f32, blend(p.line, p.accent, 0.55)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let target_name = models::find_speech_entry(&ms.install_target_id)
                        .map_or(ms.install_target_id.as_str(), |e| e.name);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Installing {target_name}"))
                                .size(13.0)
                                .strong()
                                .color(p.fg),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::danger_btn(ui, p, "Cancel", true).clicked() {
                                actions.push(UiAction::CancelModelInstall);
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.add(
                        ProgressBar::new(ms.install_progress.clamp(0.0, 1.0))
                            .show_percentage()
                            .animate(true),
                    );
                    if !ms.install_stage.is_empty() {
                        ui.add_space(3.0);
                        ui.label(RichText::new(&ms.install_stage).size(11.5).color(p.muted));
                    }
                });
            ui.add_space(8.0);
        }

        if let Some(err) = ms.install_error.as_deref().filter(|s| !s.is_empty()) {
            ui.label(RichText::new(err).size(12.5).color(p.rec));
            ui.add_space(6.0);
        }

        for (idx, entry) in SPEECH_CATALOG.iter().enumerate() {
            if idx > 0 {
                ui.add_space(6.0);
                widgets::hline(ui, p);
                ui.add_space(6.0);
            }
            let installed = if entry.id == PARAKEET_ID {
                ms.parakeet_installed
            } else {
                ms.installed_whisper_ids.contains(entry.id)
            };
            let active = ms.active_speech_id == entry.id;
            let installing_this = ms.install_active && ms.install_target_id == entry.id;

            ui.horizontal(|ui| {
                let info_w = (ui.available_width() - 150.0).max(160.0);
                ui.allocate_ui_with_layout(
                    vec2(info_w, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(entry.name).size(13.5).strong().color(p.fg));
                            widgets::badge(ui, p, entry.tag, None);
                        });
                        ui.label(
                            RichText::new(format!("{} MB · {}", entry.size_mb, entry.ram_note))
                                .size(11.5)
                                .color(p.faint),
                        );
                        ui.label(RichText::new(entry.note).size(12.0).color(p.muted));
                    },
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if active {
                        widgets::badge(ui, p, "Active", Some(p.green));
                    } else if installing_this {
                        widgets::badge(ui, p, "Installing…", Some(p.amber));
                    } else if installed {
                        if widgets::secondary_btn(ui, p, "Use", !ms.install_active).clicked() {
                            actions.push(UiAction::SelectSpeechModel(entry.id.to_owned()));
                        }
                        widgets::badge(ui, p, "Installed", Some(p.accent));
                    } else if widgets::secondary_btn(ui, p, "Install", !ms.install_active).clicked()
                    {
                        actions.push(UiAction::InstallSpeechModel(entry.id.to_owned()));
                    }
                });
            });
        }
    });

    ui.add_space(12.0);

    // Card 2: AI formatting & Model Benchmarker
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("AI formatting").size(15.0).strong().color(p.fg));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if view.ollama_available {
                    widgets::badge(ui, p, "Ollama ready", Some(p.green));
                } else {
                    widgets::badge(ui, p, "Ollama offline", Some(p.amber));
                }
            });
        });
        ui.add_space(8.0);

        let mut cleanup = view.settings.cleanup;
        setting_row(
            ui,
            p,
            "AI formatting",
            Some("Clean up punctuation, fillers, and phrasing locally with Ollama."),
            |ui| {
                if widgets::pill_toggle(ui, p, &mut cleanup).changed() {
                    actions.push(UiAction::SetCleanup(cleanup));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        setting_row(
            ui,
            p,
            "Formatting style",
            Some("How much Listen tightens phrasing when cleaning up speech."),
            |ui| {
                let mut style = view.settings.style;
                let opts = [
                    (Style::Tighten, Style::Tighten.label()),
                    (Style::Clean, Style::Clean.label()),
                ];
                let _sel_lbl = style.label();
                if widgets::styled_combo(
                    ui,
                    "settings_formatting_style",
                    &mut style,
                    _sel_lbl,
                    190.0,
                    &opts,
                ) {
                    actions.push(UiAction::SetStyle(style));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        setting_row(
            ui,
            p,
            "Local Ollama model",
            Some("Model used for dictation cleanup and command mode."),
            |ui| {
                if widgets::secondary_btn(ui, p, "Refresh", true).clicked() {
                    actions.push(UiAction::RefreshModels);
                }
                let mut model = view.settings.local_model.clone();
                let selected_label = if model.is_empty() {
                    "Select model…".to_owned()
                } else {
                    model.clone()
                };
                let mut changed = false;
                ComboBox::from_id_salt("settings_local_model")
                    .width(190.0)
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        if view.models.is_empty() {
                            ui.label(
                                RichText::new("No local Ollama models detected")
                                    .size(12.0)
                                    .color(p.muted),
                            );
                        } else {
                            for m in view.models {
                                if ui.selectable_value(&mut model, m.clone(), m).changed() {
                                    changed = true;
                                }
                            }
                        }
                    });
                if changed {
                    actions.push(UiAction::SetLocalModel(model));
                }
            },
        );

        ui.add_space(10.0);
        widgets::hline(ui, p);
        ui.add_space(10.0);

        // Model Benchmarker
        let ms = view.model_status;
        setting_row(
            ui,
            p,
            "Model benchmarker",
            Some("Measure cleanup accuracy and latency across installed Ollama models."),
            |ui| {
                if ms.benchmark_active {
                    if widgets::danger_btn(ui, p, "Cancel", true).clicked() {
                        actions.push(UiAction::CancelBenchmark);
                    }
                } else {
                    let can_run = view.ollama_available && !view.models.is_empty();
                    if widgets::secondary_btn(ui, p, "Run benchmark", can_run).clicked() {
                        actions.push(UiAction::StartBenchmark);
                    }
                }
            },
        );

        if ms.benchmark_active {
            ui.add_space(6.0);
            ui.add(
                ProgressBar::new(ms.benchmark_progress.clamp(0.0, 1.0))
                    .show_percentage()
                    .animate(true),
            );
            if !ms.benchmark_stage.is_empty() {
                ui.add_space(3.0);
                ui.label(RichText::new(&ms.benchmark_stage).size(11.5).color(p.muted));
            }
        }

        if let Some(err) = ms.benchmark_error.as_deref().filter(|s| !s.is_empty()) {
            ui.add_space(4.0);
            ui.label(RichText::new(err).size(12.5).color(p.rec));
        }

        if !ms.saved_benchmarks.results.is_empty() {
            ui.add_space(8.0);
            Frame::default()
                .fill(p.bg)
                .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.6)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Grid::new("settings_benchmark_table")
                        .num_columns(3)
                        .spacing(vec2(16.0, 6.0))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Model").size(12.0).strong().color(p.muted));
                            ui.label(
                                RichText::new("Pass score").size(12.0).strong().color(p.muted),
                            );
                            ui.label(
                                RichText::new("Median latency")
                                    .size(12.0)
                                    .strong()
                                    .color(p.muted),
                            );
                            ui.end_row();

                            for rec in ms.saved_benchmarks.results.values() {
                                ui.label(RichText::new(&rec.model).size(12.5).color(p.fg));
                                let ratio = if rec.cases > 0 {
                                    rec.passed as f32 / rec.cases as f32
                                } else {
                                    0.0
                                };
                                let score_col = if ratio >= 0.85 {
                                    p.green
                                } else if ratio >= 0.6 {
                                    p.amber
                                } else {
                                    p.rec
                                };
                                ui.label(
                                    RichText::new(format!("{}/{}", rec.passed, rec.cases))
                                        .size(12.5)
                                        .strong()
                                        .color(score_col),
                                );
                                ui.label(
                                    RichText::new(format!("{:.2}s", rec.median_seconds))
                                        .size(12.5)
                                        .color(p.muted),
                                );
                                ui.end_row();
                            }
                        });
                });
        }
    });

    ui.add_space(12.0);

    // Card 3: App-Aware Tone & Style
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("App-Aware Tone & Style")
                .size(15.0)
                .strong()
                .color(p.fg),
        );
        ui.add_space(6.0);

        let mut auto_tone = view.settings.auto_app_tone;
        setting_row(
            ui,
            p,
            "Auto-detect tone by app",
            Some("Casual for chat · Polished for email/docs · Technical for code"),
            |ui| {
                if widgets::pill_toggle(ui, p, &mut auto_tone).changed() {
                    actions.push(UiAction::SetAutoAppTone(auto_tone));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        section_overline(ui, p, "CUSTOM APP RULES");
        ui.add_space(6.0);

        let mut submit_rule = false;
        ui.horizontal_wrapped(|ui| {
            let input_w = (ui.available_width() - 230.0).clamp(140.0, 280.0);
            let resp = ui.add(
                TextEdit::singleline(&mut state.tone_app_input)
                    .hint_text("App name (e.g. Slack, VS Code)")
                    .desired_width(input_w),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                submit_rule = true;
            }

            let tone_opts: Vec<(TonePreset, &str)> =
                TonePreset::ALL.iter().map(|&t| (t, t.label())).collect();
            let _sel_lbl = state.tone_preset_input.label();
            widgets::styled_combo(
                ui,
                "settings_tone_preset_add",
                &mut state.tone_preset_input,
                _sel_lbl,
                115.0,
                &tone_opts,
            );

            if widgets::primary_btn(ui, p, "Add rule", true).clicked() {
                submit_rule = true;
            }
        });

        if submit_rule {
            actions.push(UiAction::AddAppToneRule {
                app: state.tone_app_input.clone(),
                tone: state.tone_preset_input,
            });
        }

        if let Some(err) = state.tone_error.as_deref().filter(|s| !s.is_empty()) {
            ui.add_space(4.0);
            ui.label(RichText::new(err).size(12.5).color(p.rec));
        }

        ui.add_space(8.0);
        if view.settings.app_tone_rules.is_empty() {
            ui.label(
                RichText::new("No custom rules yet. Built-in app matching applies when enabled.")
                    .size(12.5)
                    .color(p.muted),
            );
        } else {
            for (idx, rule) in view.settings.app_tone_rules.iter().enumerate() {
                if idx > 0 {
                    widgets::hline(ui, p);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&rule.app).size(13.5).strong().color(p.fg));
                    widgets::badge(ui, p, rule.tone.label(), Some(p.accent));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::ghost_btn(ui, p, "Remove", true).clicked() {
                            actions.push(UiAction::DeleteAppToneRule(idx));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Section 3: Snippets
// ---------------------------------------------------------------------------

fn render_snippets(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    actions: &mut Vec<UiAction>,
) {
    // Intro note
    Frame::default()
        .fill(blend(p.surface, p.accent, 0.07))
        .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.75)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(
                    "Spoken triggers expand automatically with whole-word safety — so short phrases never accidentally expand inside longer words.",
                )
                .size(13.0)
                .color(p.muted),
            );
        });

    ui.add_space(12.0);

    // Add snippet card
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Add voice snippet").size(15.0).strong().color(p.fg));
        ui.add_space(8.0);

        ui.label(RichText::new("Spoken trigger").size(12.5).color(p.muted));
        ui.add_space(3.0);
        ui.add(
            TextEdit::singleline(&mut state.snippet_trigger)
                .hint_text("e.g. my email")
                .desired_width(ui.available_width()),
        );

        ui.add_space(8.0);
        ui.label(RichText::new("Expansion text").size(12.5).color(p.muted));
        ui.add_space(3.0);
        ui.add(
            TextEdit::multiline(&mut state.snippet_expansion)
                .desired_rows(3)
                .hint_text("e.g. alex@example.com")
                .desired_width(ui.available_width()),
        );

        if let Some(err) = state.snippet_error.as_deref().filter(|s| !s.is_empty()) {
            ui.add_space(6.0);
            ui.label(RichText::new(err).size(12.5).color(p.rec));
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::primary_btn(ui, p, "Add snippet", true).clicked() {
                actions.push(UiAction::AddSnippet {
                    trigger: state.snippet_trigger.clone(),
                    expansion: state.snippet_expansion.clone(),
                });
            }
        });
    });

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        section_overline(ui, p, "SAVED SNIPPETS");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let n = view.settings.snippets.len();
            let suffix = if n == 1 { "snippet" } else { "snippets" };
            ui.label(RichText::new(format!("{n} {suffix}")).size(12.0).color(p.muted));
        });
    });
    ui.add_space(6.0);

    if view.settings.snippets.is_empty() {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(
                    "No snippets saved yet. Add a trigger phrase above for email addresses, links, or standard sign-offs.",
                )
                .size(13.0)
                .color(p.muted),
            );
        });
    } else {
        widgets::card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (idx, snippet) in view.settings.snippets.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(6.0);
                    widgets::hline(ui, p);
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    let preview_w = (ui.available_width() - 80.0).max(140.0);
                    ui.allocate_ui_with_layout(
                        vec2(preview_w, 0.0),
                        Layout::left_to_right(Align::Center).with_main_wrap(true),
                        |ui| {
                            ui.label(
                                RichText::new(&snippet.trigger).size(13.5).strong().color(p.fg),
                            );
                            ui.label(RichText::new("→").size(13.0).color(p.faint));
                            let preview = truncate_preview(&snippet.expansion, 140);
                            ui.label(RichText::new(preview).size(13.0).color(p.muted));
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::ghost_btn(ui, p, "Delete", true).clicked() {
                            actions.push(UiAction::DeleteSnippet(idx));
                        }
                    });
                });
            }
        });
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let single_line: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ↵ ");
    let count = single_line.chars().count();
    if count <= max_chars {
        single_line
    } else {
        let mut out: String = single_line.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

// ---------------------------------------------------------------------------
// Section 4: Appearance
// ---------------------------------------------------------------------------

fn render_appearance(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    actions: &mut Vec<UiAction>,
) {
    // Color scheme card
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let themes = ThemeId::ALL;
        let total = themes.len();
        let cur_idx = themes
            .iter()
            .position(|&t| t == view.settings.theme)
            .unwrap_or(0);

        // Header row with arrow cycler
        ui.horizontal(|ui| {
            ui.label(RichText::new("Color scheme").size(15.0).strong().color(p.fg));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let next_btn = egui::Button::new(RichText::new("›").size(15.0).color(p.fg))
                    .fill(p.surface2)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(CornerRadius::same(6))
                    .min_size(vec2(26.0, 24.0));
                if ui
                    .add(next_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Next color scheme")
                    .clicked()
                {
                    actions.push(UiAction::SetTheme(themes[(cur_idx + 1) % total]));
                }

                ui.label(
                    RichText::new(format!(
                        "{} · {}/{}",
                        view.settings.theme.label(),
                        cur_idx + 1,
                        total
                    ))
                    .size(12.5)
                    .color(p.muted),
                );

                let prev_btn = egui::Button::new(RichText::new("‹").size(15.0).color(p.fg))
                    .fill(p.surface2)
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .corner_radius(CornerRadius::same(6))
                    .min_size(vec2(26.0, 24.0));
                if ui
                    .add(prev_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Previous color scheme")
                    .clicked()
                {
                    actions.push(UiAction::SetTheme(themes[(cur_idx + total - 1) % total]));
                }
            });
        });

        ui.add_space(10.0);

        // Compact 6x2 grid (3x4 on narrow < 560px)
        let avail = ui.available_width();
        let cols: usize = if avail >= 560.0 { 6 } else { 3 };
        let gap = 8.0;
        let tile_w = ((avail - gap * (cols.saturating_sub(1) as f32)) / cols as f32)
            .floor()
            .max(76.0);

        for row in themes.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &theme in row {
                    let selected = theme == view.settings.theme;
                    if widgets::theme_tile_3band(ui, p, theme, selected, tile_w).clicked() {
                        actions.push(UiAction::SetTheme(theme));
                    }
                }
            });
            ui.add_space(gap);
        }
    });

    ui.add_space(12.0);

    // Layout card
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Layout").size(15.0).strong().color(p.fg));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(view.settings.layout.blurb())
                        .size(12.5)
                        .color(p.muted),
                );
            });
        });
        ui.add_space(10.0);

        let avail = ui.available_width();
        if avail >= 480.0 {
            let gap = 10.0;
            let tile_w = ((avail - gap * 2.0) / 3.0).floor().max(130.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for layout in LayoutId::ALL {
                    let selected = layout == view.settings.layout;
                    if widgets::layout_tile(ui, p, layout, selected, tile_w).clicked() {
                        actions.push(UiAction::SetLayout(layout));
                    }
                }
            });
        } else {
            for layout in LayoutId::ALL {
                let selected = layout == view.settings.layout;
                if widgets::layout_tile(ui, p, layout, selected, avail).clicked() {
                    actions.push(UiAction::SetLayout(layout));
                }
                ui.add_space(8.0);
            }
        }
    });

    ui.add_space(12.0);

    // Text size / Zoom compact row
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        setting_row(
            ui,
            p,
            "Text size / Zoom",
            Some("Scale interface text and controls across Listen."),
            |ui| {
                let can_reset = (view.settings.zoom - 1.0).abs() > 0.005;
                if widgets::secondary_btn(ui, p, "Reset (100%)", can_reset).clicked() {
                    actions.push(UiAction::SetZoom(1.0));
                }
                ui.add_space(6.0);
                let mut zoom = view.settings.zoom.clamp(0.85, 1.35);
                let slider = Slider::new(&mut zoom, 0.85..=1.35)
                    .step_by(0.05)
                    .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as i32));
                if ui.add(slider).changed() {
                    actions.push(UiAction::SetZoom(zoom));
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Section 5: Prompt Lab
// ---------------------------------------------------------------------------

fn render_prompt_lab(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    actions: &mut Vec<UiAction>,
) {
    // Card 1: Target & Providers
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Target & AI providers")
                .size(15.0)
                .strong()
                .color(p.fg),
        );
        ui.add_space(8.0);

        setting_row(
            ui,
            p,
            "Default prompt target",
            Some("Tailor engineered prompts for coding agents or general AI."),
            |ui| {
                let mut target = view.settings.prompt_target;
                let opts = [
                    (PromptTarget::Auto, PromptTarget::Auto.label()),
                    (PromptTarget::ClaudeCode, PromptTarget::ClaudeCode.label()),
                    (PromptTarget::General, PromptTarget::General.label()),
                ];
                let _sel_lbl = target.label();
                if widgets::styled_combo(
                    ui,
                    "settings_prompt_target",
                    &mut target,
                    _sel_lbl,
                    210.0,
                    &opts,
                ) {
                    actions.push(UiAction::SetPromptTarget(target));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        let provider_opts: Vec<(PromptProvider, &str)> = PromptProvider::ALL
            .iter()
            .map(|&prov| (prov, prov.label()))
            .collect();

        // Writing AI
        setting_row(
            ui,
            p,
            "Writing AI",
            Some("Provider that drafts and rewrites prompts in Prompt Lab."),
            |ui| {
                let mut prov = view.settings.prompt_engine;
                let _sel_lbl = prov.label();
                if widgets::styled_combo(
                    ui,
                    "settings_prompt_engine",
                    &mut prov,
                    _sel_lbl,
                    190.0,
                    &provider_opts,
                ) {
                    actions.push(UiAction::SetPromptEngine(prov));
                }
            },
        );

        if view.settings.prompt_engine == PromptProvider::Local {
            ui.add_space(6.0);
            render_provider_local_model_row(
                ui,
                p,
                "settings_engine_local_model",
                "Local model for writing",
                &view.settings.engine_local_model,
                view.models,
                |m| actions.push(UiAction::SetEngineLocalModel(m)),
            );
        }

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Checking AI
        setting_row(
            ui,
            p,
            "Checking AI",
            Some("Provider that scores drafts and highlights gaps."),
            |ui| {
                let mut prov = view.settings.prompt_judge;
                let _sel_lbl = prov.label();
                if widgets::styled_combo(
                    ui,
                    "settings_prompt_judge",
                    &mut prov,
                    _sel_lbl,
                    190.0,
                    &provider_opts,
                ) {
                    actions.push(UiAction::SetPromptJudge(prov));
                }
            },
        );

        if view.settings.prompt_judge == PromptProvider::Local {
            ui.add_space(6.0);
            render_provider_local_model_row(
                ui,
                p,
                "settings_judge_local_model",
                "Local model for checking",
                &view.settings.judge_local_model,
                view.models,
                |m| actions.push(UiAction::SetJudgeLocalModel(m)),
            );
        }
    });

    ui.add_space(12.0);

    // Card 2: Paid cloud APIs
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Paid cloud APIs").size(15.0).strong().color(p.fg));
        ui.add_space(8.0);

        let mut cloud = view.settings.cloud_enabled;
        setting_row(
            ui,
            p,
            "Allow paid cloud APIs",
            Some("Enable Anthropic, OpenAI, or Gemini when selected above."),
            |ui| {
                if widgets::pill_toggle(ui, p, &mut cloud).changed() {
                    actions.push(UiAction::SetCloudEnabled(cloud));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        setting_row(
            ui,
            p,
            "Claude model",
            Some("Model identifier used when Claude is selected."),
            |ui| {
                let mut claude = view.settings.claude_model.clone();
                if ui
                    .add(
                        TextEdit::singleline(&mut claude)
                            .hint_text("claude-haiku-5-5")
                            .desired_width(210.0),
                    )
                    .changed()
                {
                    actions.push(UiAction::SetClaudeModel(claude));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        section_overline(ui, p, "ENVIRONMENT API KEYS");
        ui.add_space(6.0);
        let (has_anthropic, has_openai, has_gemini) = view.api_keys_present;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            render_env_key_badge(ui, p, "ANTHROPIC_API_KEY", has_anthropic);
            render_env_key_badge(ui, p, "OPENAI_API_KEY", has_openai);
            render_env_key_badge(ui, p, "GEMINI_API_KEY", has_gemini);
        });
    });

    ui.add_space(12.0);

    // Card 3: Learned preferences
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Learned preferences")
                .size(15.0)
                .strong()
                .color(p.fg),
        );
        ui.add_space(8.0);

        let mut learn = view.settings.learn;
        setting_row(
            ui,
            p,
            "Use what Listen learns",
            Some("Remember recurring phrasing and prompt preferences locally."),
            |ui| {
                if widgets::pill_toggle(ui, p, &mut learn).changed() {
                    actions.push(UiAction::SetLearn(learn));
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        if state.learned_lines.is_empty() && state.learned_snippet.trim().is_empty() {
            ui.label(
                RichText::new("No learned preferences recorded yet.")
                    .size(12.5)
                    .color(p.muted),
            );
        } else {
            for line in &state.learned_lines {
                ui.label(RichText::new(format!("• {line}")).size(12.5).color(p.fg));
            }
            if !state.learned_snippet.trim().is_empty() {
                ui.add_space(6.0);
                Frame::default()
                    .fill(p.bg)
                    .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.5)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(&state.learned_snippet)
                                .size(12.0)
                                .monospace()
                                .color(p.muted),
                        );
                    });
            }
        }

        ui.add_space(10.0);
        if widgets::danger_btn(ui, p, "Forget learned preferences", true).clicked() {
            actions.push(UiAction::ResetLearned);
        }
    });
}

fn render_provider_local_model_row(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    label: &str,
    current: &str,
    models: &[String],
    mut on_change: impl FnMut(String),
) {
    setting_row(ui, p, label, None, |ui| {
        let mut selected = current.to_owned();
        let display = if selected.is_empty() {
            "Use default Ollama model".to_owned()
        } else {
            selected.clone()
        };
        let mut changed = false;
        ComboBox::from_id_salt(id)
            .width(190.0)
            .selected_text(display)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_value(&mut selected, String::new(), "Use default Ollama model")
                    .changed()
                {
                    changed = true;
                }
                for m in models {
                    if ui.selectable_value(&mut selected, m.clone(), m).changed() {
                        changed = true;
                    }
                }
            });
        if changed {
            on_change(selected);
        }
    });
}

fn render_env_key_badge(ui: &mut Ui, p: &Palette, name: &str, present: bool) {
    let text = if present {
        format!("{name} · Set")
    } else {
        format!("{name} · Not set")
    };
    let tint = if present { Some(p.green) } else { None };
    widgets::badge(ui, p, &text, tint);
}

// ---------------------------------------------------------------------------
// Section 6: System
// ---------------------------------------------------------------------------

fn render_system(
    ui: &mut Ui,
    p: &Palette,
    view: &UiView<'_>,
    state: &mut SettingsUiState,
    actions: &mut Vec<UiAction>,
) {
    // Card 1: Keyboard shortcuts
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Keyboard shortcuts").size(15.0).strong().color(p.fg));
        ui.add_space(8.0);

        let dictation_key = if view.hotkey_display.is_empty() {
            "Right Ctrl"
        } else {
            view.hotkey_display
        };
        let cmd_key = view
            .settings
            .command_hotkey
            .map_or("Off", Hotkey::label);

        let rows = [
            (dictation_key, "Start or finish dictation"),
            (cmd_key, "Command Mode (edit highlighted text)"),
            ("Esc", "Cancel active recording"),
            ("Ctrl+1 .. Ctrl+5", "Switch between pages"),
            ("Ctrl+K", "Open command palette"),
            ("Ctrl+Enter", "Paste active Prompt Lab draft"),
        ];

        for (idx, (key_lbl, desc)) in rows.iter().enumerate() {
            if idx > 0 {
                ui.add_space(5.0);
                widgets::hline(ui, p);
                ui.add_space(5.0);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(*desc).size(13.0).color(p.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    Frame::default()
                        .fill(p.bg)
                        .stroke(Stroke::new(1.0_f32, p.line))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(*key_lbl)
                                    .size(12.0)
                                    .monospace()
                                    .strong()
                                    .color(p.fg),
                            );
                        });
                });
            });
        }
    });

    ui.add_space(12.0);

    // Card 2: Saved files & maintenance
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("Saved files & maintenance")
                .size(15.0)
                .strong()
                .color(p.fg),
        );
        ui.add_space(8.0);

        // Clear drafts & recovery (2-step confirm)
        setting_row(
            ui,
            p,
            "Clear drafts & recovery",
            Some("Removes saved Prompt Lab drafts, archived drafts, and recovery snapshots."),
            |ui| {
                if state.confirm_clear_drafts {
                    if widgets::ghost_btn(ui, p, "Cancel", true).clicked() {
                        state.confirm_clear_drafts = false;
                    }
                    if widgets::danger_btn(ui, p, "Confirm clear", true).clicked() {
                        state.confirm_clear_drafts = false;
                        actions.push(UiAction::ClearDrafts);
                    }
                } else if widgets::secondary_btn(ui, p, "Clear drafts & recovery", true).clicked() {
                    state.confirm_clear_drafts = true;
                }
            },
        );

        ui.add_space(8.0);
        widgets::hline(ui, p);
        ui.add_space(8.0);

        // Clear app logs (2-step confirm)
        setting_row(
            ui,
            p,
            "Clear app logs",
            Some("Clears local diagnostic log files written by Listen."),
            |ui| {
                if state.confirm_clear_logs {
                    if widgets::ghost_btn(ui, p, "Cancel", true).clicked() {
                        state.confirm_clear_logs = false;
                    }
                    if widgets::danger_btn(ui, p, "Confirm clear logs", true).clicked() {
                        state.confirm_clear_logs = false;
                        actions.push(UiAction::ClearLogs);
                    }
                } else if widgets::secondary_btn(ui, p, "Clear app logs", true).clicked() {
                    state.confirm_clear_logs = true;
                }
            },
        );
    });

    ui.add_space(12.0);

    // Card 3: Diagnostics
    widgets::card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Diagnostics").size(15.0).strong().color(p.fg));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let has_diag = !state.diagnostics_summary.trim().is_empty();
                if widgets::ghost_btn(ui, p, "Copy", has_diag).clicked() {
                    actions.push(UiAction::CopyText(
                        state.diagnostics_summary.clone(),
                        "Copied diagnostics",
                    ));
                }
            });
        });
        ui.add_space(6.0);

        let text = if state.diagnostics_summary.trim().is_empty() {
            "No diagnostics available yet."
        } else {
            state.diagnostics_summary.as_str()
        };
        Frame::default()
            .fill(p.bg)
            .stroke(Stroke::new(1.0_f32, p.line.gamma_multiply(0.55)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(text).size(12.0).monospace().color(p.muted));
            });
    });
}

// ---------------------------------------------------------------------------
// Interactive Hotkey Picker Modal
// ---------------------------------------------------------------------------

/// Render the centered modal for selecting or capturing a dictation hotkey.
pub fn render_hotkey_modal(
    ctx: &egui::Context,
    p: &Palette,
    state: &mut HotkeyModalState,
    actions: &mut Vec<UiAction>,
) {
    if !state.open {
        return;
    }

    // Live keyboard combo capture while listening
    let mut esc_pressed = false;
    let mut captured_combo: Option<String> = None;
    ctx.input(|i| {
        for ev in &i.events {
            if let Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = *ev
            {
                if key == Key::Escape && modifiers.is_none() {
                    esc_pressed = true;
                    break;
                }
                if state.listening && !state.controller_mode {
                    if let Some(combo) = egui_key_to_hotkey_value(key, modifiers) {
                        captured_combo = Some(combo);
                        break;
                    }
                }
            }
        }
    });

    if esc_pressed {
        actions.push(UiAction::HotkeyModalCancel);
        return;
    }
    if let Some(combo) = captured_combo {
        actions.push(UiAction::HotkeyModalCaptureKey(combo));
    }

    // Translucent backdrop scrim
    let screen = ctx.screen_rect();
    egui::Area::new(Id::new("hotkey_modal_scrim"))
        .order(Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(135));
            if resp.clicked() {
                actions.push(UiAction::HotkeyModalCancel);
            }
        });

    let modal_w = (screen.width() - 48.0).clamp(320.0, 440.0);
    egui::Window::new("Change dictation hotkey")
        .id(Id::new("hotkey_picker_modal"))
        .order(Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .frame(
            Frame::default()
                .fill(p.surface)
                .stroke(Stroke::new(1.0_f32, p.line))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(18, 16)),
        )
        .show(ctx, |ui| {
            ui.set_width(modal_w);

            ui.label(
                RichText::new(
                    "Press a keyboard shortcut, pick a preset key, or bind a game controller button.",
                )
                .size(13.0)
                .color(p.muted),
            );
            ui.add_space(10.0);

            // Live capture preview card
            let highlight = state.listening || state.controller_mode;
            let box_border = if highlight { p.accent } else { p.line };
            let box_fill = if highlight {
                blend(p.bg, p.accent, 0.08)
            } else {
                p.bg
            };
            Frame::default()
                .fill(box_fill)
                .stroke(Stroke::new(1.4_f32, box_border))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(14, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        let preview_text = if state.controller_mode {
                            if state.captured_label.is_empty() {
                                "Press any button on your controller…".to_owned()
                            } else {
                                state.captured_label.clone()
                            }
                        } else if !state.captured_label.is_empty() {
                            state.captured_label.clone()
                        } else if state.listening {
                            "Press a key combination now…".to_owned()
                        } else {
                            "No hotkey selected".to_owned()
                        };
                        ui.label(
                            RichText::new(preview_text)
                                .size(17.0)
                                .strong()
                                .color(p.fg),
                        );
                        ui.add_space(4.0);
                        let sub = if state.controller_mode {
                            "Listening for XInput controller button"
                        } else if state.listening {
                            "Listening for Ctrl / Alt / Shift + key, or F1–F24"
                        } else {
                            "Click Listen to record a custom shortcut"
                        };
                        ui.label(RichText::new(sub).size(11.5).color(p.muted));
                    });
                });

            // Conflict / assessment message
            if !state.warning_message.is_empty() {
                ui.add_space(8.0);
                let warn_col = match state.warning_level.as_str() {
                    "blocked" => p.rec,
                    "warning" => p.amber,
                    "clear" => p.green,
                    _ => p.muted,
                };
                Frame::default()
                    .fill(blend(p.surface, warn_col, 0.14))
                    .stroke(Stroke::new(1.0_f32, warn_col.gamma_multiply(0.6)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(&state.warning_message).size(12.5).color(warn_col));
                    });
            }

            ui.add_space(12.0);
            section_overline(ui, p, "QUICK PRESETS");
            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for hk in Hotkey::ALL {
                    let id = hotkey_preset_id(hk);
                    let selected = state.captured_value == id;
                    let fill = if selected {
                        blend(p.surface, p.accent, 0.2)
                    } else {
                        p.surface2
                    };
                    let stroke = if selected {
                        Stroke::new(1.2_f32, p.accent)
                    } else {
                        Stroke::new(1.0_f32, p.line)
                    };
                    let btn = egui::Button::new(RichText::new(hk.label()).size(12.5).color(p.fg))
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(7))
                        .min_size(vec2(0.0, 28.0));
                    if ui
                        .add(btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        state.controller_mode = false;
                        actions.push(UiAction::HotkeyModalCaptureKey(id.to_owned()));
                    }
                }
            });

            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                let listen_label = if state.listening && !state.controller_mode {
                    "Listening for keys…"
                } else {
                    "Capture keyboard combo"
                };
                if widgets::secondary_btn(ui, p, listen_label, true).clicked() {
                    state.controller_mode = false;
                    state.listening = true;
                }
                let ctrl_label = if state.controller_mode {
                    "Listening for controller…"
                } else {
                    "Use game controller"
                };
                if widgets::secondary_btn(ui, p, ctrl_label, !state.controller_mode).clicked() {
                    actions.push(UiAction::HotkeyModalBeginController);
                }
            });

            ui.add_space(12.0);
            widgets::hline(ui, p);
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_apply =
                        !state.captured_value.is_empty() && state.warning_level != "blocked";
                    if widgets::primary_btn(ui, p, "Use hotkey", can_apply).clicked() {
                        actions.push(UiAction::HotkeyModalApply(state.captured_value.clone()));
                    }
                    ui.add_space(4.0);
                    if widgets::ghost_btn(ui, p, "Cancel", true).clicked() {
                        actions.push(UiAction::HotkeyModalCancel);
                    }
                });
            });
        });
}

fn hotkey_preset_id(hotkey: Hotkey) -> &'static str {
    match hotkey {
        Hotkey::RightCtrl => "ctrl_r",
        Hotkey::RightShift => "shift_r",
        Hotkey::RightAlt => "alt_r",
        Hotkey::Pause => "pause",
        Hotkey::ScrollLock => "scroll_lock",
    }
}

fn egui_key_to_hotkey_value(key: Key, modifiers: Modifiers) -> Option<String> {
    let fn_trigger: Option<&'static str> = match key {
        Key::F1 => Some("f1"),
        Key::F2 => Some("f2"),
        Key::F3 => Some("f3"),
        Key::F4 => Some("f4"),
        Key::F5 => Some("f5"),
        Key::F6 => Some("f6"),
        Key::F7 => Some("f7"),
        Key::F8 => Some("f8"),
        Key::F9 => Some("f9"),
        Key::F10 => Some("f10"),
        Key::F11 => Some("f11"),
        Key::F12 => Some("f12"),
        Key::F13 => Some("f13"),
        Key::F14 => Some("f14"),
        Key::F15 => Some("f15"),
        Key::F16 => Some("f16"),
        Key::F17 => Some("f17"),
        Key::F18 => Some("f18"),
        Key::F19 => Some("f19"),
        Key::F20 => Some("f20"),
        Key::F21 => Some("f21"),
        Key::F22 => Some("f22"),
        Key::F23 => Some("f23"),
        Key::F24 => Some("f24"),
        _ => None,
    };

    let mut mods: Vec<&'static str> = Vec::with_capacity(3);
    if modifiers.ctrl || modifiers.command {
        mods.push("ctrl");
    }
    if modifiers.alt {
        mods.push("alt");
    }
    if modifiers.shift {
        mods.push("shift");
    }

    if let Some(f) = fn_trigger {
        if mods.is_empty() {
            return Some(f.to_owned());
        }
        return Some(format!("{}+{f}", mods.join("+")));
    }

    if mods.is_empty() {
        return None;
    }

    let trigger: &'static str = match key {
        Key::A => "a",
        Key::B => "b",
        Key::C => "c",
        Key::D => "d",
        Key::E => "e",
        Key::F => "f",
        Key::G => "g",
        Key::H => "h",
        Key::I => "i",
        Key::J => "j",
        Key::K => "k",
        Key::L => "l",
        Key::M => "m",
        Key::N => "n",
        Key::O => "o",
        Key::P => "p",
        Key::Q => "q",
        Key::R => "r",
        Key::S => "s",
        Key::T => "t",
        Key::U => "u",
        Key::V => "v",
        Key::W => "w",
        Key::X => "x",
        Key::Y => "y",
        Key::Z => "z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::Space => "space",
        Key::Enter => "enter",
        Key::Backspace => "backspace",
        Key::ArrowLeft => "arrow_left",
        Key::ArrowUp => "arrow_up",
        Key::ArrowRight => "arrow_right",
        Key::ArrowDown => "arrow_down",
        Key::PageUp => "page_up",
        Key::PageDown => "page_down",
        Key::End => "end",
        Key::Home => "home",
        Key::Insert => "insert",
        Key::Delete => "delete",
        _ => return None,
    };

    Some(format!("{}+{trigger}", mods.join("+")))
}
