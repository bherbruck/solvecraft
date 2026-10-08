//! STEP (ISO 10303-21) export: AP242 product structure, assemblies, names and colours.
//!
//! Each body's B-rep is written by truck's STEP writer (geometry and topology only), read back
//! with our Part 21 parser and copied into a file we assemble ourselves: one header and
//! context, a product per part with its shape representation, named solids, surface colours,
//! and assemblies as next-assembly-usage occurrences placed by item-defined transformations
//! (the structure the CAx-IF recommends and our reader follows). Identical geometric entities
//! (points, directions, placements, curves, surfaces) are written once and referenced; ids are
//! dense from `#1`.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::body::Body;
use crate::step_in::p21::{self, Param};
use crate::{KernelError, Result};

/// A named, optionally coloured body to export (coordinates in its part's frame, mm).
pub struct ExportBody<'a> {
    pub name: String,
    pub body: &'a Body,
    pub color: Option<[f32; 3]>,
}

/// A part or assembly: its own bodies and placed child products.
pub struct ExportProduct<'a> {
    pub name: String,
    pub bodies: Vec<ExportBody<'a>>,
    /// (child product index, rigid placement in this product, column-major 4×4, occurrence name).
    pub children: Vec<(usize, [[f64; 4]; 4], String)>,
}

/// Header fields.
#[derive(Clone, Debug, Default)]
pub struct StepHeader {
    /// FILE_NAME name (the file's own name).
    pub file_name: String,
    /// FILE_NAME time stamp (ISO 8601), empty when unknown.
    pub time_stamp: String,
    pub author: String,
    pub organization: String,
}

/// Entities whose identity is topological: never merged even when their text is equal.
const TOPOLOGY: &[&str] = &[
    "VERTEX_POINT",
    "EDGE_CURVE",
    "ORIENTED_EDGE",
    "EDGE_LOOP",
    "FACE_BOUND",
    "FACE_OUTER_BOUND",
    "FACE_SURFACE",
    "ADVANCED_FACE",
    "ORIENTED_FACE",
    "CLOSED_SHELL",
    "OPEN_SHELL",
    "ORIENTED_CLOSED_SHELL",
    "MANIFOLD_SOLID_BREP",
    "BREP_WITH_VOIDS",
];

fn real(x: f64) -> String {
    if !x.is_finite() {
        return "0.".into();
    }
    let s = format!("{x:?}");
    match s.split_once('e') {
        Some((m, e)) => {
            let m = if m.contains('.') { m.to_string() } else { format!("{m}.") };
            format!("{m}E{e}")
        }
        None if s.contains('.') => s,
        None => format!("{s}."),
    }
}

/// A STEP string literal (quotes doubled, non-ASCII as `\X2\` UTF-16 runs).
fn string(s: &str) -> String {
    let mut o = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => o.push_str("''"),
            '\\' => o.push_str("\\\\"),
            c if c.is_ascii() && !c.is_ascii_control() => o.push(c),
            c if c.is_ascii_control() => {}
            c => {
                o.push_str("\\X2\\");
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(o, "{u:04X}");
                }
                o.push_str("\\X0\\");
            }
        }
    }
    o.push('\'');
    o
}

fn param(p: &Param, map: &HashMap<u64, u64>, o: &mut String) -> std::result::Result<(), String> {
    match p {
        Param::Int(i) => {
            let _ = write!(o, "{i}");
        }
        Param::Real(x) => o.push_str(&real(*x)),
        Param::Str(s) => o.push_str(&string(s)),
        Param::Enum(e) => {
            let _ = write!(o, ".{e}.");
        }
        Param::Ref(r) => {
            let n = map.get(r).ok_or_else(|| format!("dangling reference #{r}"))?;
            let _ = write!(o, "#{n}");
        }
        Param::List(v) => {
            o.push('(');
            params(v, map, o)?;
            o.push(')');
        }
        Param::Typed(name, v) => {
            o.push_str(name);
            o.push('(');
            params(v, map, o)?;
            o.push(')');
        }
        Param::Binary(b) => {
            let _ = write!(o, "\"{b}\"");
        }
        Param::Null => o.push('$'),
        Param::Derived => o.push('*'),
    }
    Ok(())
}

fn params(v: &[Param], map: &HashMap<u64, u64>, o: &mut String) -> std::result::Result<(), String> {
    for (i, p) in v.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        param(p, map, o)?;
    }
    Ok(())
}

/// The DATA section under construction.
#[derive(Default)]
struct Out {
    lines: Vec<String>,
    /// Geometry text → id (shared entities).
    shared: HashMap<String, u64>,
}

impl Out {
    fn add(&mut self, text: impl Into<String>) -> u64 {
        self.lines.push(text.into());
        self.lines.len() as u64
    }
    /// Add, or reuse an identical earlier entity (geometry only).
    fn add_shared(&mut self, text: String) -> u64 {
        if let Some(id) = self.shared.get(&text) {
            return *id;
        }
        let id = self.add(text.clone());
        self.shared.insert(text, id);
        id
    }
    fn point(&mut self, p: [f64; 3]) -> u64 {
        self.add_shared(format!("CARTESIAN_POINT('',({},{},{}))", real(p[0]), real(p[1]), real(p[2])))
    }
    fn direction(&mut self, d: [f64; 3]) -> u64 {
        self.add_shared(format!("DIRECTION('',({},{},{}))", real(d[0]), real(d[1]), real(d[2])))
    }
    fn placement(&mut self, o: [f64; 3], z: [f64; 3], x: [f64; 3]) -> u64 {
        let (po, dz, dx) = (self.point(o), self.direction(z), self.direction(x));
        self.add_shared(format!("AXIS2_PLACEMENT_3D('',#{po},#{dz},#{dx})"))
    }

    /// Copy a solid entity of a parsed file and everything it uses; returns its new id.
    fn copy_solid(&mut self, ex: &p21::Exchange, root: u64, name: &str) -> std::result::Result<u64, String> {
        let mut map: HashMap<u64, u64> = HashMap::new();
        // Iterative post-order walk (children before parents).
        let mut stack: Vec<(u64, bool)> = vec![(root, false)];
        let mut guard = 0usize;
        while let Some((id, done)) = stack.pop() {
            guard += 1;
            if guard > 50_000_000 {
                return Err("solid too large".into());
            }
            if map.contains_key(&id) {
                continue;
            }
            let e = ex.get(id).ok_or_else(|| format!("missing #{id}"))?;
            if !done {
                stack.push((id, true));
                let mut refs = Vec::new();
                fn collect(p: &Param, out: &mut Vec<u64>) {
                    match p {
                        Param::Ref(r) => out.push(*r),
                        Param::List(v) | Param::Typed(_, v) => v.iter().for_each(|x| collect(x, out)),
                        _ => {}
                    }
                }
                e.records.iter().flat_map(|r| &r.params).for_each(|p| collect(p, &mut refs));
                stack.extend(refs.into_iter().filter(|r| !map.contains_key(r)).map(|r| (r, false)));
                continue;
            }
            let mut text = String::new();
            let is_root = id == root;
            if e.is_complex() {
                text.push('(');
                for r in &e.records {
                    text.push_str(&r.name);
                    text.push('(');
                    params(&r.params, &map, &mut text)?;
                    text.push(')');
                }
                text.push(')');
            } else {
                // Advanced breps use advanced faces (same attributes as truck's face surfaces).
                let ename = if e.name() == "FACE_SURFACE" { "ADVANCED_FACE" } else { e.name() };
                text.push_str(ename);
                text.push('(');
                let mut ps = e.params().to_vec();
                if is_root && let Some(first) = ps.first_mut() {
                    *first = Param::Str(name.to_string());
                }
                params(&ps, &map, &mut text)?;
                text.push(')');
            }
            let topo = TOPOLOGY.contains(&e.name());
            let new = if topo { self.add(text) } else { self.add_shared(text) };
            map.insert(id, new);
        }
        map.get(&root).copied().ok_or_else(|| "solid not copied".into())
    }
}

/// The B-rep entities of one body as truck writes them, parsed.
fn brep_exchange(b: &Body) -> Result<p21::Exchange> {
    let text = crate::step::truck_step(&[b], "SolveCraft")?;
    p21::parse(&text).map_err(|e| KernelError::Failed(format!("STEP export: {e}")))
}

fn rigid_frame(m: &[[f64; 4]; 4]) -> Result<([f64; 3], [f64; 3], [f64; 3])> {
    let x = [m[0][0], m[0][1], m[0][2]];
    let y = [m[1][0], m[1][1], m[1][2]];
    let z = [m[2][0], m[2][1], m[2][2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let ok = (dot(x, x) - 1.0).abs() < 1e-6 && (dot(y, y) - 1.0).abs() < 1e-6 && dot(x, y).abs() < 1e-6 && dot(cross, z) > 0.999_999;
    if !ok || m.iter().flatten().any(|v| !v.is_finite()) {
        return Err(KernelError::Invalid("component placements must be rigid (rotation and translation) to export as STEP".into()));
    }
    Ok(([m[3][0], m[3][1], m[3][2]], z, x))
}

/// STEP text (AP242) for products with bodies and assembly placements; `root` is the top product.
pub fn step_export_products(products: &[ExportProduct], root: usize, header: &StepHeader) -> Result<String> {
    if products.is_empty() || root >= products.len() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    if products.iter().all(|p| p.bodies.is_empty()) {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    for p in products {
        for b in &p.bodies {
            b.body.require_brep("STEP export")?;
        }
        if p.children.iter().any(|(c, _, _)| *c >= products.len()) {
            return Err(KernelError::Invalid("bad component reference".into()));
        }
    }
    // No product may contain itself.
    fn cyclic(products: &[ExportProduct], p: usize, stack: &mut Vec<usize>) -> bool {
        if stack.contains(&p) || stack.len() > 64 {
            return true;
        }
        stack.push(p);
        let bad = products.get(p).is_some_and(|x| x.children.iter().any(|(c, _, _)| cyclic(products, *c, stack)));
        stack.pop();
        bad
    }
    if cyclic(products, root, &mut Vec::new()) {
        return Err(KernelError::Invalid("the component tree is recursive".into()));
    }

    let mut o = Out::default();
    let app = o.add("APPLICATION_CONTEXT('managed model based 3d engineering')");
    o.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','ap242_managed_model_based_3d_engineering',2014,#{app})"));
    let pctx = o.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let pdctx = o.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let mm = o.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let rad = o.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
    let sr = o.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
    let unc = o.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{mm},'distance_accuracy_value','confusion accuracy')"));
    let ctx = o.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    let origin = o.placement([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);

    // Products (only those reachable from the root).
    let mut reach = vec![false; products.len()];
    let mut todo = vec![root];
    while let Some(p) = todo.pop() {
        if let Some(r) = reach.get_mut(p)
            && !*r
        {
            *r = true;
            if let Some(x) = products.get(p) {
                todo.extend(x.children.iter().map(|(c, _, _)| *c));
            }
        }
    }
    let mut pd = vec![0u64; products.len()];
    let mut shape_rep = vec![0u64; products.len()];
    let mut product_ids = Vec::new();
    let mut styled = Vec::new();
    let mut child_items: Vec<Vec<u64>> = vec![Vec::new(); products.len()];
    // Placements of children inside each parent, made before the parent's representation.
    let mut placements: Vec<Vec<u64>> = vec![Vec::new(); products.len()];
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        for (_, m, _) in &p.children {
            let (t, z, x) = rigid_frame(m)?;
            let (pt, dz, dx) = (o.point(t), o.direction(z), o.direction(x));
            // Its own entity: it is an item of the parent's representation.
            let a = o.add(format!("AXIS2_PLACEMENT_3D('',#{pt},#{dz},#{dx})"));
            if let Some(v) = placements.get_mut(pi) {
                v.push(a);
            }
            if let Some(v) = child_items.get_mut(pi) {
                v.push(a);
            }
        }
    }
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        let name = if p.name.trim().is_empty() { "Part".to_string() } else { p.name.clone() };
        let prod = o.add(format!("PRODUCT({},{},'',(#{pctx}))", string(&name), string(&name)));
        product_ids.push(prod);
        let pdf = o.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
        let d = o.add(format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdctx})"));
        let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{d})"));
        let mut items = vec![origin];
        items.extend(child_items.get(pi).cloned().unwrap_or_default());
        let item_list = items.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        let srep = o.add(format!("SHAPE_REPRESENTATION({},({item_list}),#{ctx})", string(&name)));
        o.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{srep})"));
        if let Some(slot) = pd.get_mut(pi) {
            *slot = d;
        }
        if let Some(slot) = shape_rep.get_mut(pi) {
            *slot = srep;
        }
        if !p.bodies.is_empty() {
            let mut solids = Vec::new();
            for b in &p.bodies {
                let ex = brep_exchange(b.body)?;
                let mut roots: Vec<u64> =
                    ex.entities.iter().filter(|(_, e)| matches!(e.name(), "MANIFOLD_SOLID_BREP" | "BREP_WITH_VOIDS")).map(|(i, _)| *i).collect();
                roots.sort();
                for r in roots {
                    let id = o.copy_solid(&ex, r, &b.name).map_err(|e| KernelError::Failed(format!("STEP export: {e}")))?;
                    solids.push(id);
                    if let Some(c) = b.color {
                        styled.push(style(&mut o, id, c));
                    }
                }
            }
            let list = solids.iter().chain(std::iter::once(&origin)).map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
            let absr = o.add(format!("ADVANCED_BREP_SHAPE_REPRESENTATION({},({list}),#{ctx})", string(&name)));
            o.add(format!("SHAPE_REPRESENTATION_RELATIONSHIP('','',#{srep},#{absr})"));
        }
    }
    if !product_ids.is_empty() {
        let list = product_ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        o.add(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part','',({list}))"));
    }
    // Assembly occurrences.
    for (pi, p) in products.iter().enumerate() {
        if !reach.get(pi).copied().unwrap_or(false) {
            continue;
        }
        for (k, (ci, _, oname)) in p.children.iter().enumerate() {
            let (Some(&parent_pd), Some(&child_pd), Some(&parent_sr), Some(&child_sr), Some(&target)) =
                (pd.get(pi), pd.get(*ci), shape_rep.get(pi), shape_rep.get(*ci), placements.get(pi).and_then(|v| v.get(k)))
            else {
                continue;
            };
            let nm = if oname.trim().is_empty() {
                format!("{}:{}", products.get(*ci).map(|c| c.name.as_str()).unwrap_or(""), k + 1)
            } else {
                oname.clone()
            };
            let nauo = o.add(format!("NEXT_ASSEMBLY_USAGE_OCCURRENCE({},{},'',#{parent_pd},#{child_pd},$)", string(&nm), string(&nm)));
            let pds = o.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{nauo})"));
            let idt = o.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{origin},#{target})"));
            let rr = o.add(format!(
                "(REPRESENTATION_RELATIONSHIP('','',#{child_sr},#{parent_sr})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{idt})SHAPE_REPRESENTATION_RELATIONSHIP())"
            ));
            o.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{rr},#{pds})"));
        }
    }
    if !styled.is_empty() {
        let list = styled.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(",");
        o.add(format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',({list}),#{ctx})"));
    }

    let mut s = String::new();
    s.push_str("ISO-10303-21;\nHEADER;\n");
    s.push_str("FILE_DESCRIPTION(('SolveCraft model'),'2;1');\n");
    let _ = writeln!(
        s,
        "FILE_NAME({},{},({}),({}),'SolveCraft','SolveCraft','');",
        string(&header.file_name),
        string(&header.time_stamp),
        string(&header.author),
        string(&header.organization)
    );
    s.push_str("FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\nENDSEC;\nDATA;\n");
    for (i, l) in o.lines.iter().enumerate() {
        let _ = writeln!(s, "#{}={l};", i + 1);
    }
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(s)
}

/// A surface colour for a solid (STYLED_ITEM chain).
fn style(o: &mut Out, item: u64, c: [f32; 3]) -> u64 {
    let rgb = o.add_shared(format!("COLOUR_RGB('',{},{},{})", real(c[0] as f64), real(c[1] as f64), real(c[2] as f64)));
    let fasc = o.add_shared(format!("FILL_AREA_STYLE_COLOUR('',#{rgb})"));
    let fas = o.add_shared(format!("FILL_AREA_STYLE('',(#{fasc}))"));
    let ssfa = o.add_shared(format!("SURFACE_STYLE_FILL_AREA(#{fas})"));
    let sss = o.add_shared(format!("SURFACE_SIDE_STYLE('',(#{ssfa}))"));
    let ssu = o.add_shared(format!("SURFACE_STYLE_USAGE(.BOTH.,#{sss})"));
    let psa = o.add_shared(format!("PRESENTATION_STYLE_ASSIGNMENT((#{ssu}))"));
    o.add(format!("STYLED_ITEM('color',(#{psa}),#{item})"))
}

/// STEP text for bodies of one part (names `Body1`…).
pub fn step_export_bodies(bodies: &[ExportBody], header: &StepHeader) -> Result<String> {
    let name = if header.file_name.is_empty() {
        "Part".to_string()
    } else {
        header.file_name.trim_end_matches(".step").trim_end_matches(".stp").to_string()
    };
    let p = ExportProduct {
        name,
        bodies: bodies.iter().map(|b| ExportBody { name: b.name.clone(), body: b.body, color: b.color }).collect(),
        children: Vec::new(),
    };
    step_export_products(&[p], 0, header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_strings() {
        assert_eq!(real(1.0), "1.0");
        assert_eq!(real(1e-6), "1.E-6");
        assert_eq!(real(-2.5e20), "-2.5E20");
        assert_eq!(real(3.0e100), "3.E100");
        assert_eq!(string("it's"), "'it''s'");
        assert_eq!(string("é"), "'\\X2\\00E9\\X0\\'");
        let ex = p21::parse(&format!("ISO-10303-21;DATA;#1=A({},{});ENDSEC;END-ISO-10303-21;", string("a'b\\é"), real(1e-6))).unwrap();
        let p = ex.get(1).unwrap().params();
        assert_eq!(p[0].as_str(), Some("a'b\\é"));
        assert_eq!(p[1].as_f64(), Some(1e-6));
    }

    #[test]
    fn assembly_names_colours_round_trip() {
        use crate::{box_solid, cylinder, measure, step_import, step_validate};
        let bx = box_solid(solvecraft_geom::Vec3::ZERO, solvecraft_geom::Vec3::new(10.0, 20.0, 5.0)).unwrap();
        let pin = cylinder(solvecraft_geom::Vec3::ZERO, solvecraft_geom::Vec3::Z, 2.0, 8.0).unwrap();
        let rot90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, 0.0, 0.0, 1.0]];
        let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 5.0, 1.0]];
        let products = vec![
            ExportProduct {
                name: "Root Ä".into(),
                bodies: vec![ExportBody { name: "Base".into(), body: &bx, color: Some([1.0, 0.0, 0.0]) }],
                children: vec![(1, shift, "Pin:1".into()), (1, rot90, "Pin:2".into())],
            },
            ExportProduct { name: "Pin".into(), bodies: vec![ExportBody { name: "Shaft".into(), body: &pin, color: None }], children: vec![] },
        ];
        let text = step_export_products(&products, 0, &StepHeader { file_name: "a.step".into(), ..Default::default() }).unwrap();
        step_validate(&text).unwrap();
        assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
        assert_eq!(text.matches("NEXT_ASSEMBLY_USAGE_OCCURRENCE").count(), 2);
        // Shared geometry: the unit Z direction is written once.
        assert_eq!(text.matches("DIRECTION('',(0.0,0.0,1.0))").count(), 1);
        let imp = step_import(&text).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        assert_eq!(imp.bodies.len(), 3);
        let names: Vec<&str> = imp.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Base", "Shaft", "Shaft"]);
        assert_eq!(imp.bodies[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(imp.tree[0].name, "Root Ä");
        assert_eq!(imp.tree[0].children.len(), 2);
        let m: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
        assert!((m[1].bbox.min.z - 5.0).abs() < 1e-6 && (m[1].bbox.max.z - 13.0).abs() < 1e-6);
        // Rotated 90° about Z and moved 50 along X: centre at (50, 0).
        assert!(m[2].centroid.dist(solvecraft_geom::Vec3::new(50.0, 0.0, 4.0)) < 1e-3, "{:?}", m[2].centroid);
        // Recursion and bad placements are errors.
        let cyc = vec![ExportProduct {
            name: "A".into(),
            bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }],
            children: vec![(0, shift, String::new())],
        }];
        assert!(step_export_products(&cyc, 0, &StepHeader::default()).is_err());
        let scaled = [[2.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]];
        let bad = vec![
            ExportProduct { name: "A".into(), bodies: vec![], children: vec![(1, scaled, String::new())] },
            ExportProduct { name: "B".into(), bodies: vec![ExportBody { name: "B".into(), body: &bx, color: None }], children: vec![] },
        ];
        assert!(step_export_products(&bad, 0, &StepHeader::default()).is_err());
    }
}
