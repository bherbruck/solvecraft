//! A sample design (used by `solvecraft --sample` and demos): a parametric plate with rounded
//! corners, two holes and a boss.

use serde_json::{Value, json};

pub fn script() -> Value {
    json!({"commands": [
        {"command": "ChangeParameterCommand", "params": {"name": "width", "expression": "80 mm", "comment": "plate width"}},
        {"command": "ChangeParameterCommand", "params": {"name": "depth", "expression": "50 mm"}},
        {"command": "ChangeParameterCommand", "params": {"name": "thickness", "expression": "8 mm"}},
        {"command": "SketchCreate", "params": {"plane": "XY", "name": "Base"}},
        {"command": "ShapeRectangleTwoPoint", "params": {"p0": [0, 0], "p1": [80, 50]}},
        {"command": "ConstraintCoincident", "params": {"a": "p1", "b": "origin"}},
        {"command": "SketchDimension", "params": {"entities": ["l1"], "value": "width"}},
        {"command": "SketchDimension", "params": {"entities": ["l2"], "value": "depth"}},
        {"command": "SketchStop", "params": {}},
        {"command": "Extrude", "params": {"distance": "thickness", "body_name": "Plate"}},
        {"command": "FusionFilletEdgesCommand", "params": {"edges": [[0, 0, 4], [80, 0, 4], [80, 50, 4], [0, 50, 4]], "radius": "6 mm"}},
        {"command": "SketchCreate", "params": {"plane": "XY", "name": "Holes"}},
        {"command": "CircleCenterRadius", "params": {"center": [12, 25], "diameter": 8}},
        {"command": "CircleCenterRadius", "params": {"center": [68, 25], "diameter": 8}},
        {"command": "SketchStop", "params": {}},
        {"command": "Extrude", "params": {"distance": "thickness", "operation": "cut", "name": "Holes"}},
        {"command": "SketchCreate", "params": {"plane": {"face": [40, 25, 8]}, "name": "Boss"}},
        {"command": "CircleCenterRadius", "params": {"center": [40, 25], "diameter": 24}},
        {"command": "SketchStop", "params": {}},
        {"command": "Extrude", "params": {"distance": 14, "operation": "join", "name": "Boss"}},
        {"command": "SketchCreate", "params": {"plane": {"face": [40, 37, 22]}, "name": "Bore"}},
        {"command": "CircleCenterRadius", "params": {"center": [40, 25], "diameter": 12}},
        {"command": "SketchStop", "params": {}},
        {"command": "Extrude", "params": {"distance": 22, "direction": "negative", "operation": "cut", "name": "Bore"}},
    ]})
}
