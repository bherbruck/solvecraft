//! Configurations: the design's configuration table (rows of parameter values and feature
//! suppressions) and switching between its rows.

use serde_json::{Value, json};
use solvecraft_doc::config::{Column, ConfigRow, MAX_COLUMNS, MAX_ROWS};

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("FusionStartDesignConfigModeCmd", "Configure", configure).at("SOLID", "CONFIGURE").icon("params").params(
        "params?: [parameter names]; features?: [feature names] (columns to add; with no rows yet, a \"Default\" row takes the current values)",
    ),
    CommandSpec::new("FusionShowDesignConfigPanelCmd", "Display Configuration Table", show)
        .at("SOLID", "CONFIGURE")
        .icon("params")
        .noundo()
        .params("→ columns (param:<name> or suppress:<feature>), rows, the active row and whether the design still matches it"),
    CommandSpec::new("config.column", "Configuration Column", column).params("param? | feature? (name); delete?: bool"),
    CommandSpec::new("config.row", "Configuration Row", row).params(
        "name; from? (row to copy; default the current values); values?: {param name or feature name: expression or true/false}; rename?; delete?: bool",
    ),
    CommandSpec::new("config.activate", "Activate Configuration", activate).params("row: name — sets its parameter values and suppressions"),
];

fn column_of(s: &Session, p: &Value, cmd: &str) -> Result<Column> {
    if let Some(n) = str_(p, "param") {
        if !s.doc.all_param_exprs().iter().any(|(m, _, _)| m == n) {
            return Err(bad(cmd, format!("no parameter `{n}`")));
        }
        return Ok(Column::Param(n.to_string()));
    }
    let f = str_(p, "feature").ok_or_else(|| bad(cmd, "give `param` or `feature`"))?;
    let id = s.doc.find_feature(f).map(|x| x.id).ok_or_else(|| bad(cmd, format!("no feature `{f}`")))?;
    Ok(Column::Suppress(id))
}

/// Add a column, filling existing rows with the design's current value.
fn add_column(s: &mut Session, c: Column, cmd: &str) -> Result<()> {
    if s.doc.configs.columns.contains(&c) {
        return Ok(());
    }
    if s.doc.configs.columns.len() >= MAX_COLUMNS {
        return Err(bad(cmd, "too many columns"));
    }
    let cur = s.doc.config_current(&c).unwrap_or_default();
    let t = &mut s.doc_mut().configs;
    t.columns.push(c);
    for r in &mut t.rows {
        r.cells.push(cur.clone());
    }
    Ok(())
}

fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionStartDesignConfigModeCmd";
    for n in string_list(p, "params") {
        let c = column_of(s, &json!({ "param": n }), cmd)?;
        add_column(s, c, cmd)?;
    }
    for n in string_list(p, "features") {
        let c = column_of(s, &json!({ "feature": n }), cmd)?;
        add_column(s, c, cmd)?;
    }
    if s.doc.configs.rows.is_empty() {
        let cells = s.doc.configs.columns.iter().map(|c| s.doc.config_current(c).unwrap_or_default()).collect();
        let t = &mut s.doc_mut().configs;
        t.rows.push(ConfigRow { name: "Default".into(), cells });
        t.active = Some("Default".into());
    }
    show(s, p)
}

fn show(s: &mut Session, _p: &Value) -> Result<Value> {
    let t = &s.doc.configs;
    let keys: Vec<String> = t.columns.iter().map(|c| s.doc.config_key(c)).collect();
    let rows: Vec<Value> = t
        .rows
        .iter()
        .map(|r| json!({"name": r.name, "values": keys.iter().zip(&r.cells).map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>()}))
        .collect();
    Ok(json!({"columns": keys, "rows": rows, "active": t.active, "matches": s.doc.configuration_matches()}))
}

fn column(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "config.column";
    let c = column_of(s, p, cmd)?;
    if bool_(p, "delete").unwrap_or(false) {
        let t = &mut s.doc_mut().configs;
        let i = t.columns.iter().position(|x| *x == c).ok_or_else(|| bad(cmd, "not a column of the table"))?;
        t.columns.remove(i);
        for r in &mut t.rows {
            if i < r.cells.len() {
                r.cells.remove(i);
            }
        }
    } else {
        add_column(s, c, cmd)?;
    }
    show(s, p)
}

fn row(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "config.row";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    let idx = s.doc.configs.rows.iter().position(|r| r.name == name);
    if bool_(p, "delete").unwrap_or(false) {
        let i = idx.ok_or_else(|| bad(cmd, format!("no row `{name}`")))?;
        let t = &mut s.doc_mut().configs;
        t.rows.remove(i);
        if t.active.as_deref() == Some(name.as_str()) {
            t.active = None;
        }
        return show(s, p);
    }
    let mut cells: Vec<String> = match (idx, str_(p, "from")) {
        (_, Some(f)) => s.doc.configs.rows.iter().find(|r| r.name == f).map(|r| r.cells.clone()).ok_or_else(|| bad(cmd, format!("no row `{f}`")))?,
        (Some(i), None) => s.doc.configs.rows.get(i).map(|r| r.cells.clone()).unwrap_or_default(),
        (None, None) => s.doc.configs.columns.iter().map(|c| s.doc.config_current(c).unwrap_or_default()).collect(),
    };
    if let Some(vals) = p.get("values") {
        let vals = vals.as_object().ok_or_else(|| bad(cmd, "`values` must map columns to values"))?;
        for (k, v) in vals {
            let text = match v {
                Value::String(t) => t.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => return Err(bad(cmd, format!("value for `{k}` must be an expression or true/false"))),
            };
            if text.len() > 4096 {
                return Err(bad(cmd, "value too long"));
            }
            let key = k.strip_prefix("param:").or_else(|| k.strip_prefix("suppress:")).unwrap_or(k);
            let col = s
                .doc
                .configs
                .columns
                .iter()
                .position(|c| match c {
                    Column::Param(n) => n == key,
                    Column::Suppress(id) => s.doc.feature(*id).is_some_and(|f| f.name == key),
                })
                .ok_or_else(|| bad(cmd, format!("`{k}` is not a column (config.column adds one)")))?;
            if let Some(cell) = cells.get_mut(col) {
                *cell = text;
            }
        }
    }
    let rename = str_(p, "rename").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).map(str::to_string);
    let t = &mut s.doc_mut().configs;
    match idx {
        Some(i) => {
            if let Some(r) = t.rows.get_mut(i) {
                r.cells = cells;
                if let Some(n) = rename {
                    if t.active.as_deref() == Some(r.name.as_str()) {
                        t.active = Some(n.clone());
                    }
                    r.name = n;
                }
            }
        }
        None => {
            if t.rows.len() >= MAX_ROWS {
                return Err(bad(cmd, "too many rows"));
            }
            t.rows.push(ConfigRow { name, cells });
        }
    }
    show(s, p)
}

fn activate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "config.activate";
    let r = str_(p, "row").ok_or_else(|| bad(cmd, "`row` is required"))?.to_string();
    s.doc_mut().apply_configuration(&r).map_err(|e| bad(cmd, e.to_string()))?;
    s.refresh();
    let errors: Vec<String> = s.model.results.iter().filter_map(|x| x.error.clone()).collect();
    let mut out = show(s, p)?;
    out["errors"] = json!(errors);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn volume(s: &mut Session) -> f64 {
        run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    }

    #[test]
    fn configurations_switch_sizes_and_suppressions() {
        let mut s = Session::default();
        run(&mut s, "ChangeParameterCommand", json!({"name": "len", "expression": "40 mm"}));
        run(&mut s, "PrimitiveBox", json!({"length": "len", "width": 20, "height": 10}));
        run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[0, 0, 5]], "radius": 3}));
        let fillet = s.doc.features.last().map(|f| f.name.clone()).unwrap_or_default();
        let t = run(&mut s, "FusionStartDesignConfigModeCmd", json!({"params": ["len"], "features": [fillet]}));
        assert_eq!(t["rows"][0]["name"], "Default");
        run(&mut s, "config.row", json!({"name": "Long", "values": {"len": "80 mm"}}));
        run(&mut s, "config.row", json!({"name": "Sharp", "from": "Long", "values": {fillet.clone(): true}}));
        let filleted = 8000.0 - (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 10.0;
        assert!((volume(&mut s) - filleted).abs() < 0.5);
        run(&mut s, "config.activate", json!({"row": "Long"}));
        assert!((volume(&mut s) - (16000.0 - (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 10.0)).abs() < 0.5, "{}", volume(&mut s));
        run(&mut s, "config.activate", json!({"row": "Sharp"}));
        assert!((volume(&mut s) - 16000.0).abs() < 1e-6);
        let t = run(&mut s, "FusionShowDesignConfigPanelCmd", json!({}));
        assert_eq!((t["active"].as_str(), t["matches"].as_bool()), (Some("Sharp"), Some(true)), "{t}");
        // Undo goes back to the previous configuration's values.
        run(&mut s, "UndoCommand", json!({}));
        assert!(volume(&mut s) > 15000.0 && volume(&mut s) < 16000.0);
        // Renaming the parameter keeps the column; editing it by hand breaks the match.
        run(&mut s, "parameters.rename", json!({"name": "len", "new_name": "length"}));
        let t = run(&mut s, "FusionShowDesignConfigPanelCmd", json!({}));
        assert_eq!(t["columns"][0], "param:length", "{t}");
        run(&mut s, "ChangeParameterCommand", json!({"name": "length", "expression": "50 mm"}));
        assert_eq!(run(&mut s, "FusionShowDesignConfigPanelCmd", json!({}))["matches"], false);
        // Bad input.
        assert!(s.execute("config.row", &json!({"name": "X", "values": {"nope": 1}})).is_err());
        assert!(s.execute("config.activate", &json!({"row": "Missing"})).is_err());
        run(&mut s, "config.row", json!({"name": "Broken", "values": {"length": "1/"}}));
        assert!(s.execute("config.activate", &json!({"row": "Broken"})).is_err());
        run(&mut s, "config.row", json!({"name": "Broken", "delete": true}));
        run(&mut s, "config.column", json!({"feature": fillet, "delete": true}));
        assert_eq!(run(&mut s, "FusionShowDesignConfigPanelCmd", json!({}))["columns"].as_array().map(Vec::len), Some(1));
    }
}
