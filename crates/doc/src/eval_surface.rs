//! Evaluation of the surface features: Patch, Stitch, Thicken and Trim (kernel `surfaces`).

use super::*;

pub(super) fn eval(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    match &f.kind {
        FeatureKind::Patch { sketch, profiles, body, edges } => {
            let mut made = Vec::new();
            if let Some(sk) = sketch {
                let ss = st.sketch(*sk).ok_or_else(|| DocError::Unknown(format!("sketch {sk} (it must come earlier in the timeline)")))?;
                let regions = solvecraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
                for r in &regions {
                    made.push(kernel::patch_region(&ss.plane, r)?);
                }
            }
            if !edges.is_empty() {
                let i = body_at(st, body, edges)?;
                let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid("body".into()))?;
                made.push(kernel::patch_edges(&b.body, edges)?);
            }
            if made.is_empty() {
                return Err(DocError::Invalid("a patch needs a sketch region or a loop of edges".into()));
            }
            for (k, b) in made.into_iter().enumerate() {
                let name = new_name(st, f, k);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Stitch { bodies, tolerance } => {
            let tol = val(vals, tolerance, Kind::Length)?;
            let idx: Vec<usize> = bodies
                .iter()
                .map(|n| st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`"))))
                .collect::<Result<_>>()?;
            let Some(&first) = idx.first() else { return Err(DocError::Invalid("pick the surfaces to stitch".into())) };
            let refs: Vec<&kernel::Body> = idx.iter().filter_map(|i| st.bodies.get(*i).map(|b| &b.body)).collect();
            let sewn = kernel::stitch(&refs, tol)?;
            let Some(mb) = st.bodies.get(first).cloned() else { return Err(DocError::Invalid("body".into())) };
            if let Some(slot) = st.bodies.get_mut(first) {
                *slot = ModelBody::new(mb.name, sewn, mb.feature);
            }
            // The others are now part of the first.
            let gone: Vec<String> = idx.iter().skip(1).filter_map(|i| st.bodies.get(*i).map(|b| b.name.clone())).collect();
            st.bodies.retain(|b| !gone.contains(&b.name));
            Ok(())
        }
        FeatureKind::Thicken { bodies, thickness, symmetric, operation, targets } => {
            let t = val(vals, thickness, Kind::Length)?;
            let mut tools = Vec::new();
            for n in bodies {
                let b = st.body(n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                tools.push(kernel::thicken(&b.body, t, *symmetric)?);
            }
            if tools.is_empty() {
                return Err(DocError::Invalid("pick the surfaces to thicken".into()));
            }
            apply_op(st, f, tools, *operation, targets)
        }
        FeatureKind::SurfaceTrim { body, plane, tool, keep } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let trimmed = match tool {
                Some(t) => {
                    let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                    let face = crate::appearance::face_index_at(tb, t.point)
                        .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the tool point", t.body)))?;
                    kernel::trim(&mb.body, &kernel::SplitTool::Face { body: &tb.body, face }, *keep)?
                }
                None => kernel::trim(&mb.body, &kernel::SplitTool::Plane(doc.resolve_plane(vals, plane, 0)?), *keep)?,
            };
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::new(mb.name, trimmed, mb.feature);
            }
            Ok(())
        }
        _ => Err(DocError::Invalid("not a surface feature".into())),
    }
}
