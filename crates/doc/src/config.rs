//! Configurations: a table whose rows are variants of the design. Each column is a parameter
//! (its expression per row) or a feature (suppressed or not per row); activating a row sets
//! those values in the design, which then rebuilds as usual.

use serde::{Deserialize, Serialize};

use crate::{DocError, Document, Result};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    /// A parameter's expression.
    Param(String),
    /// A feature's suppression (by feature id).
    Suppress(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigRow {
    pub name: String,
    /// One cell per column: an expression, or "true"/"false" for suppression.
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigTable {
    pub columns: Vec<Column>,
    pub rows: Vec<ConfigRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

pub const MAX_COLUMNS: usize = 500;
pub const MAX_ROWS: usize = 2000;

impl ConfigTable {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.rows.is_empty()
    }
}

impl Document {
    /// A column's current value in the design.
    pub fn config_current(&self, c: &Column) -> Option<String> {
        match c {
            Column::Param(n) => self.all_param_exprs().into_iter().find(|(m, _, _)| m == n).map(|(_, e, _)| e),
            Column::Suppress(id) => self.feature(*id).map(|f| f.suppressed.to_string()),
        }
    }

    /// A column's display key: `param:<name>` or `suppress:<feature name>`.
    pub fn config_key(&self, c: &Column) -> String {
        match c {
            Column::Param(n) => format!("param:{n}"),
            Column::Suppress(id) => format!("suppress:{}", self.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string())),
        }
    }

    /// Set the design to a row's values (all or nothing).
    pub fn apply_configuration(&mut self, row: &str) -> Result<()> {
        let r = self.configs.rows.iter().find(|r| r.name == row).cloned().ok_or_else(|| DocError::Unknown(format!("configuration `{row}`")))?;
        let mut next = self.clone();
        for (c, v) in self.configs.columns.iter().zip(&r.cells) {
            match c {
                Column::Param(n) => {
                    if next.config_current(c).as_deref() != Some(v.as_str()) {
                        next.change_param(n, v, None, None).map_err(|e| DocError::Invalid(format!("configuration `{row}`, parameter `{n}`: {e}")))?;
                    }
                }
                Column::Suppress(id) => {
                    let on = match v.trim() {
                        "true" | "1" | "yes" => true,
                        "false" | "0" | "no" => false,
                        o => return Err(DocError::Invalid(format!("configuration `{row}`: suppression must be true or false, not `{o}`"))),
                    };
                    let f = next.feature_mut(*id).ok_or_else(|| DocError::Unknown(format!("configuration `{row}`: feature {id} is gone")))?;
                    f.suppressed = on;
                }
            }
        }
        next.configs.active = Some(row.to_string());
        *self = next;
        Ok(())
    }

    /// The active row's values all hold in the design.
    pub fn configuration_matches(&self) -> bool {
        let Some(r) = self.configs.active.as_ref().and_then(|a| self.configs.rows.iter().find(|r| r.name == *a)) else { return false };
        self.configs.columns.iter().zip(&r.cells).all(|(c, v)| match c {
            Column::Suppress(_) => self.config_current(c).as_deref() == Some(v.trim()),
            Column::Param(_) => self.config_current(c).is_some_and(|x| x.replace(' ', "") == v.replace(' ', "")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_switch_parameters() {
        let mut d = Document::new("A");
        d.set_param("w", "10 mm", None, None).unwrap();
        d.configs.columns.push(Column::Param("w".into()));
        d.configs.rows.push(ConfigRow { name: "Small".into(), cells: vec!["10 mm".into()] });
        d.configs.rows.push(ConfigRow { name: "Large".into(), cells: vec!["40 mm".into()] });
        d.apply_configuration("Large").unwrap();
        assert_eq!(d.config_current(&Column::Param("w".into())).as_deref(), Some("40 mm"));
        assert!(d.configuration_matches());
        d.set_param("w", "12 mm", None, None).unwrap();
        assert!(!d.configuration_matches());
        d.configs.rows.push(ConfigRow { name: "Bad".into(), cells: vec!["1/".into()] });
        let before = d.clone();
        assert!(d.apply_configuration("Bad").is_err());
        assert_eq!(d, before, "all or nothing");
    }
}
