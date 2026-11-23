//! The anvil-brep adapter: answers the B-rep protocol's requests on stdin and stdout, so the
//! conformance suite can drive AnvilCAD's exact kernel.

use std::f64::consts::TAU;
use std::process::ExitCode;

use anvil_brep::blend::{Blend, blend_chain, edge_near};
use anvil_brep::boolean::{BoolOp, boolean, contains_point, face_contains};
use anvil_brep::loft::{Section, loft_ruled};
use anvil_brep::profile::{Loop2, Region, Segment, build_regions};
use anvil_brep::sweep::PathSegment;
use anvil_brep::tess::{TessParams, tessellate};
use anvil_brep::{Frame, KernelError, check, mass, tol};
use brep_protocol::{Mesh, Refusal, model, model::Primitive, profile};
use glam::{DVec2, DVec3};

/// The protocol's `10·ε`: how near a point must be to the edge it names.
const NAMES: f64 = 1e-6;

/// A solid set: anvil-brep's bodies, none for the empty solid. Its operations return a list of
/// bodies, and an empty list is its way of saying that a result is empty.
struct Solid(Vec<anvil_brep::Solid>);

struct Anvil;

fn v3(a: model::Vec3) -> DVec3 {
    DVec3::from_array(a)
}

fn arr(p: DVec3) -> model::Vec3 {
    p.to_array()
}

/// A kernel error as a refusal of the kind it reports.
fn err(e: KernelError) -> Refusal {
    let reason = e.to_string();
    match e {
        KernelError::Unsupported(_) => Refusal::unsupported(reason),
        KernelError::EmptyProfile
        | KernelError::NonFinite
        | KernelError::OpenLoop { .. }
        | KernelError::DegenerateSegment { .. }
        | KernelError::DegenerateLoop(_)
        | KernelError::SelfIntersection { .. }
        | KernelError::InvalidFrame
        | KernelError::InvalidDistance
        | KernelError::InvalidInput(_) => Refusal::invalid(reason),
        _ => Refusal::failed(reason),
    }
}

/// The protocol's frame as anvil-brep's.
fn frame(f: &model::Frame) -> Frame {
    Frame {
        origin: v3(f.origin),
        x: v3(f.x),
        y: v3(f.y),
        z: v3(f.z),
    }
}

/// A profile's loops as anvil-brep's: lines and circular arcs, which it draws the same way.
fn loops(p: &profile::Profile) -> Result<Vec<Loop2>, Refusal> {
    p.loops
        .iter()
        .map(|lp| {
            lp.iter()
                .map(|s| match *s {
                    profile::Seg::Line { a, b } => Ok(Segment::Line {
                        a: DVec2::from_array(a),
                        b: DVec2::from_array(b),
                    }),
                    profile::Seg::Arc {
                        center,
                        radius,
                        start,
                        sweep,
                    } => Ok(Segment::Arc {
                        center: DVec2::from_array(center),
                        radius,
                        start,
                        sweep,
                    }),
                    _ => Err(Refusal::unsupported(
                        "anvil-brep's profiles are lines and circular arcs",
                    )),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Loop2::new)
        })
        .collect()
}

/// The regions anvil-brep finds in a profile.
fn regions(p: &profile::Profile) -> Result<Vec<Region>, Refusal> {
    build_regions(&loops(p)?).map_err(err)
}

/// The one region a constructor that takes one is given.
fn region(p: &profile::Profile) -> Result<Region, Refusal> {
    let mut r = regions(p)?;
    if r.len() != 1 {
        return Err(Refusal::unsupported(format!(
            "this anvil-brep constructor takes one region, and the profile makes {}",
            r.len()
        )));
    }
    Ok(r.remove(0))
}

/// A sweep's path in the frame as anvil-brep's segments in model space. The path starts at the
/// origin heading along z; an arc bends toward the current x about an axis along the current y,
/// `radius` from the path, which is a right-handed turn about that axis.
fn path(f: &model::Frame, segs: &[model::PathSeg]) -> Vec<PathSegment> {
    let (mut at, mut d, mut x, y) = (v3(f.origin), v3(f.z), v3(f.x), v3(f.y));
    segs.iter()
        .map(|s| match *s {
            model::PathSeg::Line { length } => {
                let b = at + d * length;
                let seg = PathSegment::Line { a: at, b };
                at = b;
                seg
            }
            model::PathSeg::Arc { radius, angle } => {
                let center = at + x * radius;
                let seg = PathSegment::Arc {
                    center,
                    axis: y,
                    start: at,
                    angle,
                };
                at = seg.end();
                let (s, c) = angle.sin_cos();
                (d, x) = (d * c + x * s, x * c - d * s);
                seg
            }
        })
        .collect()
}

fn make(p: &Primitive) -> Result<Vec<anvil_brep::Solid>, Refusal> {
    match p {
        Primitive::Extrude {
            frame: f,
            profile,
            height,
        } => anvil_brep::extrude::extrude_regions(&frame(f), &regions(profile)?, 0.0, *height)
            .map_err(err),
        Primitive::Revolve {
            frame: f,
            profile,
            angle,
        } => {
            // The profile's plane holds the frame's x, out from the axis, and its z, along it;
            // anvil-brep turns a positive angle toward that plane's normal, here −y, so the
            // protocol's right-handed turn about z is a negative one.
            let plane = Frame {
                origin: v3(f.origin),
                x: v3(f.x),
                y: v3(f.z),
                z: -v3(f.y),
            };
            let angle = if *angle >= TAU { TAU } else { -*angle };
            anvil_brep::revolve::revolve_regions(
                &plane,
                &regions(profile)?,
                DVec2::ZERO,
                DVec2::Y,
                angle,
            )
            .map_err(err)
        }
        Primitive::Loft {
            frame: f,
            profile,
            height,
            top,
            ..
        } => {
            let (bottom, top) = (loops(profile)?, loops(top)?);
            let ([bottom], [top]) = (&bottom[..], &top[..]) else {
                return Err(Refusal::unsupported(
                    "anvil-brep's loft takes one loop at each end",
                ));
            };
            let base = frame(f);
            let raised = Frame {
                origin: base.origin + base.z * *height,
                ..base
            };
            let a = Section::Loop {
                frame: base,
                outline: bottom.clone(),
            };
            let b = Section::Loop {
                frame: raised,
                outline: top.clone(),
            };
            Ok(vec![loft_ruled(&a, &b, [None, None]).map_err(err)?])
        }
        Primitive::Sweep {
            frame: f,
            profile,
            path: segs,
        } => anvil_brep::sweep::sweep(&frame(f), &region(profile)?, &path(f, segs)).map_err(err),
        _ => Err(Refusal::unsupported(
            "anvil-brep has no constructor of this kind",
        )),
    }
}

/// The one body an operation on a solid is given: anvil-brep's operations take one.
fn body(s: &Solid) -> Result<&anvil_brep::Solid, Refusal> {
    match &s.0[..] {
        [b] => Ok(b),
        [] => Err(Refusal::unsupported(
            "anvil-brep has no empty solid to work on",
        )),
        _ => Err(Refusal::unsupported(
            "anvil-brep's operations take one body, and this solid is several",
        )),
    }
}

/// The edge each point names, by anvil-brep's own nearest-edge query at `10·ε`.
fn edges_at(
    b: &anvil_brep::Solid,
    points: &[model::Vec3],
) -> Result<Vec<(anvil_brep::topo::EdgeId, u32)>, Refusal> {
    points
        .iter()
        .map(|&p| {
            edge_near(b, v3(p), NAMES)
                .map(|e| (e, 0))
                .ok_or_else(|| Refusal::invalid(format!("no edge passes through {p:?}")))
        })
        .collect()
}

/// The face each point lies on, by anvil-brep's own test of a point on a face.
fn faces_at(b: &anvil_brep::Solid, points: &[model::Vec3]) -> Result<Vec<usize>, Refusal> {
    points
        .iter()
        .map(|&p| {
            let on: Vec<usize> = (0..b.faces.len())
                .filter(|&f| face_contains(b, f, v3(p)))
                .collect();
            match on[..] {
                [f] => Ok(f),
                [] => Err(Refusal::invalid(format!("no face passes through {p:?}"))),
                _ => Err(Refusal::unsupported(format!(
                    "{p:?} lies on {} faces, and anvil-brep names one",
                    on.len()
                ))),
            }
        })
        .collect()
}

/// Every body's tessellation at `chord`.
fn meshes(s: &Solid, chord: f64) -> Result<Vec<anvil_brep::tess::TriMesh>, Refusal> {
    let params = TessParams {
        chord,
        max_angle: tol::MAX_ANGLE,
    };
    s.0.iter()
        .map(|b| tessellate(b, &params).map_err(err))
        .collect()
}

/// The mass properties of each body, integrated exactly on its own surfaces.
fn properties(s: &Solid) -> Result<Vec<mass::MassProperties>, Refusal> {
    s.0.iter()
        .map(|b| mass::exact_properties(b).map_err(err))
        .collect()
}

impl Anvil {
    fn blend(&self, s: &Solid, edges: &[model::Vec3], kind: Blend) -> Result<Solid, Refusal> {
        let b = body(s)?;
        // The named edges alone, in one call: following a tangent chain is the kernel's to do.
        Ok(Solid(
            blend_chain(b, &edges_at(b, edges)?, kind, 1).map_err(err)?,
        ))
    }

    fn boolean_of(&self, a: &Solid, b: &Solid, op: BoolOp) -> Result<Solid, Refusal> {
        Ok(Solid(boolean(body(a)?, body(b)?, op).map_err(err)?))
    }
}

/// One size for every edge, where the kernel takes one size per call.
fn one_size(sizes: &[f64], what: &str) -> Result<f64, Refusal> {
    match sizes.split_first() {
        Some((&first, rest)) if rest.iter().all(|&x| x == first) => Ok(first),
        _ => Err(Refusal::unsupported(format!(
            "anvil-brep blends with one {what} per call"
        ))),
    }
}

impl brep_protocol::Kernel for Anvil {
    type Solid = Solid;

    fn name(&self) -> String {
        "anvil-brep".into()
    }

    fn version(&self) -> String {
        "0.2.0".into()
    }

    fn tolerance(&self) -> f64 {
        tol::LINEAR
    }

    fn configuration(&self) -> String {
        "no kernel setting changed; volume, area and centroid are anvil-brep's exact mass \
         properties, integrated on each body's own surfaces (mass::exact_properties)"
            .into()
    }

    fn make(&mut self, p: &Primitive) -> Result<Solid, Refusal> {
        make(p).map(Solid)
    }

    fn union(&mut self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        self.boolean_of(a, b, BoolOp::Union)
    }

    fn intersection(&mut self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        self.boolean_of(a, b, BoolOp::Intersect)
    }

    fn difference(&mut self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        self.boolean_of(a, b, BoolOp::Subtract)
    }

    fn volume(&mut self, s: &Solid) -> Result<f64, Refusal> {
        Ok(properties(s)?.iter().map(|m| m.volume).sum())
    }

    fn area(&mut self, s: &Solid) -> Result<f64, Refusal> {
        Ok(properties(s)?.iter().map(|m| m.area).sum())
    }

    fn centroid(&mut self, s: &Solid) -> Result<Option<model::Vec3>, Refusal> {
        let ms = properties(s)?;
        let volume: f64 = ms.iter().map(|m| m.volume).sum();
        if ms.is_empty() || volume == 0.0 {
            return Ok(None);
        }
        let moment: DVec3 = ms.iter().map(|m| m.centroid * m.volume).sum();
        Ok(Some(arr(moment / volume)))
    }

    fn check(&mut self, s: &Solid) -> Result<Vec<String>, Refusal> {
        Ok(s.0
            .iter()
            .filter_map(|b| check::validate(b).err().map(|e| e.to_string()))
            .collect())
    }

    fn classify(&mut self, s: &Solid, points: &[model::Vec3]) -> Result<Vec<model::Side>, Refusal> {
        // anvil-brep's classifier says whether a body holds a point, its boundary counted in; it
        // has no answer of its own for a point on the boundary.
        points
            .iter()
            .map(|&p| {
                for b in &s.0 {
                    if contains_point(b, v3(p)).map_err(err)? {
                        return Ok(model::Side::In);
                    }
                }
                Ok(model::Side::Out)
            })
            .collect()
    }

    fn fillet(&mut self, s: &Solid, edges: &[model::Vec3], radius: f64) -> Result<Solid, Refusal> {
        self.blend(s, edges, Blend::Fillet(radius))
    }

    fn fillet_radii(
        &mut self,
        s: &Solid,
        edges: &[model::Vec3],
        radii: &[f64],
    ) -> Result<Solid, Refusal> {
        self.blend(s, edges, Blend::Fillet(one_size(radii, "radius")?))
    }

    fn chamfer(
        &mut self,
        s: &Solid,
        edges: &[model::Vec3],
        distance: f64,
    ) -> Result<Solid, Refusal> {
        self.blend(s, edges, Blend::Chamfer(distance))
    }

    fn chamfer_distances(
        &mut self,
        s: &Solid,
        edges: &[model::Vec3],
        distances: &[f64],
    ) -> Result<Solid, Refusal> {
        self.blend(s, edges, Blend::Chamfer(one_size(distances, "distance")?))
    }

    fn chamfer_two(
        &mut self,
        s: &Solid,
        edge: model::Vec3,
        face: model::Vec3,
        d1: f64,
        d2: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let along = faces_at(b, &[face])?[0];
        self.blend(
            s,
            &[edge],
            Blend::ChamferDistances {
                distance: d1,
                other: d2,
                along,
            },
        )
    }

    fn offset_faces(
        &mut self,
        s: &Solid,
        faces: &[model::Vec3],
        distance: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let mut by = vec![0.0; b.faces.len()];
        for f in faces_at(b, faces)? {
            by[f] = distance;
        }
        Ok(Solid(vec![
            anvil_brep::offset::offset_faces(b, &by).map_err(err)?,
        ]))
    }

    fn shell(&mut self, s: &Solid, open: &[model::Vec3], thickness: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        Ok(Solid(
            anvil_brep::shell::shell(b, &faces_at(b, open)?, thickness).map_err(err)?,
        ))
    }

    fn draft(
        &mut self,
        s: &Solid,
        faces: &[model::Vec3],
        origin: model::Vec3,
        pull: model::Vec3,
        angle: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let n = v3(pull);
        let hint = if n.cross(DVec3::Z).length() > 1e-9 * n.length() {
            DVec3::Z
        } else {
            DVec3::X
        };
        let neutral = Frame::new(v3(origin), n, hint.cross(n)).map_err(err)?;
        Ok(Solid(vec![
            anvil_brep::draft::draft_faces(b, &faces_at(b, faces)?, &neutral, angle)
                .map_err(err)?,
        ]))
    }

    fn transform(&mut self, s: &Solid, m: [[f64; 4]; 3]) -> Result<Solid, Refusal> {
        // anvil-brep moves a solid rigidly in one call; a mirror or a scale is another call, and
        // a motion that is both is two.
        let col = |j: usize| DVec3::new(m[0][j], m[1][j], m[2][j]);
        let (x, y, z) = (col(0), col(1), col(2));
        let rigid = [(x, x), (y, y), (z, z)]
            .iter()
            .all(|(a, b)| (a.dot(*b) - 1.0).abs() <= 1e-12)
            && x.dot(y).abs() <= 1e-12
            && y.dot(z).abs() <= 1e-12
            && z.dot(x).abs() <= 1e-12
            && x.cross(y).dot(z) > 0.0;
        if !rigid {
            return Err(Refusal::unsupported(
                "anvil-brep moves a solid rigidly in one call, and mirrors or scales it in others",
            ));
        }
        let motion = Frame {
            origin: col(3),
            x,
            y,
            z,
        };
        Ok(Solid(s.0.iter().map(|b| b.transformed(&motion)).collect()))
    }

    fn write_step(&mut self, s: &Solid) -> Result<String, Refusal> {
        Ok(anvil_step::export::write(&s.0, "anvil-brep solid"))
    }

    fn mesh(&mut self, s: &Solid, tolerance: f64) -> Result<Mesh, Refusal> {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for m in meshes(s, tolerance)? {
            let base = positions.len() as u32;
            positions.extend(m.positions.iter().map(|&p| arr(p)));
            triangles.extend(m.triangles.iter().map(|t| t.map(|i| i + base)));
        }
        Ok(Mesh::new(positions, triangles))
    }

    fn supports(&self) -> Vec<String> {
        [
            "extrude",
            "revolve",
            "loft",
            "sweep",
            "union",
            "intersection",
            "difference",
            "fillet",
            "fillet_radii",
            "chamfer",
            "chamfer_distances",
            "chamfer_two",
            "offset_faces",
            "shell",
            "draft",
            "transform",
            "write_step",
            "volume",
            "area",
            "centroid",
            "classify",
            "check",
            "mesh",
        ]
        .map(String::from)
        .to_vec()
    }
}

fn main() -> ExitCode {
    match brep_protocol::serve(&mut Anvil) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
