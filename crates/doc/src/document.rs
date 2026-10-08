use serde::{Deserialize, Serialize};
use solvecraft_geom::{Plane, Vec2, Vec3};
use solvecraft_sketch::Sketch;

use crate::expr::{self, Kind};
use crate::{DocError, Result};

pub const MAX_FEATURES: usize = 10_000;
pub const MAX_PARAMS: usize = 10_000;

/// A named parameter. User parameters are created by the user; model parameters (`d1`, `d2`…)
/// are created by dimensions and features.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub expr: String,
    /// `mm`, `deg` or empty (unit-less).
    pub unit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Created by a dimension or feature (not by the user).
    #[serde(default)]
    pub model: bool,
}

impl Param {
    pub fn kind(&self) -> Kind {
        Kind::from_unit(&self.unit)
    }
}

/// Where a sketch lies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaneRef {
    /// `XY`, `XZ` or `YZ`.
    Origin { name: String },
    /// An explicit plane (e.g. picked from a planar face).
    Custom { plane: Plane },
    /// Another plane offset along its normal by an expression.
    Offset { base: Box<PlaneRef>, distance: String },
    /// Another plane rotated about a line (in world coordinates) by an angle expression.
    AtAngle { base: Box<PlaneRef>, axis_origin: Vec3, axis_dir: Vec3, angle: String },
    /// A construction plane feature (by name).
    Construction { name: String },
    /// A planar body face: found again on each evaluation (the face with this plane's normal
    /// nearest `at`), so sketches on it follow when earlier features change; `plane` is the
    /// plane as picked (its frame, and the fallback when the face is gone). `name` is the face's
    /// persistent name (found first; see `naming`).
    Face {
        plane: Plane,
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// Which closed profiles of a sketch a feature uses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSel {
    /// Every profile.
    All,
    /// Profiles by index in the solved sketch's profile list.
    Indices { indices: Vec<usize> },
    /// Profiles whose outer loop is made of exactly these curves (order-insensitive); stable
    /// across parameter changes.
    Curves { loops: Vec<Vec<String>> },
    /// The profile containing each sketch point.
    Points { points: Vec<Vec2> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    #[default]
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl Operation {
    pub fn parse(s: &str) -> Option<Operation> {
        Some(match s.to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
            "new" | "newbody" | "newcomponent" => Operation::NewBody,
            "join" | "union" | "add" => Operation::Join,
            "cut" | "subtract" | "difference" => Operation::Cut,
            "intersect" | "intersection" => Operation::Intersect,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Along the sketch normal.
    #[default]
    Positive,
    Negative,
    /// Both sides; `distance` per side.
    Symmetric,
}

/// Extent of an extrude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    pub distance: String,
    #[serde(default)]
    pub direction: Direction,
    /// Optional second side distance (two-sided extrude), along the negative normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<String>,
    /// Start offset from the sketch plane (expression).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<String>,
    /// Through all bodies in the direction(s) instead of `distance`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub through_all: bool,
    /// Taper angle (expression; positive grows the profile).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<String>,
    /// To Object: end at the face (or vertex/point) at this point, instead of `distance`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec3>,
    /// Offset past the To Object face (expression; negative stops short).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_offset: Option<String>,
    /// From Object: start at the face (or point) here instead of the sketch plane (plus
    /// `start_offset`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Vec3>,
}

/// Revolve axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AxisRef {
    /// A line of the profile's sketch.
    SketchLine { curve: String },
    /// Sketch x or y axis through the sketch origin (`x` / `y`).
    SketchAxis { axis: String },
    /// World X / Y / Z axis through the origin.
    World { axis: String },
    /// Any line in the sketch plane, in world coordinates.
    Line { origin: Vec3, dir: Vec3 },
}

/// Feature definitions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureKind {
    Sketch {
        plane: PlaneRef,
        sketch: Sketch,
    },
    Extrude {
        /// Feature id of the sketch.
        sketch: u64,
        profiles: ProfileSel,
        extent: Extent,
        #[serde(default)]
        operation: Operation,
        /// Target body names for join/cut/intersect (empty = all bodies).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    Revolve {
        sketch: u64,
        profiles: ProfileSel,
        axis: AxisRef,
        angle: String,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
        /// Two Sides: the angle on the other side.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle2: Option<String>,
        /// Symmetric: `angle` split evenly either side of the profile.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        symmetric: bool,
        /// To Object: turn until the face, vertex or point here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Vec3>,
    },
    Fillet {
        /// Reference points on the edges to round (each re-finds the nearest edge).
        edges: Vec<Vec3>,
        radius: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        /// Constant radius (the default), chord length (`radius` is the width across), or a
        /// radius varying along each edge.
        #[serde(default, skip_serializing_if = "FilletStyle::is_constant")]
        style: FilletStyle,
    },
    Chamfer {
        edges: Vec<Vec3>,
        distance: String,
        /// Unequal chamfers: the distance along the second face…
        #[serde(default, skip_serializing_if = "Option::is_none")]
        distance2: Option<String>,
        /// …or the angle the chamfer makes with the first face.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<String>,
        /// Swap which face is the first (by default the one facing up most).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    Box {
        corner: Vec3,
        length: String,
        width: String,
        height: String,
        #[serde(default)]
        operation: Operation,
    },
    Cylinder {
        base: Vec3,
        #[serde(default = "z_axis")]
        axis: Vec3,
        radius: String,
        height: String,
        #[serde(default)]
        operation: Operation,
    },
    Sphere {
        center: Vec3,
        radius: String,
        #[serde(default)]
        operation: Operation,
    },
    Torus {
        center: Vec3,
        major: String,
        minor: String,
        #[serde(default)]
        operation: Operation,
    },
    Combine {
        target: String,
        tools: Vec<String>,
        operation: Operation,
        #[serde(default)]
        keep_tools: bool,
    },
    /// Copies of other features' results, or of bodies (rectangular, circular or along a path).
    Pattern {
        features: Vec<String>,
        pattern: PatternKind,
        /// Copy these bodies (as new bodies) instead of replaying features.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bodies: Vec<String>,
    },
    /// Mirror copies of other features' results in a plane.
    Mirror {
        features: Vec<String>,
        plane: PlaneRef,
        /// Mirror these bodies instead of replaying features.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bodies: Vec<String>,
        /// Join each mirrored body with its original.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        combine: bool,
    },
    /// A drilled hole (simple, counterbore or countersink) at a point, into the material.
    Hole {
        position: Vec3,
        /// Drilling direction (into the material).
        direction: Vec3,
        diameter: String,
        /// Depth to the shoulder; `None` = through all.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<String>,
        #[serde(default)]
        hole: HoleKind,
        /// Drill at these sketch points instead (one hole each, perpendicular to the sketch);
        /// the holes follow the points when the sketch changes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        points: Option<SketchPoints>,
        /// A cosmetic thread in the holes (e.g. "M6x1").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread: Option<String>,
        /// To Object: drill down to the face (or point) here instead of `depth`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Vec3>,
    },
    /// A thread on a cylindrical face. Cosmetic by default (no geometry change; drawn and
    /// exported as an annotation); `modeled` cuts the real helical ISO profile into the face.
    /// `designation` is an ISO metric size such as "M8" or "M8x1".
    Thread {
        face: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        designation: Option<String>,
        /// Thread length from the face's end nearest `face`; `None` = full length.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        modeled: bool,
    },
    /// Ruled loft through profiles of several sketches, in order.
    Loft {
        sections: Vec<LoftSection>,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// Sweep profiles along a chain of sketch curves.
    Sweep {
        sketch: u64,
        profiles: ProfileSel,
        path_sketch: u64,
        path: Vec<String>,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// Hollow a body leaving walls of `thickness`, removing the faces at the given points.
    Shell {
        faces: Vec<Vec3>,
        thickness: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        /// inside (default) | outside | both (the wall straddles the faces).
        #[serde(default, skip_serializing_if = "String::is_empty")]
        direction: String,
        /// Open the faces smoothly joined to the picked ones too.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        tangent_chain: bool,
    },
    /// Tilt faces (at the given points) about a neutral plane, leaning toward `pull`.
    Draft {
        faces: Vec<Vec3>,
        angle: String,
        neutral: PlaneRef,
        pull: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// A construction plane (sketch placement, split tool).
    ConstructionPlane {
        plane: PlaneRef,
    },
    /// Split a body into the parts on either side of a plane, or of a face of a body (the face
    /// through `point`, its surface extended) when `tool` is set.
    Split {
        body: String,
        plane: PlaneRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool: Option<FaceAt>,
    },
    /// Split faces (at the given points) where a tool meets them, the solid unchanged: a plane,
    /// a face of a body (`tool`, its surface extended) or a sketch's curves (`sketch`, swept
    /// along its normal). The pieces are named after the face (`#k`).
    SplitFace {
        faces: Vec<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        plane: PlaneRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool: Option<FaceAt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<u64>,
    },
    /// Scale bodies about a point (uniform, or per axis).
    Scale {
        bodies: Vec<String>,
        #[serde(default)]
        origin: Vec3,
        factor: String,
        /// Per-axis factors instead of `factor`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        factors: Option<[String; 3]>,
    },
    /// Move planar faces (at the given points) along their normals; the body keeps its shape.
    OffsetFace {
        faces: Vec<Vec3>,
        distance: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Remove faces (at the given points) and heal the gap from the neighbouring planes, as when
    /// a fillet, chamfer, hole or boss is taken off. A picked face brings the rest of its surface
    /// (both halves of a hole's wall).
    DeleteFace {
        faces: Vec<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// A box around bodies, grown by a margin.
    BoundingSolid {
        bodies: Vec<String>,
        #[serde(default = "zero_expr")]
        margin: String,
    },
    /// A tube along a chain of sketch curves (solid, or hollow with a wall thickness).
    Pipe {
        path_sketch: u64,
        path: Vec<String>,
        diameter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wall: Option<String>,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// Raise (emboss) or sink (deboss) sketch profiles on the planar face they lie on.
    Emboss {
        sketch: u64,
        profiles: ProfileSel,
        depth: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        deboss: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// A thin wall from open sketch curves: `thickness` across the sketch plane, filling from
    /// the curves toward the body (to the next face, or `depth`). A web makes one per curve.
    Rib {
        sketch: u64,
        curves: Vec<String>,
        thickness: String,
        /// Depth from the curves; `None` = to the next face of the body.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<String>,
        /// Fill toward the other side of the curves.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// Web: each curve is its own wall.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        web: bool,
    },
    /// Move planar faces (at the given points) onto a target plane parallel to them.
    ReplaceFace {
        faces: Vec<Vec3>,
        target: PlaneRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Move bodies so that `from` lands on `to` (and, with normals, the faces meet face to face).
    Align {
        bodies: Vec<String>,
        from: Vec3,
        to: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_normal: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_normal: Option<Vec3>,
        /// Normals the same way instead of facing each other.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// A helical coil: a circular or square section swept `turns` times about `axis`, rising
    /// `pitch` per turn, on a helix of `diameter` (the section's centre, or its inside or outside).
    Coil {
        base: Vec3,
        #[serde(default = "z_axis")]
        axis: Vec3,
        diameter: String,
        pitch: String,
        turns: String,
        section_size: String,
        #[serde(default)]
        section: CoilSection,
        /// -1 inside, 0 on the centre, 1 outside of the diameter.
        #[serde(default, skip_serializing_if = "is_zero_i8")]
        position: i8,
        /// Start angle about the axis (expression; 0 = from the axis toward +x, or a
        /// perpendicular direction when the axis is x).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_angle: Option<String>,
        /// Turn the other way (left-handed).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clockwise: bool,
        #[serde(default)]
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// Sheet metal base flange: a sheet from a closed profile (thickness from the rule).
    SheetBase {
        sketch: u64,
        profiles: ProfileSel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
        /// Thickness grows against the sketch normal.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
    },
    /// Sheet metal contour flange: a sheet bent along an open chain of sketch lines.
    SheetContour {
        sketch: u64,
        curves: Vec<String>,
        distance: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
        /// Material on the other side of the chain.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// Width the other way along the sketch normal.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reverse: bool,
    },
    /// Edge flanges on sheet edges (points on the top or bottom edges).
    SheetFlange {
        edges: Vec<Vec3>,
        height: String,
        angle: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<String>,
        /// inside | outside | middle.
        #[serde(default = "inside_str")]
        position: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Flat hems on sheet edges.
    SheetHem {
        edges: Vec<Vec3>,
        length: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Fold a sheet along a line on its base face: a sketch line (followed when the sketch
    /// changes), else the points `a`, `b`.
    SheetFold {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve: Option<String>,
        a: Vec3,
        b: Vec3,
        angle: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<String>,
        /// centerline | start | end | mould
        position: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// A point on the side that stays put (default: the larger side).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Unfold a sheet body flat (all bends), or refold it.
    SheetUnfold {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        refold: bool,
    },
    /// Convert a plate body (constant thickness from the face at `face`) to sheet metal.
    SheetConvert {
        body: String,
        face: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<String>,
    },
    /// Plastic boss: a round post on a face (draft, root fillet, hole, ribs).
    Boss {
        position: Vec3,
        /// Out of the face (the boss grows along it).
        #[serde(default = "z_axis")]
        direction: Vec3,
        diameter: String,
        height: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hole_diameter: Option<String>,
        /// Hole depth from the top (default: down to the face).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hole_depth: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fillet: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ribs: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rib_thickness: Option<String>,
        /// Rib reach out from the boss wall.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rib_length: Option<String>,
        /// Rib top below the boss top.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rib_offset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Plastic lip (raised band) or groove (recess) along the rim face picked at `face`.
    Lip {
        face: Vec3,
        width: String,
        height: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        groove: bool,
        /// Clearance added to a groove's width (and depth).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gap: Option<String>,
        /// Along the rim's outer edge instead of its inner one.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        outside: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Plastic snap fit: a cantilever arm on a face with a hook (catch) at its tip.
    SnapFit {
        position: Vec3,
        /// The arm grows along this (out of the face).
        #[serde(default = "z_axis")]
        direction: Vec3,
        /// The catch sticks out this way.
        hook: Vec3,
        length: String,
        thickness: String,
        width: String,
        catch_depth: String,
        /// The catch's height along the arm.
        catch_length: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Plastic rest: a raised pad on a face (round, or a rectangle), solid or a hollow wall,
    /// with draft.
    Rest {
        position: Vec3,
        /// Out of the face (the pad grows along it).
        #[serde(default = "z_axis")]
        direction: Vec3,
        /// The rectangle's length runs along this (default: any direction in the face).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        along: Option<Vec3>,
        /// Diameter of a round rest, or the rectangle's width.
        width: String,
        /// The rectangle's length (none: round).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<String>,
        height: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<String>,
        /// Wall thickness of a hollow rest (none: solid).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Boundary Fill: solids from the cells the tool bodies enclose (a cell: inside some
    /// tools and outside the rest), each picked by a point inside it.
    BoundaryFill {
        tools: Vec<String>,
        cells: Vec<Vec3>,
        #[serde(default)]
        operation: Operation,
        /// Remove the tool bodies afterwards.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        remove_tools: bool,
    },
    /// Remove bodies from the model (from here on in the timeline).
    Remove {
        bodies: Vec<String>,
    },
    /// Surface bodies: planar patches filling sketch regions (`profiles` of `sketch`), or a
    /// closed loop of a body's edges (`edges`: points on them, on `body`).
    Patch {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<u64>,
        #[serde(default = "all_profiles")]
        profiles: ProfileSel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<Vec3>,
    },
    /// Surface bodies sewn along shared edges into the first one (a solid when they close).
    Stitch {
        bodies: Vec<String>,
        #[serde(default = "zero_expr")]
        tolerance: String,
    },
    /// Surface bodies thickened into solids along their normals (by half each way when
    /// `symmetric`).
    Thicken {
        bodies: Vec<String>,
        thickness: String,
        #[serde(default)]
        symmetric: bool,
        operation: Operation,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        targets: Vec<String>,
    },
    /// A planar surface body extended outward by `distance` past the edges through `edges`.
    SurfaceExtend {
        body: String,
        edges: Vec<Vec3>,
        distance: String,
    },
    /// A surface body trimmed by a plane, or by a face of a body (`tool`), keeping the side of
    /// `keep`.
    SurfaceTrim {
        body: String,
        plane: PlaneRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool: Option<FaceAt>,
        keep: Vec3,
    },
    Move {
        bodies: Vec<String>,
        translate: [String; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotate_axis: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<String>,
    },
    /// A non-parametric base feature: bodies imported from a STEP file. The STEP text is kept
    /// in the design (it is the B-rep), so the design reopens without the original file.
    Import {
        /// Source file name (display only).
        file: String,
        /// The STEP (ISO 10303-21) text.
        step: String,
        /// The file's product/assembly tree, kept for components.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        components: Vec<solvecraft_kernel::StepNode>,
    },
    /// A non-parametric base feature: mesh bodies imported from a 3MF or STL file (kept in the
    /// design). Mesh bodies render, measure, move and export; solid features cannot use them.
    MeshImport {
        /// Source file name (display only).
        file: String,
        meshes: Vec<MeshData>,
    },
}

/// How a fillet's size is given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FilletStyle {
    #[default]
    Constant,
    /// `radius` is the chord: the width across the blend.
    Chord,
    /// `radius` at the end of each edge nearest `start`, `radius2` at the other, linear between.
    Variable { radius2: String, start: Vec3 },
}

impl FilletStyle {
    pub fn is_constant(&self) -> bool {
        *self == FilletStyle::Constant
    }
}

/// A face picked by a point on it, on a named body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceAt {
    pub body: String,
    pub point: Vec3,
}

/// Triangles of one imported mesh body (millimetres).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub name: String,
    /// x, y, z per vertex.
    pub positions: Vec<f64>,
    /// Three vertex indices per triangle.
    pub triangles: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
}

/// Points of a sketch, by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPoints {
    pub sketch: u64,
    pub ids: Vec<String>,
}

/// A component: a node of the design tree that owns sketches, features and bodies. The root
/// (id 0) is the design itself and is not stored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: u64,
    pub name: String,
    /// Parent component (0 = the root).
    #[serde(default)]
    pub parent: u64,
}

/// One loft section: profiles of a sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftSection {
    pub sketch: u64,
    pub profiles: ProfileSel,
    /// A sketch point instead of a profile (the loft closes to it, as a pyramid's apex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
}

/// Coil cross-sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoilSection {
    #[default]
    Circular,
    Square,
}

fn is_zero_i8(v: &i8) -> bool {
    *v == 0
}

/// Hole shapes.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HoleKind {
    /// Plain hole; blind holes get a drill point when `tip_angle` is set.
    #[default]
    Simple,
    Drilled {
        tip_angle: String,
    },
    Counterbore {
        cb_diameter: String,
        cb_depth: String,
    },
    Countersink {
        cs_diameter: String,
        cs_angle: String,
    },
}

/// Pattern layouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternKind {
    Rectangular {
        dir1: Vec3,
        count1: String,
        spacing1: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir2: Option<Vec3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count2: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spacing2: Option<String>,
    },
    Circular {
        origin: Vec3,
        axis: Vec3,
        count: String,
        /// Total angle (360 deg = evenly around).
        angle: String,
    },
    /// Copies along a chain of sketch curves: instance k moves by path(k·spacing) − path(0),
    /// measured along the path from its start.
    Path {
        path_sketch: u64,
        path: Vec<String>,
        count: String,
        /// Distance along the path between instances, or the whole extent when `extent`.
        spacing: String,
        /// `spacing` is the distance from the first to the last instance.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        extent: bool,
        /// Run from the path's other end.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// Turn copies with the path's direction (else they keep their orientation).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        orient: bool,
    },
}

fn plane_exprs<'a>(p: &'a PlaneRef, v: &mut Vec<&'a str>) {
    let mut p = p;
    for _ in 0..64 {
        match p {
            PlaneRef::Offset { base, distance } => {
                v.push(distance);
                p = base;
            }
            PlaneRef::AtAngle { base, angle, .. } => {
                v.push(angle);
                p = base;
            }
            _ => break,
        }
    }
}

fn z_axis() -> Vec3 {
    Vec3::Z
}

impl FeatureKind {
    /// A sheet metal feature (its result depends on the sheet metal rules).
    pub fn is_plastic(&self) -> bool {
        matches!(self, FeatureKind::Boss { .. } | FeatureKind::Lip { .. } | FeatureKind::SnapFit { .. } | FeatureKind::Rest { .. })
    }

    pub fn is_sheet(&self) -> bool {
        matches!(
            self,
            FeatureKind::SheetBase { .. }
                | FeatureKind::SheetContour { .. }
                | FeatureKind::SheetFlange { .. }
                | FeatureKind::SheetHem { .. }
                | FeatureKind::SheetFold { .. }
                | FeatureKind::SheetUnfold { .. }
                | FeatureKind::SheetConvert { .. }
        )
    }

    /// Fusion-like type name (timeline tooltip, `inspect`).
    pub fn type_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "ExtrudeFeature",
            FeatureKind::Revolve { .. } => "RevolveFeature",
            FeatureKind::Fillet { .. } => "FilletFeature",
            FeatureKind::Chamfer { .. } => "ChamferFeature",
            FeatureKind::Box { .. } => "BoxFeature",
            FeatureKind::Cylinder { .. } => "CylinderFeature",
            FeatureKind::Sphere { .. } => "SphereFeature",
            FeatureKind::Torus { .. } => "TorusFeature",
            FeatureKind::Combine { .. } => "CombineFeature",
            FeatureKind::Pattern { pattern: PatternKind::Rectangular { .. }, .. } => "RectangularPatternFeature",
            FeatureKind::Pattern { pattern: PatternKind::Path { .. }, .. } => "PathPatternFeature",
            FeatureKind::Pattern { .. } => "CircularPatternFeature",
            FeatureKind::Mirror { .. } => "MirrorFeature",
            FeatureKind::ConstructionPlane { .. } => "ConstructionPlane",
            FeatureKind::Hole { .. } => "HoleFeature",
            FeatureKind::Thread { .. } => "ThreadFeature",
            FeatureKind::Loft { .. } => "LoftFeature",
            FeatureKind::Sweep { .. } => "SweepFeature",
            FeatureKind::Shell { .. } => "ShellFeature",
            FeatureKind::Draft { .. } => "DraftFeature",
            FeatureKind::Split { .. } => "SplitBodyFeature",
            FeatureKind::SplitFace { .. } => "SplitFaceFeature",
            FeatureKind::Move { .. } => "MoveFeature",
            FeatureKind::Scale { .. } => "ScaleFeature",
            FeatureKind::OffsetFace { .. } => "OffsetFacesFeature",
            FeatureKind::DeleteFace { .. } => "DeleteFaceFeature",
            FeatureKind::BoundingSolid { .. } => "BoundingSolidFeature",
            FeatureKind::Pipe { .. } => "PipeFeature",
            FeatureKind::Emboss { .. } => "EmbossFeature",
            FeatureKind::Boss { .. } => "BossFeature",
            FeatureKind::Lip { groove: false, .. } => "LipFeature",
            FeatureKind::Lip { .. } => "GrooveFeature",
            FeatureKind::SnapFit { .. } => "SnapFitFeature",
            FeatureKind::Rest { .. } => "RestFeature",
            FeatureKind::BoundaryFill { .. } => "BoundaryFillFeature",
            FeatureKind::SheetBase { .. } => "BaseFlangeFeature",
            FeatureKind::SheetContour { .. } => "ContourFlangeFeature",
            FeatureKind::SheetFlange { .. } => "EdgeFlangeFeature",
            FeatureKind::SheetHem { .. } => "HemFeature",
            FeatureKind::SheetFold { .. } => "FoldFeature",
            FeatureKind::SheetUnfold { refold: false, .. } => "UnfoldFeature",
            FeatureKind::SheetUnfold { .. } => "RefoldFeature",
            FeatureKind::SheetConvert { .. } => "ConvertToSheetMetalFeature",
            FeatureKind::Coil { .. } => "CoilFeature",
            FeatureKind::Rib { web: false, .. } => "RibFeature",
            FeatureKind::Rib { .. } => "WebFeature",
            FeatureKind::ReplaceFace { .. } => "ReplaceFaceFeature",
            FeatureKind::Align { .. } => "AlignFeature",
            FeatureKind::Remove { .. } => "RemoveFeature",
            FeatureKind::Patch { .. } => "PatchFeature",
            FeatureKind::Stitch { .. } => "StitchFeature",
            FeatureKind::Thicken { .. } => "ThickenFeature",
            FeatureKind::SurfaceTrim { .. } => "TrimFeature",
            FeatureKind::SurfaceExtend { .. } => "ExtendFeature",
            FeatureKind::Import { .. } => "BaseFeature",
            FeatureKind::MeshImport { .. } => "MeshFeature",
        }
    }
    /// Default name prefix (`Extrude` → `Extrude1`).
    pub fn base_name(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Sketch",
            FeatureKind::Extrude { .. } => "Extrude",
            FeatureKind::Revolve { .. } => "Revolve",
            FeatureKind::Fillet { .. } => "Fillet",
            FeatureKind::Chamfer { .. } => "Chamfer",
            FeatureKind::Box { .. } => "Box",
            FeatureKind::Cylinder { .. } => "Cylinder",
            FeatureKind::Sphere { .. } => "Sphere",
            FeatureKind::Torus { .. } => "Torus",
            FeatureKind::Combine { .. } => "Combine",
            FeatureKind::Pattern { pattern: PatternKind::Rectangular { .. }, .. } => "RectangularPattern",
            FeatureKind::Pattern { pattern: PatternKind::Path { .. }, .. } => "PathPattern",
            FeatureKind::Pattern { .. } => "CircularPattern",
            FeatureKind::Mirror { .. } => "Mirror",
            FeatureKind::ConstructionPlane { .. } => "Plane",
            FeatureKind::Hole { .. } => "Hole",
            FeatureKind::Thread { .. } => "Thread",
            FeatureKind::Loft { .. } => "Loft",
            FeatureKind::Sweep { .. } => "Sweep",
            FeatureKind::Shell { .. } => "Shell",
            FeatureKind::Draft { .. } => "Draft",
            FeatureKind::Split { .. } => "Split",
            FeatureKind::SplitFace { .. } => "SplitFace",
            FeatureKind::Move { .. } => "Move",
            FeatureKind::Scale { .. } => "Scale",
            FeatureKind::OffsetFace { .. } => "OffsetFace",
            FeatureKind::DeleteFace { .. } => "DeleteFace",
            FeatureKind::BoundingSolid { .. } => "BoundingSolid",
            FeatureKind::Pipe { .. } => "Pipe",
            FeatureKind::Emboss { .. } => "Emboss",
            FeatureKind::Boss { .. } => "Boss",
            FeatureKind::Lip { groove: false, .. } => "Lip",
            FeatureKind::Lip { .. } => "Groove",
            FeatureKind::SnapFit { .. } => "SnapFit",
            FeatureKind::Rest { .. } => "Rest",
            FeatureKind::BoundaryFill { .. } => "BoundaryFill",
            FeatureKind::SheetBase { .. } => "BaseFlange",
            FeatureKind::SheetContour { .. } => "ContourFlange",
            FeatureKind::SheetFlange { .. } => "EdgeFlange",
            FeatureKind::SheetHem { .. } => "Hem",
            FeatureKind::SheetFold { .. } => "Fold",
            FeatureKind::SheetUnfold { refold: false, .. } => "Unfold",
            FeatureKind::SheetUnfold { .. } => "Refold",
            FeatureKind::SheetConvert { .. } => "ConvertToSheetMetal",
            FeatureKind::Coil { .. } => "Coil",
            FeatureKind::Rib { web: false, .. } => "Rib",
            FeatureKind::Rib { .. } => "Web",
            FeatureKind::ReplaceFace { .. } => "ReplaceFace",
            FeatureKind::Align { .. } => "Align",
            FeatureKind::Remove { .. } => "Remove",
            FeatureKind::Patch { .. } => "Patch",
            FeatureKind::Stitch { .. } => "Stitch",
            FeatureKind::Thicken { .. } => "Thicken",
            FeatureKind::SurfaceTrim { .. } => "Trim",
            FeatureKind::SurfaceExtend { .. } => "Extend",
            FeatureKind::Import { .. } => "Import",
            FeatureKind::MeshImport { .. } => "Mesh",
        }
    }
    /// Every expression the feature uses.
    /// Points on faces the feature refers to (shell, draft, offset and replace faces, a hole's
    /// placement), for persistent face names.
    pub fn face_points_mut(&mut self) -> Vec<&mut Vec3> {
        match self {
            FeatureKind::Shell { faces, .. }
            | FeatureKind::Draft { faces, .. }
            | FeatureKind::OffsetFace { faces, .. }
            | FeatureKind::DeleteFace { faces, .. }
            | FeatureKind::SplitFace { faces, .. }
            | FeatureKind::ReplaceFace { faces, .. } => faces.iter_mut().collect(),
            FeatureKind::Hole { position, points: None, to, .. } => std::iter::once(position).chain(to.iter_mut()).collect(),
            FeatureKind::Hole { to: Some(t), .. } => vec![t],
            FeatureKind::Extrude { extent, .. } => extent.to.iter_mut().chain(extent.from.iter_mut()).collect(),
            FeatureKind::Revolve { to: Some(p), .. } => vec![p],
            FeatureKind::Thread { face, .. } => vec![face],
            _ => Vec::new(),
        }
    }

    pub fn face_points(&self) -> Vec<Vec3> {
        let mut k = self.clone();
        k.face_points_mut().into_iter().map(|p| *p).collect()
    }

    pub fn expressions(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        match self {
            FeatureKind::Sketch { plane, .. }
            | FeatureKind::ConstructionPlane { plane }
            | FeatureKind::Split { plane, .. }
            | FeatureKind::SplitFace { plane, .. } => plane_exprs(plane, &mut v),
            FeatureKind::Extrude { extent, .. } => {
                v.push(&extent.distance);
                if let Some(d) = &extent.distance2 {
                    v.push(d);
                }
                if let Some(d) = &extent.start_offset {
                    v.push(d);
                }
                if let Some(d) = &extent.taper {
                    v.push(d);
                }
            }
            FeatureKind::Revolve { angle, angle2, .. } => {
                v.push(angle);
                v.extend(angle2.iter().map(String::as_str));
            }
            FeatureKind::Fillet { radius, style, .. } => {
                v.push(radius);
                if let FilletStyle::Variable { radius2, .. } = style {
                    v.push(radius2);
                }
            }
            FeatureKind::Chamfer { distance, distance2, angle, .. } => {
                v.push(distance);
                v.extend(distance2.iter().map(String::as_str));
                v.extend(angle.iter().map(String::as_str));
            }
            FeatureKind::Box { length, width, height, .. } => v.extend([length.as_str(), width, height]),
            FeatureKind::Cylinder { radius, height, .. } => v.extend([radius.as_str(), height]),
            FeatureKind::Sphere { radius, .. } => v.push(radius),
            FeatureKind::Torus { major, minor, .. } => v.extend([major.as_str(), minor]),
            FeatureKind::Combine { .. } => {}
            FeatureKind::Pattern { pattern, .. } => match pattern {
                PatternKind::Rectangular { count1, spacing1, count2, spacing2, .. } => {
                    v.extend([count1.as_str(), spacing1]);
                    v.extend(count2.iter().map(String::as_str));
                    v.extend(spacing2.iter().map(String::as_str));
                }
                PatternKind::Circular { count, angle, .. } => v.extend([count.as_str(), angle]),
                PatternKind::Path { count, spacing, .. } => v.extend([count.as_str(), spacing]),
            },
            FeatureKind::Mirror { plane, .. } => plane_exprs(plane, &mut v),
            FeatureKind::Shell { thickness, .. } => v.push(thickness),
            FeatureKind::Loft { .. } | FeatureKind::Sweep { .. } => {}
            FeatureKind::Draft { angle, neutral, .. } => {
                v.push(angle);
                plane_exprs(neutral, &mut v);
            }
            FeatureKind::Hole { diameter, depth, hole, .. } => {
                v.push(diameter);
                v.extend(depth.iter().map(String::as_str));
                match hole {
                    HoleKind::Simple => {}
                    HoleKind::Drilled { tip_angle } => v.push(tip_angle),
                    HoleKind::Counterbore { cb_diameter, cb_depth } => v.extend([cb_diameter.as_str(), cb_depth]),
                    HoleKind::Countersink { cs_diameter, cs_angle } => v.extend([cs_diameter.as_str(), cs_angle]),
                }
            }
            FeatureKind::Thread { length, .. } => v.extend(length.iter().map(String::as_str)),
            FeatureKind::Scale { factor, factors, .. } => {
                v.push(factor);
                v.extend(factors.iter().flatten().map(String::as_str));
            }
            FeatureKind::OffsetFace { distance, .. } => v.push(distance),
            FeatureKind::BoundingSolid { margin, .. } => v.push(margin),
            FeatureKind::Pipe { diameter, wall, .. } => {
                v.push(diameter);
                v.extend(wall.iter().map(String::as_str));
            }
            FeatureKind::Move { translate, angle, .. } => {
                v.extend(translate.iter().map(String::as_str));
                if let Some(a) = angle {
                    v.push(a);
                }
            }
            FeatureKind::Emboss { depth, .. } => v.push(depth),
            FeatureKind::Boss { diameter, height, hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset, .. } => {
                v.extend([diameter.as_str(), height]);
                for o in [hole_diameter, hole_depth, draft, fillet, ribs, rib_thickness, rib_length, rib_offset].into_iter().flatten() {
                    v.push(o);
                }
            }
            FeatureKind::Lip { width, height, gap, .. } => {
                v.extend([width.as_str(), height]);
                v.extend(gap.iter().map(String::as_str));
            }
            FeatureKind::SnapFit { length, thickness, width, catch_depth, catch_length, .. } => {
                v.extend([length.as_str(), thickness, width, catch_depth, catch_length]);
            }
            FeatureKind::Rest { width, length, height, draft, thickness, .. } => {
                v.extend([width.as_str(), height]);
                v.extend([length, draft, thickness].into_iter().flatten().map(String::as_str));
            }
            FeatureKind::SheetContour { distance, .. } => v.push(distance),
            FeatureKind::SheetFlange { height, angle, radius, .. } => {
                v.extend([height.as_str(), angle]);
                v.extend(radius.iter().map(String::as_str));
            }
            FeatureKind::SheetFold { angle, radius, .. } => {
                v.push(angle);
                v.extend(radius.iter().map(String::as_str));
            }
            FeatureKind::SheetHem { length, gap, .. } => {
                v.push(length);
                v.extend(gap.iter().map(String::as_str));
            }
            FeatureKind::SheetBase { .. } | FeatureKind::SheetUnfold { .. } | FeatureKind::SheetConvert { .. } => {}
            FeatureKind::Coil { diameter, pitch, turns, section_size, start_angle, .. } => {
                v.extend([diameter.as_str(), pitch, turns, section_size]);
                v.extend(start_angle.iter().map(String::as_str));
            }
            FeatureKind::Rib { thickness, depth, .. } => {
                v.push(thickness);
                v.extend(depth.iter().map(String::as_str));
            }
            FeatureKind::ReplaceFace { target, .. } => plane_exprs(target, &mut v),
            FeatureKind::Align { .. } | FeatureKind::Remove { .. } | FeatureKind::BoundaryFill { .. } | FeatureKind::DeleteFace { .. } => {}
            FeatureKind::Stitch { tolerance, .. } => v.push(tolerance),
            FeatureKind::Thicken { thickness, .. } => v.push(thickness),
            FeatureKind::SurfaceTrim { plane, .. } => plane_exprs(plane, &mut v),
            FeatureKind::SurfaceExtend { distance, .. } => v.push(distance),
            FeatureKind::Patch { .. } => {}
            FeatureKind::Import { .. } | FeatureKind::MeshImport { .. } => {}
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    /// Names for the bodies this feature creates (in order); missing ones get `BodyN`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_names: Vec<String>,
    /// Parameter names of the feature's inputs (see `FeatureKind::inputs`), e.g. `d3`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_names: Vec<String>,
    /// The component the feature (and what it makes) belongs to; 0 = the root.
    #[serde(default, skip_serializing_if = "is_root")]
    pub component: u64,
    /// For each picked edge (fillets, chamfers): where it sat in its body when picked, so it
    /// is found again when earlier edits move or resize the body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_refs: Vec<crate::EdgeRef>,
    /// Persistent names of the picked edges (resolved first; see `naming`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_names: Vec<String>,
    /// Persistent names of the faces at the feature's face points (`FeatureKind::face_points`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_names: Vec<String>,
    /// Bodies a join/cut/intersect may change (any component); empty = the feature's own
    /// component's bodies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participants: Vec<String>,
    #[serde(flatten)]
    pub kind: FeatureKind,
}

fn inside_str() -> String {
    "inside".into()
}

fn all_profiles() -> ProfileSel {
    ProfileSel::All
}

fn zero_expr() -> String {
    "0".into()
}

fn is_root(c: &u64) -> bool {
    *c == 0
}

/// A standard part (fastener, pin, bearing) inserted as a component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StandardPart {
    pub family: String,
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
}

/// The document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default = "default_format")]
    pub format: String,
    pub name: String,
    #[serde(default = "default_units")]
    pub units: String,
    pub params: Vec<Param>,
    pub features: Vec<Feature>,
    /// Timeline marker: features at or after this index are rolled back (not evaluated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<usize>,
    #[serde(default)]
    pub next_id: u64,
    /// Components below the root, as a tree (by parent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
    /// Placements of components in their parents (each non-root component has at least one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<crate::Occurrence>,
    /// Joints between occurrences, joint origins and motion links.
    #[serde(default, skip_serializing_if = "crate::joints::Assembly::is_empty")]
    pub assembly: crate::joints::Assembly,
    /// Sheet metal rules.
    #[serde(default, skip_serializing_if = "crate::sheet::SheetSettings::is_empty")]
    pub sheet: crate::sheet::SheetSettings,
    /// Plastic rules.
    #[serde(default, skip_serializing_if = "crate::plastic::PlasticSettings::is_empty")]
    pub plastic: crate::plastic::PlasticSettings,
    /// Components that are standard parts (by component id): what to rebuild them from.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub parts: std::collections::BTreeMap<u64, StandardPart>,
    /// Configurations (rows of parameter values and suppressions).
    #[serde(default, skip_serializing_if = "crate::config::ConfigTable::is_empty")]
    pub configs: crate::config::ConfigTable,
    /// Favourite parameters (by name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub favorites: std::collections::BTreeSet<String>,
    /// Reference images on planes (Insert Canvas).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canvases: Vec<crate::canvas::Canvas>,
    /// Comments of feature-input parameters (by name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub param_comments: std::collections::BTreeMap<String, String>,
    /// Physical material per body (by body name); others use the default.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub materials: std::collections::BTreeMap<String, String>,
    /// Appearance colour per body (by body name, sRGB); it overrides the material's look.
    #[serde(default, skip_serializing_if = "crate::appearance::Appearances::is_empty")]
    pub appearances: crate::appearance::Appearances,
    /// Bodies moved into another component than their feature's (by body name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub body_components: std::collections::BTreeMap<String, u64>,
    /// Bodies moved into another component keep their place in the world: the transform from
    /// the new component's frame to the frame the body was made in (by body name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub body_offsets: std::collections::BTreeMap<String, crate::Mat>,
    /// Browser groups (folders of bodies, sketches or construction planes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browser_groups: Vec<BrowserGroup>,
    /// Browser item order per component folder (`"<component>/<folder>"` → item keys).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub browser_order: std::collections::BTreeMap<String, Vec<String>>,
    /// Named views: saved cameras.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_views: Vec<NamedView>,
}

/// A saved camera (orbit camera: target, yaw, pitch, distance; `fov` 0 for orthographic).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    #[serde(default)]
    pub name: String,
    pub target: solvecraft_geom::Vec3,
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    #[serde(default)]
    pub fov: f64,
}

/// A browser group: items of one folder (`bodies`, `sketches` or `construction`) of a
/// component, by key (body name, or the feature id of a sketch or construction plane).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowserGroup {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub component: u64,
    pub folder: String,
    #[serde(default)]
    pub items: Vec<String>,
}

fn default_format() -> String {
    crate::format::format_string(crate::format::FORMAT)
}
fn default_units() -> String {
    "mm".into()
}

impl Default for Document {
    fn default() -> Self {
        Document::new("Untitled")
    }
}

impl Document {
    pub fn new(name: &str) -> Self {
        Document {
            format: default_format(),
            name: name.into(),
            units: default_units(),
            params: Vec::new(),
            features: Vec::new(),
            marker: None,
            next_id: 1,
            components: Vec::new(),
            occurrences: Vec::new(),
            assembly: Default::default(),
            sheet: Default::default(),
            plastic: Default::default(),
            configs: Default::default(),
            parts: Default::default(),
            body_components: Default::default(),
            body_offsets: Default::default(),
            materials: Default::default(),
            appearances: Default::default(),
            favorites: Default::default(),
            canvases: Vec::new(),
            param_comments: Default::default(),
            browser_groups: Vec::new(),
            browser_order: Default::default(),
            named_views: Vec::new(),
        }
    }

    /// The colour a body is shown and exported in (sRGB as 0–1 fractions): its appearance,
    /// else its component's, else its material's look, else the colour it was imported with.
    /// (Faces may have their own: `face_looks`.)
    pub fn body_color(&self, b: &crate::ModelBody) -> Option<[f32; 3]> {
        self.body_look(&b.name, b.feature).map(|l| l.rgb()).or_else(|| b.body.color()).or_else(|| b.imported.as_ref().and_then(|i| i.color))
    }

    /// The model with each body carrying the colour, opacity and face colours it is shown with
    /// (for export).
    pub fn painted(&self, state: &crate::ModelState) -> crate::ModelState {
        let mut st = state.clone();
        for b in &mut st.bodies {
            let c = self.body_color(b);
            let look = self.body_look(&b.name, b.feature);
            let faces: Vec<solvecraft_kernel::FacePaint> = self
                .face_colors(b)
                .into_iter()
                .map(|(face, l)| solvecraft_kernel::FacePaint { face, color: l.rgb(), opacity: l.opacity as f32 })
                .collect();
            let opacity = look
                .map(|l| l.opacity as f32)
                .or_else(|| b.body.paint().map(|p| p.opacity))
                .or_else(|| b.imported.as_ref().and_then(|i| i.opacity))
                .unwrap_or(1.0);
            let paint = solvecraft_kernel::Paint { opacity, faces };
            if c != b.body.color() || b.body.paint() != Some(&paint) {
                b.body = b.body.clone().with_color(c).with_paint(Some(paint));
            }
        }
        st
    }

    /// Density of a body's material (g/cm³).
    pub fn density(&self, body: &str) -> f64 {
        self.materials.get(body).and_then(|m| material_density(m)).unwrap_or(DEFAULT_DENSITY)
    }

    /// The component a body belongs to: where it was moved, else its feature's.
    pub fn body_component(&self, body: &str, feature: u64) -> u64 {
        self.body_components.get(body).copied().unwrap_or_else(|| self.feature(feature).map(|f| f.component).unwrap_or(0))
    }

    /// Add a component under `parent` and return its id.
    pub fn add_component(&mut self, name: Option<&str>, parent: u64) -> Result<u64> {
        if parent != 0 && !self.components.iter().any(|c| c.id == parent) {
            return Err(DocError::Unknown(format!("component {parent}")));
        }
        if self.components.len() >= 10_000 {
            return Err(DocError::Invalid("too many components".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let name = match name.map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                let mut k = self.components.len() + 1;
                while self.components.iter().any(|c| c.name == format!("Component{k}")) {
                    k += 1;
                }
                format!("Component{k}")
            }
        };
        self.components.push(Component { id, name, parent });
        self.add_occurrence(id, parent, crate::IDENTITY)?;
        Ok(id)
    }

    /// A component by id or name (0 / "root" is the design itself).
    pub fn find_component(&self, key: &str) -> Option<u64> {
        if key == "0" || key.eq_ignore_ascii_case("root") || key == self.name {
            return Some(0);
        }
        self.components.iter().find(|c| c.id.to_string() == key || c.name == key).map(|c| c.id)
    }

    /// Read a design file's JSON (any format this build knows; older ones are upgraded).
    pub fn from_json(s: &str) -> Result<Document> {
        let d = crate::format::read(s)?;
        if d.features.len() > MAX_FEATURES || d.params.len() > MAX_PARAMS {
            return Err(DocError::Invalid("document too large".into()));
        }
        Ok(d)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn feature(&self, id: u64) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }
    pub fn feature_mut(&mut self, id: u64) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }
    pub fn feature_index(&self, id: u64) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }
    /// Find a feature by id (number) or name.
    pub fn find_feature(&self, r: &str) -> Option<&Feature> {
        if let Ok(id) = r.parse::<u64>()
            && let Some(f) = self.feature(id)
        {
            return Some(f);
        }
        self.features.iter().find(|f| f.name.eq_ignore_ascii_case(r))
    }

    fn unique_name(&self, base: &str) -> String {
        let mut n = 1;
        loop {
            let name = format!("{base}{n}");
            if !self.features.iter().any(|f| f.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// Insert a feature at the marker (or the end) and return its id.
    pub fn add_feature(&mut self, kind: FeatureKind, name: Option<&str>) -> Result<u64> {
        if self.features.len() >= MAX_FEATURES {
            return Err(DocError::Invalid("too many features".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        let name = match name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => self.unique_name(kind.base_name()),
        };
        let f = Feature {
            id,
            name,
            suppressed: false,
            body_names: Vec::new(),
            param_names: Vec::new(),
            component: 0,
            edge_refs: Vec::new(),
            edge_names: Vec::new(),
            face_names: Vec::new(),
            participants: Vec::new(),
            kind,
        };
        match self.marker {
            Some(m) if m < self.features.len() => {
                self.features.insert(m, f);
                self.marker = Some(m + 1);
            }
            _ => self.features.push(f),
        }
        self.name_feature_inputs(id);
        Ok(id)
    }

    /// Delete a feature (and, for a sketch, the features that use it).
    pub fn delete_feature(&mut self, id: u64) -> Result<Vec<u64>> {
        let idx = self.feature_index(id).ok_or_else(|| DocError::Unknown(format!("feature {id}")))?;
        let mut gone = vec![id];
        for f in &self.features {
            match &f.kind {
                FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } if *sketch == id => gone.push(f.id),
                FeatureKind::Sweep { sketch, path_sketch, .. } if *sketch == id || *path_sketch == id => gone.push(f.id),
                FeatureKind::Loft { sections, .. } if sections.iter().any(|s| s.sketch == id) => gone.push(f.id),
                FeatureKind::Pattern { pattern: PatternKind::Path { path_sketch, .. }, .. } if *path_sketch == id => gone.push(f.id),
                FeatureKind::Emboss { sketch, .. } | FeatureKind::Rib { sketch, .. } if *sketch == id => gone.push(f.id),
                FeatureKind::SheetBase { sketch, .. } | FeatureKind::SheetContour { sketch, .. } if *sketch == id => gone.push(f.id),
                FeatureKind::SheetFold { sketch: Some(s), .. } | FeatureKind::SplitFace { sketch: Some(s), .. } if *s == id => gone.push(f.id),
                _ => {}
            }
        }
        self.features.retain(|f| !gone.contains(&f.id));
        // Configuration columns of deleted features go with them.
        let keep: Vec<bool> = self.configs.columns.iter().map(|c| !matches!(c, crate::config::Column::Suppress(id) if gone.contains(id))).collect();
        if keep.contains(&false) {
            let mut k = keep.iter();
            self.configs.columns.retain(|_| k.next().copied().unwrap_or(true));
            for r in &mut self.configs.rows {
                let mut k = keep.iter();
                r.cells.retain(|_| k.next().copied().unwrap_or(true));
            }
        }
        if let Some(m) = self.marker
            && m > idx
        {
            self.marker = Some(m.saturating_sub(gone.len()).max(idx));
        }
        // Drop model parameters nobody references any more.
        self.prune_model_params();
        Ok(gone)
    }

    /// Remove model parameters not referenced by any dimension, feature or other parameter.
    pub fn prune_model_params(&mut self) {
        let used = self.referenced_names();
        self.params.retain(|p| !p.model || used.contains(&p.name));
    }

    fn referenced_names(&self) -> Vec<String> {
        let mut used: Vec<String> = Vec::new();
        for f in &self.features {
            for e in f.kind.expressions() {
                used.extend(expr::references(e));
            }
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                used.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
            }
        }
        for p in &self.params {
            used.extend(expr::references(&p.expr));
        }
        used
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Why a name cannot be a parameter name (None = fine).
    pub fn param_name_problem(name: &str) -> Option<String> {
        let mut cs = name.chars();
        if !matches!(cs.next(), Some(c) if c.is_alphabetic() || c == '_') || !cs.all(|c| c.is_alphanumeric() || c == '_') {
            return Some(format!("`{name}` is not a valid parameter name (letters, digits and _, starting with a letter)"));
        }
        if name.len() > 64 {
            return Some("parameter names are at most 64 characters".into());
        }
        if expr::is_reserved(name) {
            return Some(format!("`{name}` is a unit, constant or function name"));
        }
        None
    }

    /// Add or change a parameter. Fails if the result would not evaluate. A new parameter
    /// without a unit takes the unit of its value (mm, deg or none).
    pub fn set_param(&mut self, name: &str, expr_s: &str, unit: Option<&str>, comment: Option<&str>) -> Result<()> {
        if let Some(p) = Self::param_name_problem(name) {
            return Err(DocError::Invalid(p));
        }
        if expr_s.len() > 4096 {
            return Err(DocError::Expr("expression too long".into()));
        }
        if expr_s.trim().is_empty() {
            return Err(DocError::Expr("empty expression".into()));
        }
        if let Some(u) = unit
            && expr::unit_info(u).is_none()
        {
            return Err(DocError::Expr(format!("unknown unit `{u}` (mm, cm, m, in, ft, deg, rad or none)")));
        }
        if comment.is_some_and(|c| c.len() > 4096) {
            return Err(DocError::Invalid("comment too long".into()));
        }
        if self.features.iter().any(|f| f.param_names.iter().any(|n| n == name)) {
            return Err(DocError::Invalid(format!("`{name}` is a feature's parameter")));
        }
        let mut next = self.clone();
        let inferred = match unit {
            Some(u) => u.trim().to_string(),
            None => next.infer_unit(expr_s),
        };
        match next.params.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.expr = expr_s.trim().to_string();
                if let Some(u) = unit {
                    p.unit = u.trim().to_string();
                }
                if let Some(c) = comment {
                    p.comment = c.to_string();
                }
            }
            None => {
                if next.params.len() >= MAX_PARAMS {
                    return Err(DocError::Invalid("too many parameters".into()));
                }
                next.params.push(Param {
                    name: name.into(),
                    expr: expr_s.trim().into(),
                    unit: inferred,
                    comment: comment.unwrap_or_default().into(),
                    model: false,
                })
            }
        }
        let (_, errors) = next.param_values();
        if let Some(e) = errors.get(name) {
            return Err(DocError::Expr(e.clone()));
        }
        *self = next;
        Ok(())
    }

    /// Create the next free model parameter `dN` with the given expression and unit.
    pub fn new_model_param(&mut self, expr_s: &str, unit: &str) -> String {
        let name = self.next_d_name();
        self.params.push(Param { name: name.clone(), expr: expr_s.into(), unit: unit.into(), comment: String::new(), model: true });
        name
    }

    /// Evaluate an expression against the document's parameters.
    pub fn eval(&self, e: &str, kind: Kind) -> Result<f64> {
        let (vals, _) = self.param_values();
        Self::eval_in(&vals, e, kind)
    }

    pub fn eval_in(vals: &std::collections::BTreeMap<String, expr::Value>, e: &str, kind: Kind) -> Result<f64> {
        expr::eval_with(e, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`"))))?.to_kind(kind)
    }

    /// Resolve a plane reference.
    pub fn resolve_plane(&self, vals: &std::collections::BTreeMap<String, expr::Value>, p: &PlaneRef, depth: usize) -> Result<Plane> {
        if depth > 32 {
            return Err(DocError::Invalid("plane offsets nested too deeply".into()));
        }
        match p {
            PlaneRef::Origin { name } => Plane::named(name).ok_or_else(|| DocError::Unknown(format!("plane `{name}`"))),
            PlaneRef::Custom { plane } | PlaneRef::Face { plane, .. } => {
                Plane::new(plane.origin, plane.x, plane.y).ok_or_else(|| DocError::Invalid("degenerate plane".into()))
            }
            PlaneRef::Offset { base, distance } => {
                let b = self.resolve_plane(vals, base, depth + 1)?;
                Ok(b.offset(Self::eval_in(vals, distance, Kind::Length)?))
            }
            PlaneRef::AtAngle { base, axis_origin, axis_dir, angle } => {
                let b = self.resolve_plane(vals, base, depth + 1)?;
                let a = Self::eval_in(vals, angle, Kind::Angle)?;
                let d = axis_dir.normalized().ok_or_else(|| DocError::Invalid("rotation axis".into()))?;
                let rot = |v: Vec3| v * a.cos() + d.cross(v) * a.sin() + d * (d.dot(v) * (1.0 - a.cos()));
                let o = *axis_origin + rot(b.origin - *axis_origin);
                Plane::new(o, rot(b.x), rot(b.y)).ok_or_else(|| DocError::Invalid("degenerate plane".into()))
            }
            PlaneRef::Construction { name } => match self.find_feature(name).map(|f| &f.kind) {
                Some(FeatureKind::ConstructionPlane { plane }) => self.resolve_plane(vals, plane, depth + 1),
                _ => Err(DocError::Unknown(format!("construction plane `{name}`"))),
            },
        }
    }

    /// Mutable access to a sketch feature's sketch.
    pub fn sketch_mut(&mut self, id: u64) -> Result<&mut Sketch> {
        match self.feature_mut(id).map(|f| &mut f.kind) {
            Some(FeatureKind::Sketch { sketch, .. }) => Ok(sketch),
            Some(_) => Err(DocError::Invalid(format!("feature {id} is not a sketch"))),
            None => Err(DocError::Unknown(format!("sketch {id}"))),
        }
    }
    pub fn sketch(&self, id: u64) -> Result<&Sketch> {
        match self.feature(id).map(|f| &f.kind) {
            Some(FeatureKind::Sketch { sketch, .. }) => Ok(sketch),
            Some(_) => Err(DocError::Invalid(format!("feature {id} is not a sketch"))),
            None => Err(DocError::Unknown(format!("sketch {id}"))),
        }
    }

    /// Apply parameter values to every dimension in a sketch (in place).
    pub fn apply_dimension_values(&self, vals: &std::collections::BTreeMap<String, expr::Value>, sk: &mut Sketch) -> Result<()> {
        for c in &mut sk.constraints {
            if let Some(p) = &c.param {
                let kind = if c.kind.is_angle() { Kind::Angle } else { Kind::Length };
                let v = vals.get(p).copied().ok_or_else(|| DocError::Unknown(format!("parameter `{p}`")))?.to_kind(kind)?;
                c.kind.set_value(v);
            }
        }
        Ok(())
    }
}

/// Density of the default material (g/cm³), as Fusion's default.
pub const DEFAULT_DENSITY: f64 = 1.29;

/// Physical materials: name and density (g/cm³).
pub const MATERIALS: [(&str, f64); 14] = [
    ("Default", DEFAULT_DENSITY),
    ("Steel", 7.85),
    ("Stainless Steel", 8.0),
    ("Aluminum", 2.70),
    ("Brass", 8.50),
    ("Copper", 8.96),
    ("Titanium", 4.43),
    ("Cast Iron", 7.20),
    ("ABS Plastic", 1.06),
    ("PLA", 1.24),
    ("Nylon", 1.14),
    ("Polycarbonate", 1.20),
    ("Oak", 0.75),
    ("Glass", 2.50),
];

/// How a physical material looks (sRGB); the default material has no colour of its own.
pub fn material_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "Steel" => [158, 164, 172],
        "Stainless Steel" => [186, 191, 198],
        "Aluminum" => [204, 208, 214],
        "Brass" => [208, 172, 88],
        "Copper" => [204, 122, 80],
        "Titanium" => [150, 148, 158],
        "Cast Iron" => [104, 104, 110],
        "ABS Plastic" => [222, 222, 216],
        "PLA" => [110, 160, 222],
        "Nylon" => [236, 230, 214],
        "Polycarbonate" => [196, 216, 232],
        "Oak" => [190, 146, 92],
        "Glass" => [170, 208, 218],
        _ => return None,
    })
}

pub fn material_density(name: &str) -> Option<f64> {
    MATERIALS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, d)| *d)
}
