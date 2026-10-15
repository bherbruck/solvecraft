//! The assistant's tools are the MCP tools: the same schemas, the same dispatch.

use serde_json::Value;

use crate::model::{Part, ToolDef};

/// The tools offered to the model: all of MCP's. Without `vision` the screenshot tool is left
/// out, as the model could not read its result.
pub fn definitions(vision: bool) -> Vec<ToolDef> {
    solidcraft_mcp::tool_definitions()
        .into_iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            if !vision && name == "screenshot" {
                return None;
            }
            Some(ToolDef {
                name,
                description: t.get("description")?.as_str()?.to_string(),
                schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
            })
        })
        .collect()
}

/// The parts and the error flag of an MCP tool result (`{"content": […], "isError": bool}`).
pub fn result_parts(v: &Value) -> (Vec<Part>, bool) {
    let is_error = v.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let parts = v
        .get("content")
        .and_then(Value::as_array)
        .map(|c| {
            c.iter()
                .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                    Some("text") => p.get("text").and_then(Value::as_str).map(|t| Part::Text(t.to_string())),
                    Some("image") => {
                        let data = p.get("data").and_then(Value::as_str)?;
                        Some(Part::Image {
                            mime: p.get("mimeType").and_then(Value::as_str).unwrap_or("image/png").to_string(),
                            data: data.to_string(),
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    (parts, is_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_tool_list_follows_mcp() {
        let all = definitions(true);
        let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        for want in ["list_commands", "execute", "batch", "inspect_design", "measure", "screenshot", "undo"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
        assert!(names.contains(&"crash_reports"));
        assert!(all.iter().all(|t| t.schema.get("type").and_then(Value::as_str) == Some("object") && !t.description.is_empty()));
        assert!(!definitions(false).iter().any(|t| t.name == "screenshot"));
    }

    #[test]
    fn results_split_into_text_and_pictures() {
        let (parts, err) = result_parts(
            &json!({"content": [{"type": "image", "data": "AAAA", "mimeType": "image/png"}, {"type": "text", "text": "hi"}, {"type": "resource"}, 5], "isError": true}),
        );
        assert!(err);
        assert_eq!(parts, vec![Part::Image { mime: "image/png".into(), data: "AAAA".into() }, Part::Text("hi".into())]);
        assert_eq!(result_parts(&json!(null)), (vec![], false));
    }
}
