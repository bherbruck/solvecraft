//! The Change Parameters dialog and parameter-name autocomplete for expression boxes.
//!
//! A window over the engine's `parameters.*` commands: favourites first, then user parameters
//! (name, unit, expression, value, comment; add, rename, delete) and model parameters grouped by
//! the sketch or feature they drive (editable). Expressions validate as you type
//! (`expr.evaluate`) with a live model preview; Enter or leaving the field applies them.
//! Import and export go through `parameters.import` / `parameters.export`.
//!
//! [`complete`] adds suggestions (parameters, functions, units) under any expression box: the
//! names come from `parameters.complete`, cached per document revision by [`publish`].

use std::sync::Arc;

use egui::{Color32, RichText};
use serde_json::{Value, json};

use crate::SolveApp;
use crate::theme::Tokens;

const UNITS: [&str; 9] = ["mm", "cm", "m", "in", "ft", "deg", "rad", "", "um"];

/// Dialog state (what is being typed; the parameters themselves live in the document).
#[derive(Default)]
pub struct ParamsDialog {
    pub filter: String,
    /// Expressions being edited, by parameter name.
    pub editing: std::collections::BTreeMap<String, String>,
    /// Comments being edited, by parameter name.
    pub comments: std::collections::BTreeMap<String, String>,
    /// A parameter being renamed: (old name, new name).
    pub renaming: Option<(String, String)>,
    pub new_name: String,
    pub new_unit: usize,
    pub new_expr: String,
    pub new_comment: String,
    /// The last error from an action in the dialog.
    pub error: Option<String>,
    /// The expression edit to preview: (name, expression).
    pub preview: Option<(String, String)>,
}

impl ParamsDialog {
    pub fn new() -> Self {
        ParamsDialog { new_expr: "10 mm".into(), ..Default::default() }
    }
    /// The command the live preview should run (a valid expression being edited).
    pub fn preview_commands(&self) -> Option<Vec<(String, Value)>> {
        let (n, e) = self.preview.as_ref()?;
        Some(vec![("parameters.change".to_string(), json!({"name": n, "expression": e}))])
    }
}

/// One row as the engine lists it.
#[derive(Clone, Debug, Default)]
struct Row {
    name: String,
    expression: String,
    unit: String,
    value: Option<f64>,
    error: Option<String>,
    comment: String,
    source: String,
    group: String,
    favorite: bool,
}

fn rows(app: &mut SolveApp) -> Vec<Row> {
    let list = app.session.execute("parameters.list", &json!({})).unwrap_or_default();
    // Sketch dimension parameters: the sketch that owns each.
    let mut owner: std::collections::BTreeMap<String, String> = Default::default();
    for f in &app.session.doc.features {
        if let solvecraft_engine::doc::FeatureKind::Sketch { sketch, .. } = &f.kind {
            for c in &sketch.constraints {
                if let Some(p) = &c.param {
                    owner.insert(p.clone(), f.name.clone());
                }
            }
        }
    }
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    list["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let name = s(r, "name");
            let source = s(r, "source");
            let group = match source.as_str() {
                "feature" => s(r, "feature"),
                "sketch" => owner.get(&name).cloned().unwrap_or_else(|| "Sketch".into()),
                _ => String::new(),
            };
            Row {
                expression: s(r, "expression"),
                unit: s(r, "unit"),
                value: r.get("value").and_then(Value::as_f64),
                error: r.get("error").and_then(Value::as_str).map(str::to_string),
                comment: s(r, "comment"),
                favorite: r.get("favorite").and_then(Value::as_bool).unwrap_or(false),
                source,
                group,
                name,
            }
        })
        .collect()
}

fn show_value(r: &Row) -> String {
    match r.value {
        Some(v) if r.unit.is_empty() => format!("{v:.4}"),
        Some(v) => format!("{v:.4} {}", r.unit),
        None => "—".into(),
    }
}

/// Open the dialog (Change Parameters on the toolbar).
pub fn open(app: &mut SolveApp) {
    app.params = Some(ParamsDialog::new());
}

/// Draw the dialog when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    publish(app, ctx);
    let Some(mut d) = app.params.take() else { return };
    let t = Tokens::get();
    let mut open = true;
    let all = rows(app);
    crate::frame::window(ctx, RichText::new("PARAMETERS").strong().size(13.0), crate::frame::Width::Wide)
        .id(egui::Id::new("sc_params_dialog"))
        .open(&mut open)
        .default_width(740.0)
        .default_height(520.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.filter).hint_text("Filter").desired_width(180.0));
                ui.separator();
                if ui.button("Import…").on_hover_text("CSV or JSON: updates existing parameters, adds new ones").clicked()
                    && let Some(p) = app.services.pick_open.as_ref().and_then(|f| f())
                {
                    d.error = app.run("parameters.import", json!({ "path": p })).err();
                }
                for (label, ext) in [("Export CSV…", "csv"), ("Export JSON…", "json")] {
                    if ui.button(label).clicked()
                        && let Some(p) = app.services.pick_save.as_ref().and_then(|f| f(&format!("parameters.{ext}"), &[ext]))
                    {
                        d.error = app.run("parameters.export", json!({ "path": p, "format": ext })).err();
                    }
                }
            });
            if let Some(e) = &d.error {
                ui.label(RichText::new(e).color(t.error));
            }
            ui.separator();
            let f = d.filter.to_ascii_lowercase();
            let visible: Vec<&Row> = all
                .iter()
                .filter(|r| {
                    f.is_empty()
                        || r.name.to_ascii_lowercase().contains(&f)
                        || r.comment.to_ascii_lowercase().contains(&f)
                        || r.group.to_ascii_lowercase().contains(&f)
                })
                .collect();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let favs: Vec<&Row> = visible.iter().copied().filter(|r| r.favorite).collect();
                if !favs.is_empty() {
                    section(ui, &t, "Favorites");
                    table(app, ui, &mut d, &favs, "fav");
                }
                section(ui, &t, "User Parameters");
                let users: Vec<&Row> = visible.iter().copied().filter(|r| r.source == "user").collect();
                table(app, ui, &mut d, &users, "user");
                add_row(app, ui, &mut d);
                section(ui, &t, "Model Parameters");
                let mut groups: Vec<String> = Vec::new();
                for r in visible.iter().filter(|r| r.source != "user") {
                    if !groups.contains(&r.group) {
                        groups.push(r.group.clone());
                    }
                }
                for g in groups {
                    let members: Vec<&Row> = visible.iter().copied().filter(|r| r.source != "user" && r.group == g).collect();
                    egui::CollapsingHeader::new(RichText::new(&g).strong()).id_salt(("pgroup", &g)).default_open(true).show(ui, |ui| {
                        table(app, ui, &mut d, &members, &g);
                    });
                }
            });
        });
    if !open {
        app.params = None;
        return;
    }
    app.params = Some(d);
}

fn section(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(t.text));
}

/// Live check of an expression typed for a parameter of this unit: Ok(shown value) or the message.
fn check(app: &mut SolveApp, e: &str, unit: &str) -> Result<String, String> {
    let r = app.session.execute("expr.evaluate", &json!({"expression": e, "unit": unit})).map_err(|e| e.to_string())?;
    if r["ok"].as_bool().unwrap_or(false) {
        let v = r["display_value"].as_f64().unwrap_or(0.0);
        Ok(if unit.is_empty() { format!("{v:.4}") } else { format!("{v:.4} {unit}") })
    } else {
        Err(r["error"].as_str().unwrap_or("invalid").to_string())
    }
}

fn table(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog, rows: &[&Row], salt: &str) {
    let t = Tokens::get();
    egui::Grid::new(("ptable", salt)).num_columns(7).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        for h in ["", "Name", "Unit", "Expression", "Value", "Comment", ""] {
            ui.label(RichText::new(h).color(t.text_dim).small());
        }
        ui.end_row();
        for r in rows {
            // Favourite star.
            let star = if r.favorite { RichText::new("★").color(t.accent) } else { RichText::new("☆").color(t.text_dim) };
            if ui.add(egui::Button::new(star).frame(false)).on_hover_text("Favourite").clicked() {
                d.error = app.run("parameters.favorite", json!({"name": r.name, "favorite": !r.favorite})).err();
            }
            // Name: double-click to rename.
            match &mut d.renaming {
                Some((old, new)) if *old == r.name => {
                    let resp = ui.add_sized([110.0, 20.0], egui::TextEdit::singleline(new));
                    if resp.lost_focus() {
                        let (old, new) = (old.clone(), new.trim().to_string());
                        if !new.is_empty() && new != old {
                            d.error = app.run("parameters.rename", json!({"name": old, "new_name": new})).err();
                        }
                        d.renaming = None;
                    } else {
                        resp.request_focus();
                    }
                }
                _ => {
                    let name = if r.source == "user" { RichText::new(&r.name) } else { RichText::new(&r.name).color(t.text_dim) };
                    if ui.add(egui::Label::new(name).sense(egui::Sense::click())).on_hover_text("Double-click to rename").double_clicked() {
                        d.renaming = Some((r.name.clone(), r.name.clone()));
                    }
                }
            }
            // Unit: user parameters choose it; others follow their input.
            if r.source == "user" {
                let mut sel = UNITS.iter().position(|u| *u == r.unit).unwrap_or(0);
                let before = sel;
                egui::ComboBox::from_id_salt(("punit", &r.name)).width(56.0).selected_text(unit_label(&r.unit)).show_ui(ui, |ui| {
                    for (i, u) in UNITS.iter().enumerate() {
                        ui.selectable_value(&mut sel, i, unit_label(u));
                    }
                });
                if sel != before
                    && let Some(u) = UNITS.get(sel)
                {
                    d.error = app.run("parameters.change", json!({"name": r.name, "expression": r.expression, "unit": u})).err();
                }
            } else {
                ui.label(RichText::new(unit_label(&r.unit)).color(t.text_dim));
            }
            // Expression: live check, applied on Enter or leaving the field.
            let mut e = d.editing.get(&r.name).cloned().unwrap_or_else(|| r.expression.clone());
            let live = if e != r.expression { Some(check(app, &e, &r.unit)) } else { None };
            let bad = matches!(live, Some(Err(_))) || (live.is_none() && r.error.is_some());
            let mut te = egui::TextEdit::singleline(&mut e).desired_width(170.0);
            if bad {
                te = te.text_color(t.error);
            }
            let resp = ui.add_sized([170.0, 20.0], te);
            let completed = complete(ui, &resp, &mut e);
            if resp.changed() || completed {
                d.editing.insert(r.name.clone(), e.clone());
            }
            if resp.has_focus() && matches!(live, Some(Ok(_))) {
                d.preview = Some((r.name.clone(), e.clone()));
            }
            if resp.lost_focus() {
                d.preview = None;
                if e != r.expression {
                    let res = app.run("parameters.change", json!({"name": r.name, "expression": e}));
                    d.error = res.err();
                    if d.error.is_none() {
                        d.editing.remove(&r.name);
                    }
                } else {
                    d.editing.remove(&r.name);
                }
            }
            // Value, or why it has none.
            match (&live, &r.error) {
                (Some(Err(m)), _) => ui.label(RichText::new(m).color(t.error)).on_hover_text(m),
                (Some(Ok(v)), _) => ui.label(RichText::new(v).color(t.accent)),
                (None, Some(m)) => ui.label(RichText::new(m).color(t.error)).on_hover_text(m),
                (None, None) => ui.label(RichText::new(show_value(r)).color(t.text_dim)),
            };
            // Comment.
            let mut c = d.comments.get(&r.name).cloned().unwrap_or_else(|| r.comment.clone());
            let cr = ui.add_sized([150.0, 20.0], egui::TextEdit::singleline(&mut c).hint_text("comment"));
            if cr.changed() {
                d.comments.insert(r.name.clone(), c.clone());
            }
            if cr.lost_focus() {
                if c != r.comment {
                    d.error = app.run("parameters.comment", json!({"name": r.name, "comment": c})).err();
                }
                d.comments.remove(&r.name);
            }
            // Delete (user parameters; refused while used, the message says by what).
            if r.source == "user" {
                if ui
                    .add(egui::Button::new(RichText::new("Delete").small().color(t.text_dim)).frame(false))
                    .on_hover_text("Delete this parameter")
                    .clicked()
                {
                    d.error = app.run("parameters.delete", json!({"name": r.name})).err();
                }
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
}

fn unit_label(u: &str) -> &str {
    if u.is_empty() { "none" } else { u }
}

fn add_row(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut ParamsDialog) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut d.new_name).hint_text("new parameter").desired_width(120.0));
        egui::ComboBox::from_id_salt("pnew_unit").width(56.0).selected_text(unit_label(UNITS.get(d.new_unit).copied().unwrap_or("mm"))).show_ui(
            ui,
            |ui| {
                for (i, u) in UNITS.iter().enumerate() {
                    ui.selectable_value(&mut d.new_unit, i, unit_label(u));
                }
            },
        );
        let unit = UNITS.get(d.new_unit).copied().unwrap_or("mm");
        let live = check(app, &d.new_expr, unit);
        let mut te = egui::TextEdit::singleline(&mut d.new_expr).desired_width(170.0);
        if live.is_err() {
            te = te.text_color(t.error);
        }
        let resp = ui.add(te);
        complete(ui, &resp, &mut d.new_expr);
        match &live {
            Ok(v) => ui.label(RichText::new(v).color(t.text_dim)),
            Err(m) => ui.label(RichText::new(m).color(t.error)),
        };
        ui.add(egui::TextEdit::singleline(&mut d.new_comment).hint_text("comment").desired_width(120.0));
        let ok = !d.new_name.trim().is_empty() && live.is_ok();
        if ui.add_enabled(ok, egui::Button::new("Add")).clicked() {
            let p = json!({"name": d.new_name.trim(), "expression": d.new_expr, "unit": unit, "comment": d.new_comment});
            match app.run("parameters.add", p) {
                Ok(_) => {
                    d.new_name.clear();
                    d.new_comment.clear();
                    d.error = None;
                }
                Err(e) => d.error = Some(e),
            }
        }
    });
}

/// A completion offered under an expression box.
#[derive(Clone, Debug)]
pub struct Suggestion {
    pub text: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Clone, Default)]
struct Cache {
    revision: u64,
    all: Arc<Vec<Suggestion>>,
}

fn cache_id() -> egui::Id {
    egui::Id::new("sc_param_completions")
}

/// Keep the completion list (parameters, functions, constants, units) for this revision of the
/// design where expression boxes can reach it. Called once per frame.
pub fn publish(app: &mut SolveApp, ctx: &egui::Context) {
    let rev = app.session.revision;
    if ctx.data(|d| d.get_temp::<Cache>(cache_id())).is_some_and(|c| c.revision == rev) {
        return;
    }
    let mut all: Vec<Suggestion> = Vec::new();
    // Every name (no prefix), then the units (offered only after a letter is typed).
    for prefix in ["", "m", "c", "i", "f", "d", "r", "u", "y", "k", "n"] {
        if let Ok(v) = app.session.execute("parameters.complete", &json!({ "prefix": prefix })) {
            for s in v["suggestions"].as_array().into_iter().flatten() {
                let text = s["text"].as_str().unwrap_or_default().to_string();
                if !text.is_empty() && !all.iter().any(|x| x.text == text) {
                    all.push(Suggestion {
                        text,
                        kind: s["kind"].as_str().unwrap_or_default().into(),
                        detail: s["detail"].as_str().unwrap_or_default().into(),
                    });
                }
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(cache_id(), Cache { revision: rev, all: Arc::new(all) }));
}

/// The identifier being typed before `cursor` (chars) and where it starts.
fn word_before(text: &str, cursor: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    let end = cursor.min(chars.len());
    let mut start = end;
    while start > 0 && chars.get(start - 1).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
        start -= 1;
    }
    (start, chars.get(start..end).map(|s| s.iter().collect()).unwrap_or_default())
}

/// Suggestions for the word being typed.
pub fn suggestions(ctx: &egui::Context, word: &str) -> Vec<Suggestion> {
    if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Vec::new();
    }
    let all = ctx.data(|d| d.get_temp::<Cache>(cache_id())).map(|c| c.all).unwrap_or_default();
    let w = word.to_ascii_lowercase();
    all.iter().filter(|s| s.text.to_ascii_lowercase().starts_with(&w) && s.text.trim_end_matches('(') != word).take(8).cloned().collect()
}

/// Offer completions under an expression box while it has focus; Tab or a click takes the first
/// (or the clicked) one. Returns true when the text changed.
pub fn complete(ui: &mut egui::Ui, resp: &egui::Response, text: &mut String) -> bool {
    let ctx = ui.ctx().clone();
    let state = egui::TextEdit::load_state(&ctx, resp.id);
    let cursor = state.as_ref().and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or_else(|| text.chars().count());
    let (start, word) = word_before(text, cursor);
    // Raw events: egui's focus navigation consumes Tab before widgets see it.
    let tab = ui.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Tab, pressed: true, .. })));
    // Tab moves focus away before we see it: accept on that frame and take focus back.
    // Keyboard focus in egui's memory (not `has_focus`, which also needs the OS window focused).
    let focused = ctx.memory(|m| m.has_focus(resp.id));
    let active = focused || (resp.lost_focus() && tab);
    if !active {
        return false;
    }
    let list = suggestions(&ctx, &word);
    if list.is_empty() {
        return false;
    }
    let mut pick: Option<String> = if tab { list.first().map(|s| s.text.clone()) } else { None };
    let t = Tokens::get();
    egui::Area::new(resp.id.with("completions")).order(egui::Order::Tooltip).fixed_pos(resp.rect.left_bottom() + egui::vec2(0.0, 2.0)).show(
        &ctx,
        |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(resp.rect.width().max(180.0));
                for (i, s) in list.iter().enumerate() {
                    let colour = match s.kind.as_str() {
                        "parameter" => t.text,
                        "function" => t.accent,
                        _ => t.text_dim,
                    };
                    let label = RichText::new(&s.text).color(colour);
                    let r = ui.add(egui::Button::new(label).frame(false).fill(Color32::TRANSPARENT)).on_hover_text(&s.detail);
                    if i == 0 {
                        ui.label(RichText::new(&s.detail).small().color(t.text_dim));
                    }
                    if r.clicked() {
                        pick = Some(s.text.clone());
                    }
                }
                ui.label(RichText::new("Tab to complete").small().color(t.text_dim));
            });
        },
    );
    let Some(p) = pick else { return false };
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars.get(..start).map(|s| s.iter().collect()).unwrap_or_default();
    let after: String = chars.get(cursor.min(chars.len())..).map(|s| s.iter().collect()).unwrap_or_default();
    *text = format!("{before}{p}{after}");
    let at = start + p.chars().count();
    let mut st = state.unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
    st.store(&ctx, resp.id);
    resp.request_focus();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_suggestions() {
        assert_eq!(word_before("2 * wid", 7), (4, "wid".to_string()));
        assert_eq!(word_before("width + 1", 5), (0, "width".to_string()));
        assert_eq!(word_before("1 + ", 4), (4, String::new()));
        assert_eq!(word_before("ü", 9), (0, "ü".to_string()));
        let ctx = egui::Context::default();
        let mut app = SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        let _ = app.run("parameters.add", json!({"name": "width", "expression": "40 mm"}));
        publish(&mut app, &ctx);
        let s = suggestions(&ctx, "wi");
        assert_eq!(s.first().map(|x| x.text.as_str()), Some("width"));
        assert!(suggestions(&ctx, "ata").iter().any(|x| x.text == "atan2("));
        assert!(suggestions(&ctx, "1").is_empty());
        // The dialog's rows come from the engine.
        let r = rows(&mut app);
        assert!(r.iter().any(|x| x.name == "width" && x.source == "user"));
        assert!(check(&mut app, "width / 2", "mm").is_ok());
        assert!(check(&mut app, "width + 1 deg", "mm").is_err());
    }
}
