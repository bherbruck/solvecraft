//! The assistant sidebar: the panel on the right of the viewport.
//!
//! Everything it does goes through [`super`]: the conversation state and `send`, `stop`,
//! `answer`, `undo_turn`. This file only draws and turns clicks into those calls.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Margin, RichText, Stroke, vec2};
use serde_json::Value;
use solidcraft_assistant::markdown::{self, Block, Span};
use solidcraft_assistant::settings::BackendKind;
use solidcraft_assistant::slash;

use super::{Item, Sent, ToolCard};
use crate::SolveApp;
use crate::theme::Tokens;

/// Narrowest and widest the panel can be dragged.
pub const MIN_WIDTH: f32 = 300.0;
pub const MAX_WIDTH: f32 = 720.0;

/// Longest tool input or output shown in a card.
const CARD_TEXT: usize = 4000;

fn id_input() -> egui::Id {
    egui::Id::new("sc_assistant_input")
}

/// Draw the sidebar when it is open. Must be called before the central panel.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !app.ui.show_assistant {
        return;
    }
    app.assistant.ensure_loaded();
    let t = Tokens::get();
    let width = app.preferences.assistant_width.clamp(MIN_WIDTH, MAX_WIDTH);
    let out = egui::Panel::right("sc_assistant")
        .default_size(width)
        .size_range(MIN_WIDTH..=MAX_WIDTH)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            header(app, ui);
            ui.add_space(4.0);
            if app.assistant.settings_open {
                settings(app, ui);
            } else {
                conversation(app, ui);
            }
        });
    let w = out.response.rect.width();
    if w.is_finite() && (w - app.preferences.assistant_width).abs() > 0.5 {
        app.preferences.assistant_width = w.clamp(MIN_WIDTH, MAX_WIDTH);
    }
    crate::scenario::publish_rect("assistant:panel", out.response.rect);
}

/// A small square button drawn with one of the app's own icons.
fn icon_button(ui: &mut egui::Ui, icon: &str, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover);
    }
    crate::icons::paint(ui.painter(), r.shrink(4.0), icon, t.text, t.panel_header, t.accent);
    resp
}

fn header(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::hover());
        crate::icons::paint(ui.painter(), r.shrink(2.0), "assistant", t.text, t.panel_header, t.accent);
        ui.label(RichText::new(app.tr("assistant.title")).size(13.0).strong().color(t.text));
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let (r, close) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
            if close.hovered() {
                ui.painter().rect_filled(r, 4.0, t.hover);
            }
            crate::icons::glyphs::cross(ui.painter(), r.shrink(7.0), Stroke::new(1.5, t.text));
            if close.on_hover_text(app.tr("assistant.close")).clicked() {
                app.ui.show_assistant = false;
            }
            let gear = icon_button(ui, "settings", app.assistant.settings_open).on_hover_text(app.tr("assistant.settings"));
            crate::scenario::publish_handle("assistant:settings", gear.rect.center());
            if gear.clicked() {
                app.assistant.settings_open = !app.assistant.settings_open;
            }
            let mut chosen: Option<String> = None;
            let mut export = false;
            let history = ui.menu_button(RichText::new(" ").size(14.0), |ui| {
                let all = super::saved_conversations(app);
                if all.is_empty() {
                    ui.label(RichText::new(app.tr("assistant.no_history")).size(12.0).color(t.text_dim));
                }
                for c in all.iter().take(20) {
                    let title = if c.title.is_empty() { "…" } else { c.title.as_str() };
                    if ui.button(title).clicked() {
                        chosen = Some(c.id.clone());
                        ui.close();
                    }
                }
                ui.separator();
                if ui.add_enabled(!app.assistant.items.is_empty(), egui::Button::new(app.tr("assistant.export"))).clicked() {
                    export = true;
                    ui.close();
                }
            });
            let hr = history.response.rect;
            crate::icons::paint(
                ui.painter(),
                hr.shrink2(vec2((hr.width() - 16.0).max(0.0) / 2.0, (hr.height() - 16.0).max(0.0) / 2.0)),
                "history",
                t.text,
                t.panel_header,
                t.accent,
            );
            history.response.on_hover_text(app.tr("assistant.history"));
            if let Some(id) = chosen {
                super::load_conversation(app, &id);
            }
            if export {
                let text = super::export_text(app);
                app.pick_save("assistant-conversation.md", &["md"], move |_app, path| {
                    if let Some(path) = path {
                        let _ = std::fs::write(path, text);
                    }
                });
            }
            let new = icon_button(ui, "new", false).on_hover_text(app.tr("assistant.new_chat"));
            crate::scenario::publish_handle("assistant:new", new.rect.center());
            if new.clicked() {
                app.assistant.new_chat();
            }
        });
    });
    ui.separator();
}

fn conversation(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // The composer sits at the bottom; the transcript takes what is left.
    egui::Panel::bottom("sc_assistant_composer").frame(egui::Frame::NONE).show_separator_line(false).show(ui, |ui| composer(app, ui));
    if !app.assistant.confirms.is_empty() || !app.assistant.asks.is_empty() {
        egui::Panel::bottom("sc_assistant_confirms").frame(egui::Frame::NONE).show_separator_line(false).show(ui, |ui| confirms(app, ui));
    }
    let empty = app.assistant.items.is_empty();
    let stick = app.assistant.scroll_to_end;
    app.assistant.scroll_to_end = false;
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if empty {
            empty_state(app, ui);
            return;
        }
        let mut select: Option<u64> = None;
        let n = app.assistant.items.len();
        for i in 0..n {
            let Some(item) = app.assistant.items.get(i).cloned() else { break };
            match item {
                Item::User { text, picture } => user_bubble(ui, &text, picture),
                Item::Assistant { text, thinking } => assistant_bubble(ui, &text, &thinking),
                Item::Tool(card) => {
                    if let Some(id) = tool_card(app, ui, &card) {
                        select = Some(id);
                    }
                }
                Item::Notice { text, error } => {
                    ui.add_space(2.0);
                    if error {
                        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(t.error)).wrap());
                    } else {
                        render_markdown(ui, &text);
                    }
                }
                Item::TurnEnd { .. } => {}
            }
            ui.add_space(6.0);
        }
        if app.assistant.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(app.tr("assistant.working")).size(12.0).color(t.text_dim));
            });
        }
        if let Some((_, edited)) = app.assistant.undoable(&app.session)
            && !app.assistant.busy()
        {
            let label = if edited { app.tr("assistant.undo_edited") } else { app.tr("assistant.undo_turn") };
            let b = ui.button(label);
            crate::scenario::publish_handle("assistant:undo", b.rect.center());
            if b.clicked() {
                let _ = super::undo_turn(app, edited);
            }
        }
        if stick {
            ui.scroll_to_cursor(Some(Align::BOTTOM));
        }
        if let Some(id) = select {
            let _ = app.run("select.set", serde_json::json!({"items": [{"type": "feature", "id": id}]}));
        }
    });
}

fn empty_state(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
        crate::icons::paint(ui.painter(), r, "assistant", t.text_dim, t.panel_header, t.accent);
        ui.add_space(6.0);
        ui.label(RichText::new(app.tr("assistant.empty_title")).size(15.0).strong().color(t.text));
        ui.add_space(4.0);
        ui.label(RichText::new(app.tr("assistant.empty_body")).size(12.0).color(t.text_dim));
    });
    ui.add_space(14.0);
    let has_bodies = !app.session.model.state().bodies.is_empty();
    let failing = !super::situation_failing(app).is_empty();
    for s in slash::suggestions(has_bodies, failing) {
        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(RichText::new(s).size(12.0))).clicked() {
            app.assistant.input = s.to_string();
            ui.memory_mut(|m| m.request_focus(id_input()));
        }
    }
    let s = &app.assistant.settings;
    if !s.privacy_notice_seen {
        ui.add_space(14.0);
        ui.label(RichText::new(app.tr("assistant.privacy")).size(11.0).color(t.text_dim));
        if ui.button(app.tr("assistant.privacy_ok")).clicked() {
            app.assistant.settings.privacy_notice_seen = true;
            app.assistant.save_settings();
        }
    }
}

fn user_bubble(ui: &mut egui::Ui, text: &str, picture: bool) {
    let t = Tokens::get();
    egui::Frame::NONE.fill(t.accent_soft).corner_radius(6.0).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let shown = if picture { format!("{text}\n({})", crate::i18n::tr_label("assistant.attached")) } else { text.to_string() };
        ui.add(egui::Label::new(RichText::new(shown).size(13.0).color(t.text)).wrap());
    });
}

fn assistant_bubble(ui: &mut egui::Ui, text: &str, thinking: &str) {
    let t = Tokens::get();
    if !thinking.is_empty() {
        egui::CollapsingHeader::new(RichText::new("…").size(11.0).color(t.text_dim)).id_salt(("think", thinking.len() / 512)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(thinking).size(11.0).italics().color(t.text_dim)).wrap());
        });
    }
    if !text.is_empty() {
        render_markdown(ui, text);
    }
}

fn job(ui: &egui::Ui, spans: &[Span], base: &FontId, color: Color32) -> LayoutJob {
    let t = Tokens::get();
    let mut j = LayoutJob::default();
    j.wrap.max_width = ui.available_width();
    for s in spans {
        let plain = TextFormat { font_id: base.clone(), color, ..Default::default() };
        match s {
            Span::Text(x) => j.append(x, 0.0, plain),
            Span::Bold(x) => {
                j.append(x, 0.0, TextFormat { font_id: FontId::proportional(base.size), color: t.text, underline: Stroke::NONE, ..plain })
            }
            Span::Italic(x) => j.append(x, 0.0, TextFormat { italics: true, ..plain }),
            Span::Code(x) => j.append(x, 0.0, TextFormat { font_id: FontId::monospace(base.size - 1.0), background: t.field, ..plain }),
            Span::Link { label, .. } => j.append(label, 0.0, TextFormat { color: t.accent, underline: Stroke::new(1.0, t.accent), ..plain }),
        }
    }
    j
}

fn links(ui: &mut egui::Ui, spans: &[Span]) {
    let found: Vec<(&str, &str)> =
        spans.iter().filter_map(|s| if let Span::Link { label, url } = s { Some((label.as_str(), url.as_str())) } else { None }).collect();
    if found.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (label, url) in found {
            ui.hyperlink_to(RichText::new(label).size(11.0), url).on_hover_text(url);
        }
    });
}

/// Draw `src` as Markdown.
pub fn render_markdown(ui: &mut egui::Ui, src: &str) {
    let t = Tokens::get();
    let body = FontId::proportional(13.0);
    for block in markdown::parse(src) {
        match block {
            Block::Paragraph(spans) => {
                ui.add(egui::Label::new(job(ui, &spans, &body, t.text)).wrap());
                links(ui, &spans);
            }
            Block::Heading { level, spans } => {
                let size = match level {
                    1 => 17.0,
                    2 => 15.0,
                    _ => 14.0,
                };
                ui.add_space(4.0);
                let plain = markdown::plain(&spans);
                ui.add(egui::Label::new(RichText::new(plain).size(size).strong().color(t.text)).wrap());
            }
            Block::Code { text, .. } => {
                egui::Frame::NONE.fill(t.field).corner_radius(4.0).inner_margin(Margin::same(6)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&text).monospace().size(12.0).color(t.text)).wrap());
                    if ui.small_button(crate::i18n::tr_label("assistant.copy")).clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            }
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        let mark = if ordered { format!("{}.", start.saturating_add(i as u64)) } else { "•".to_string() };
                        ui.label(RichText::new(mark).size(13.0).color(t.text_dim));
                        ui.vertical(|ui| {
                            ui.add(egui::Label::new(job(ui, item, &body, t.text)).wrap());
                            links(ui, item);
                        });
                    });
                }
            }
            Block::Quote(spans) => {
                ui.horizontal_top(|ui| {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, t.border);
                    ui.add(egui::Label::new(job(ui, &spans, &body, t.text_dim)).wrap());
                });
            }
            Block::Pre(text) => {
                egui::ScrollArea::horizontal().id_salt(("pre", text.len())).show(ui, |ui| {
                    ui.label(RichText::new(text).monospace().size(11.0).color(t.text));
                });
            }
            Block::Rule => {
                ui.separator();
            }
        }
        ui.add_space(4.0);
    }
}

/// A tool call: a collapsed card with its title and state. Returns a feature id to select.
fn tool_card(app: &mut SolveApp, ui: &mut egui::Ui, card: &ToolCard) -> Option<u64> {
    let t = Tokens::get();
    let (state, color) = match &card.result {
        None => (app.tr("assistant.tool_running"), t.text_dim),
        Some(r) if r.is_error => (app.tr("assistant.tool_failed"), t.error),
        Some(_) => (app.tr("assistant.tool_done"), t.accent),
    };
    let mut select = None;
    egui::Frame::NONE.fill(t.panel_header).corner_radius(4.0).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let title = RichText::new(format!("{}  ·  {state}", card.title)).size(12.0).color(color);
        egui::CollapsingHeader::new(title).id_salt(("tool", &card.id)).default_open(card.result.as_ref().is_some_and(|r| r.is_error)).show(
            ui,
            |ui| {
                let input = serde_json::to_string_pretty(&card.input).unwrap_or_default();
                ui.add(
                    egui::Label::new(
                        RichText::new(solidcraft_assistant::model::truncate(&input, CARD_TEXT, "cut")).monospace().size(11.0).color(t.text_dim),
                    )
                    .wrap(),
                );
                if let Some(r) = &card.result {
                    ui.separator();
                    let c = if r.is_error { t.error } else { t.text };
                    ui.add(
                        egui::Label::new(
                            RichText::new(solidcraft_assistant::model::truncate(&r.text, CARD_TEXT, "cut")).monospace().size(11.0).color(c),
                        )
                        .wrap(),
                    );
                    if r.images > 0 {
                        ui.label(RichText::new(format!("({})", app.tr("assistant.picture"))).size(11.0).color(t.text_dim));
                    }
                }
            },
        );
        if let Some(r) = &card.result
            && !r.is_error
            && let Some(id) = feature_ref(&r.text)
            && ui.small_button(app.tr("assistant.select_feature")).clicked()
        {
            select = Some(id);
        }
    });
    select
}

/// The id of the last feature a tool result names (`feature`, also in a batch's `results`).
pub fn feature_ref(text: &str) -> Option<u64> {
    let v: Value = serde_json::from_str(text).ok()?;
    let direct = |v: &Value| v.get("feature").and_then(Value::as_u64);
    direct(&v).or_else(|| v.get("results").and_then(Value::as_array).and_then(|a| a.iter().rev().find_map(direct)))
}

fn confirms(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // Questions from an external agent: its own options, as buttons.
    let mut reply: Option<(usize, Option<String>)> = None;
    for (i, a) in app.assistant.asks.iter().enumerate() {
        egui::Frame::NONE
            .fill(t.warning.gamma_multiply(0.18))
            .stroke(Stroke::new(1.0, t.warning))
            .corner_radius(4.0)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(app.tr("assistant.ask_title")).size(12.0).strong().color(t.text));
                let what = if a.request.title.is_empty() { a.request.kind.clone() } else { a.request.title.clone() };
                ui.add(egui::Label::new(RichText::new(what).monospace().size(11.0).color(t.text)).wrap());
                if !a.request.raw_input.is_null() {
                    let raw = solidcraft_assistant::model::truncate(&a.request.raw_input.to_string(), 600, "cut");
                    ui.add(egui::Label::new(RichText::new(raw).monospace().size(10.0).color(t.text_dim)).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    for o in &a.request.options {
                        let b = ui.button(&o.name);
                        if b.clicked() {
                            reply = Some((i, Some(o.id.clone())));
                        }
                    }
                    if ui.button(app.tr("assistant.deny")).clicked() {
                        reply = Some((i, None));
                    }
                });
            });
        ui.add_space(4.0);
    }
    if let Some((i, option)) = reply {
        app.assistant.answer_ask(i, option);
    }
    let mut answer: Option<(usize, bool)> = None;
    for (i, c) in app.assistant.confirms.iter().enumerate() {
        egui::Frame::NONE
            .fill(t.warning.gamma_multiply(0.18))
            .stroke(Stroke::new(1.0, t.warning))
            .corner_radius(4.0)
            .inner_margin(Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(app.tr("assistant.confirm_title")).size(12.0).strong().color(t.text));
                for l in &c.lines {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(11.0).color(t.text)).wrap());
                }
                ui.horizontal(|ui| {
                    let allow = ui.button(app.tr("assistant.allow"));
                    crate::scenario::publish_handle(&format!("assistant:allow:{i}"), allow.rect.center());
                    if allow.clicked() {
                        answer = Some((i, true));
                    }
                    if ui.button(app.tr("assistant.deny")).clicked() {
                        answer = Some((i, false));
                    }
                });
            });
        ui.add_space(4.0);
    }
    if let Some((i, allow)) = answer {
        app.assistant.answer(i, allow);
    }
}

fn composer(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let busy = app.assistant.busy();
    // Quick-action chips while a slash word is being typed.
    let typing = app.assistant.input.trim_start();
    if let Some(word) = typing.strip_prefix('/')
        && !word.contains(char::is_whitespace)
    {
        let words: Vec<String> = slash::matching(word).iter().map(|q| q.name.to_string()).collect();
        ui.horizontal_wrapped(|ui| {
            for w in words {
                if ui.small_button(format!("/{w}")).clicked() {
                    app.assistant.input = format!("/{w} ");
                    ui.memory_mut(|m| m.request_focus(id_input()));
                }
            }
        });
    }
    // Enter sends, Shift+Enter is a new line.
    let focused = ui.memory(|m| m.has_focus(id_input()));
    let enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let hint = app.tr("assistant.placeholder");
    ui.add(
        egui::TextEdit::multiline(&mut app.assistant.input)
            .id(id_input())
            .hint_text(RichText::new(hint).color(t.text_dim))
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .lock_focus(true),
    );
    let mut go = enter;
    ui.horizontal(|ui| {
        if app.assistant.picture.is_some() {
            ui.label(RichText::new(app.tr("assistant.attached")).size(11.0).color(t.text_dim));
        } else if app.services.assistant_http.is_some()
            && icon_button(ui, "view_attach", false).on_hover_text(app.tr("assistant.attach_view")).clicked()
        {
            super::attach_view(app);
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if busy {
                if ui.button(app.tr("assistant.stop")).clicked() {
                    app.assistant.stop();
                }
            } else {
                let b = ui.add_enabled(!app.assistant.input.trim().is_empty(), egui::Button::new(app.tr("assistant.send")));
                crate::scenario::publish_handle("assistant:send", b.rect.center());
                go |= b.clicked();
            }
            let u = app.assistant.usage;
            if u.input + u.output > 0 {
                let text = crate::i18n::format("assistant.usage", &[("input", &compact(u.input)), ("output", &compact(u.output))]);
                ui.label(RichText::new(text).size(10.0).color(t.text_dim));
            }
        });
    });
    if go && !busy {
        let ctx = ui.ctx().clone();
        let text = std::mem::take(&mut app.assistant.input);
        match super::send(app, &ctx, &text) {
            Sent::Started | Sent::Handled => {}
            Sent::Refused(why) => {
                // Keep what was typed; say why.
                app.assistant.input = text;
                app.assistant.items.push(Item::Notice { text: why, error: true });
                app.assistant.scroll_to_end = true;
            }
        }
        ui.memory_mut(|m| m.request_focus(id_input()));
    }
}

/// 1234 → "1.2k".
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

fn settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    if ui.button(app.tr("assistant.back")).clicked() {
        app.assistant.settings_open = false;
    }
    ui.add_space(6.0);
    let mut changed = false;
    ui.label(RichText::new(app.tr("assistant.backend")).size(12.0).strong().color(t.text));
    for (kind, key) in [
        (BackendKind::Anthropic, "assistant.backend_anthropic"),
        (BackendKind::OpenAi, "assistant.backend_openai"),
        (BackendKind::Agent, "assistant.backend_agent"),
    ] {
        if ui.radio(app.assistant.settings.backend == kind, app.tr(key)).clicked() {
            app.assistant.settings.backend = kind;
            changed = true;
        }
    }
    ui.add_space(8.0);
    match app.assistant.settings.backend {
        BackendKind::Agent => agent_settings(app, ui, &mut changed),
        kind => {
            let (model_label, url_label, key_label) =
                (app.tr("assistant.model").to_string(), app.tr("assistant.base_url").to_string(), app.tr("assistant.api_key").to_string());
            let resolved_from_env = {
                let api = app.assistant.settings.api();
                api.api_key.is_empty() && !api.resolved_key(kind).is_empty()
            };
            let has_key = !app.assistant.settings.api().api_key.is_empty();
            let (save_key, clear_key, saved, from_env, none) = (
                app.tr("assistant.save_key").to_string(),
                app.tr("assistant.clear_key").to_string(),
                app.tr("assistant.key_saved").to_string(),
                app.tr("assistant.key_env").to_string(),
                app.tr("assistant.key_none").to_string(),
            );
            let (vision, allow_http) = (app.tr("assistant.vision").to_string(), app.tr("assistant.allow_http").to_string());
            let st = &mut app.assistant;
            let api = if kind == BackendKind::OpenAi { &mut st.settings.openai } else { &mut st.settings.anthropic };
            ui.label(RichText::new(url_label).size(12.0).color(t.text_dim));
            changed |= ui.add(egui::TextEdit::singleline(&mut api.base_url).desired_width(f32::INFINITY)).changed();
            ui.label(RichText::new(model_label).size(12.0).color(t.text_dim));
            changed |= ui.add(egui::TextEdit::singleline(&mut api.model).desired_width(f32::INFINITY)).changed();
            ui.label(RichText::new(key_label).size(12.0).color(t.text_dim));
            ui.add(egui::TextEdit::singleline(&mut st.key_edit).password(true).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.add_enabled(!st.key_edit.trim().is_empty(), egui::Button::new(save_key)).clicked() {
                    api.api_key = solidcraft_assistant::settings::Secret::new(st.key_edit.trim());
                    st.key_edit.clear();
                    changed = true;
                }
                if has_key && ui.button(clear_key).clicked() {
                    api.api_key = Default::default();
                    changed = true;
                }
            });
            let status = if has_key {
                saved
            } else if resolved_from_env {
                from_env
            } else {
                none
            };
            ui.label(RichText::new(status).size(11.0).color(t.text_dim));
            ui.add_space(6.0);
            changed |= ui.checkbox(&mut api.vision, vision).changed();
            if kind == BackendKind::OpenAi {
                changed |= ui.checkbox(&mut api.allow_http, allow_http).changed();
            }
        }
    }
    ui.add_space(10.0);
    ui.label(RichText::new(app.tr("assistant.privacy")).size(11.0).color(t.text_dim));
    if changed {
        app.assistant.save_settings();
    }
}

fn agent_settings(app: &mut SolveApp, ui: &mut egui::Ui, changed: &mut bool) {
    use solidcraft_assistant::settings::AgentMode;
    let t = Tokens::get();
    let (none, add, remove, command, args, acp, print, hint, cli, auto) = (
        app.tr("assistant.agent_none").to_string(),
        app.tr("assistant.agent_add").to_string(),
        app.tr("assistant.agent_remove").to_string(),
        app.tr("assistant.agent_command").to_string(),
        app.tr("assistant.agent_args").to_string(),
        app.tr("assistant.agent_mode_acp").to_string(),
        app.tr("assistant.agent_mode_print").to_string(),
        app.tr("assistant.agent_hint").to_string(),
        app.tr("assistant.cli").to_string(),
        app.tr("assistant.auto_allow").to_string(),
    );
    let st = &mut app.assistant;
    if st.settings.agents.is_empty() {
        ui.label(RichText::new(none).size(12.0).color(t.text_dim));
    }
    let n = st.settings.agents.len();
    for i in 0..n {
        let name = st.settings.agents.get(i).map(|a| a.name.clone()).unwrap_or_default();
        if ui.radio(st.settings.agent == i, name).clicked() {
            st.settings.agent = i;
            *changed = true;
        }
    }
    let mut remove_at: Option<usize> = None;
    if let Some(a) = st.settings.agents.get_mut(st.settings.agent) {
        ui.add_space(4.0);
        ui.label(RichText::new(command).size(12.0).color(t.text_dim));
        *changed |= ui.add(egui::TextEdit::singleline(&mut a.command).desired_width(f32::INFINITY)).changed();
        ui.label(RichText::new(args).size(12.0).color(t.text_dim));
        let mut text = a.args.join("\n");
        if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(f32::INFINITY)).changed() {
            a.args = text.lines().map(str::to_string).filter(|l| !l.is_empty()).take(solidcraft_assistant::settings::MAX_ARGS).collect();
            *changed = true;
        }
        ui.horizontal(|ui| {
            *changed |= ui.radio_value(&mut a.mode, AgentMode::Acp, &acp).changed();
            *changed |= ui.radio_value(&mut a.mode, AgentMode::Print, &print).changed();
        });
        if ui.button(remove).clicked() {
            remove_at = Some(st.settings.agent);
        }
    }
    if let Some(i) = remove_at {
        st.settings.agents.remove(i);
        st.settings.agent = 0;
        *changed = true;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(add).size(12.0).color(t.text_dim));
    ui.horizontal_wrapped(|ui| {
        for p in solidcraft_assistant::acp::presets::presets() {
            if st.settings.agents.len() < solidcraft_assistant::settings::MAX_AGENTS && ui.small_button(&p.name).clicked() {
                st.settings.agents.push(p);
                st.settings.agent = st.settings.agents.len() - 1;
                *changed = true;
            }
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(cli).size(12.0).color(t.text_dim));
    *changed |= ui.add(egui::TextEdit::singleline(&mut st.settings.cli).desired_width(f32::INFINITY)).changed();
    *changed |= ui.checkbox(&mut st.settings.auto_allow_tools, auto).changed();
    ui.add_space(4.0);
    ui.label(RichText::new(hint).size(11.0).color(t.text_dim));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_found_in_results() {
        assert_eq!(feature_ref(r#"{"feature": 4, "name": "Extrude 1"}"#), Some(4));
        assert_eq!(feature_ref(r#"{"ok": true, "results": [{"feature": 2}, {"x": 1}, {"feature": 7}]}"#), Some(7));
        assert_eq!(feature_ref("not json"), None);
        assert_eq!(feature_ref(r#"{"results": "x"}"#), None);
    }

    #[test]
    fn counts_are_short() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_234), "1.2k");
        assert_eq!(compact(2_500_000), "2.5M");
    }
}
