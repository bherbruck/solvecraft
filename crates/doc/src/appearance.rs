//! Appearances: how bodies, faces and components look (colour and opacity, with a name), kept
//! in the design. A face's appearance wins over its body's, a body's over its component's, and
//! a component's over the physical material's look; a body with none of these keeps the colour
//! it was imported with.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;

use crate::Document;

/// A look: a name (a library entry's, or "Custom"), sRGB colour and opacity (1 = opaque).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub name: String,
    pub color: [u8; 3],
    #[serde(default = "opaque")]
    pub opacity: f64,
}

fn opaque() -> f64 {
    1.0
}

impl Look {
    pub fn custom(color: [u8; 3], opacity: f64) -> Look {
        Look { name: "Custom".into(), color, opacity: opacity.clamp(0.0, 1.0) }
    }
    /// Colour as 0–1 fractions.
    pub fn rgb(&self) -> [f32; 3] {
        self.color.map(|x| x as f32 / 255.0)
    }
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.color[0], self.color[1], self.color[2])
    }
}

/// A face's appearance: the face of `body` through `point` (world, on the face).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLook {
    pub body: String,
    pub point: Vec3,
    pub look: Look,
    /// The face's persistent name (found first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The design's appearances.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearances {
    /// By body name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, Look>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceLook>,
    /// By component id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<u64, Look>,
}

impl Appearances {
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.faces.is_empty() && self.components.is_empty()
    }
}

/// The built-in appearance library: (name, colour, opacity).
pub const LIBRARY: [(&str, [u8; 3], f64); 20] = [
    ("Aluminum - Satin", [204, 208, 214], 1.0),
    ("Aluminum - Anodized Black", [44, 46, 50], 1.0),
    ("Aluminum - Anodized Blue", [52, 92, 160], 1.0),
    ("Steel - Satin", [158, 164, 172], 1.0),
    ("Stainless Steel - Brushed", [186, 191, 198], 1.0),
    ("Cast Iron", [104, 104, 110], 1.0),
    ("Brass - Polished", [208, 172, 88], 1.0),
    ("Copper", [204, 122, 80], 1.0),
    ("Titanium", [150, 148, 158], 1.0),
    ("ABS - White", [236, 236, 230], 1.0),
    ("ABS - Black", [36, 36, 38], 1.0),
    ("ABS - Red", [196, 40, 40], 1.0),
    ("ABS - Blue", [40, 90, 186], 1.0),
    ("ABS - Yellow", [236, 196, 40], 1.0),
    ("ABS - Green", [52, 150, 80], 1.0),
    ("Rubber - Black", [28, 28, 28], 1.0),
    ("Glass - Clear", [190, 220, 228], 0.25),
    ("Glass - Smoked", [90, 96, 104], 0.45),
    ("Wood - Oak", [190, 146, 92], 1.0),
    ("Paint - Gloss White", [246, 246, 244], 1.0),
];

/// A library appearance by name (case-insensitive).
pub fn library(name: &str) -> Option<Look> {
    LIBRARY.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim())).map(|(n, c, o)| Look { name: n.to_string(), color: *c, opacity: *o })
}

/// The look a physical material gives a body that has no appearance of its own.
pub fn material_look(material: &str) -> Option<Look> {
    let color = crate::material_color(material)?;
    let opacity = if material == "Glass" { 0.25 } else { 1.0 };
    Some(Look { name: material.to_string(), color, opacity })
}

impl Document {
    /// A body's look (without face appearances): its own, else its component's, else its
    /// material's.
    pub fn body_look(&self, name: &str, feature: u64) -> Option<Look> {
        self.appearances
            .bodies
            .get(name)
            .cloned()
            .or_else(|| {
                let c = self.body_component(name, feature);
                (c != 0).then(|| self.appearances.components.get(&c).cloned()).flatten()
            })
            .or_else(|| self.materials.get(name).and_then(|m| material_look(m)))
    }

    /// Face appearances on a body: (point on the face, look).
    pub fn face_looks(&self, body: &str) -> Vec<(Vec3, Look)> {
        self.appearances.faces.iter().filter(|f| f.body == body).map(|f| (f.point, f.look.clone())).collect()
    }

    /// The faces of a body with appearances of their own: (B-rep face index, look). A later
    /// assignment to the same face wins.
    pub fn face_colors(&self, b: &crate::ModelBody) -> Vec<(usize, Look)> {
        // Face colours the body was imported with, under the design's own.
        let mut out: Vec<(usize, Look)> = b
            .body
            .paint()
            .map(|p| {
                let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
                p.faces.iter().map(|f| (f.face, Look::custom(f.color.map(to8), f.opacity as f64))).collect()
            })
            .unwrap_or_default();
        let names = crate::naming::face_names(b);
        for f in self.appearances.faces.iter().filter(|f| f.body == b.name) {
            let (p, look) = (f.point, f.look.clone());
            // By name (every piece of a split face), else by the point.
            if let Some(n) = &f.name {
                let stem = crate::naming::strip_piece(n);
                let hits: Vec<usize> = (0..names.len()).filter(|i| names.get(*i).is_some_and(|x| crate::naming::strip_piece(x) == stem)).collect();
                if !hits.is_empty() {
                    for i in hits {
                        out.retain(|(j, _)| *j != i);
                        out.push((i, look.clone()));
                    }
                    continue;
                }
            }
            if let Some(i) = face_index_at(b, p) {
                out.retain(|(j, _)| *j != i);
                out.push((i, look));
            }
        }
        out
    }
}

/// The B-rep face of a body nearest a point (within 5% of the body's size).
pub fn face_index_at(b: &crate::ModelBody, p: Vec3) -> Option<usize> {
    let m = b.mesh();
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, bb, c]) = m.tri(t) else { continue };
        let d = closest_on_triangle(p, a, bb, c).dist(p);
        if best.is_none_or(|(x, _)| d < x) {
            best = Some((d, *f));
        }
    }
    let (d, f) = best?;
    (d <= b.body.size() * 0.05 + 1e-6).then_some(f as usize)
}

/// The point of triangle abc nearest p.
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_and_materials() {
        assert_eq!(library("abs - red").map(|l| l.color), Some([196, 40, 40]));
        assert!(library("Glass - Clear").is_some_and(|l| l.opacity < 0.5));
        assert!(library("nope").is_none());
        assert_eq!(material_look("Brass").map(|l| l.color), crate::material_color("Brass"));
        assert!(material_look("Glass").is_some_and(|l| l.opacity < 1.0));
        assert_eq!(Look::custom([1, 2, 3], 7.0).opacity, 1.0);
        assert_eq!(Look::custom([255, 0, 16], 1.0).hex(), "#ff0010");
    }
}
