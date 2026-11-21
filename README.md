# brep-conformance-anvilcad

The [brep-conformance](https://github.com/springcraft-ai/brep-conformance) adapter for anvil-brep,
the exact B-rep kernel of [AnvilCAD](https://github.com/swperb/anvilcad): it puts anvil-brep behind
the suite's protocol, so that the suite can score it.

## Who maintains this

brep-conformance scores a B-rep kernel through an adapter: a small program that answers the suite's
requests with the kernel's own calls. Gustav Näslund wrote this adapter and maintains it.

- **It is not AnvilCAD's.** AnvilCAD's maintainers did not write it or choose the tests it runs.
- **A failure can be the adapter's.** A case anvil-brep fails in the results may be this adapter
  driving it wrongly. Corrections are welcome: open an issue or a pull request.
- **It adds nothing to the kernel.** The adapter is pass-through, as the suite's
  [contract](https://github.com/springcraft-ai/brep-conformance/blob/main/doc/contract.md#what-an-adapter-may-do)
  asks: one call of the kernel for each function, and anything the kernel has no one call for is
  listed as unsupported. What is scored is anvil-brep, not this code. The one exception, how it
  measures, is set out below.
- **AnvilCAD's maintainers are welcome here.** They know best how to drive anvil-brep, so their
  review and corrections carry the most weight. Ask in an issue to be added as a reviewer.
- **The suite decides what is scored.** An official run uses only a commit of this adapter that the
  suite's maintainers have reviewed as pass-through, whoever wrote it.

## Measuring

anvil-brep has no exact integration of volume, area or moments. Its mass properties
(`anvil_brep::mass::properties`) are those of a closed triangle mesh, and AnvilCAD measures a solid
by tessellating it (`anvil_brep::tess::tessellate`) and taking the mass properties of the mesh. The
adapter measures the same way. It tessellates each body at a chord of 1e-4 of its diagonal and 1°
per chord, and takes the volume, area and centroid of that mesh. The `configuration` in `hello`
says so.

The suite's own scope is kernels that measure exactly, at 1e-7 mm. A measure taken from a mesh is
off by roughly the chord's square over the radius on every curved face. So anvil-brep's curved
results fail at 1e-7 mm where the geometry is right, and the tier a case holds at (1e-6, 1e-5 or
1e-3) is the better guide. The checks that do not measure, such as the checker (B1), the classifier
against the suite's sample (B9, B15) and the refusals (B18), are read as they stand. This is the
one place the adapter is not one call per function. It is the open question in the issue that
proposes this adapter for the registry. Should anvil-brep gain an exact measure, the adapter will
call it.

## Overview

[`src/main.rs`](src/main.rs) answers each function with anvil-brep's one call for it, and lists the
rest as unsupported:

- **Constructors.** Extrusions with `extrude::extrude_regions`, revolutions with
  `revolve::revolve_regions` (the profile drawn in the plane of the frame's x and z, turned the
  protocol's way round), ruled lofts with `loft::loft_ruled` between the two profiles the suite
  sends, and sweeps with `sweep::sweep` along the path's lines and arcs, placed in model space.
  Profiles of lines and circular arcs are anvil-brep's own; splines, conics and blend sections are
  unsupported. The suite sends a box, cylinder, ball, ring or frustum as its own extrusion or
  revolution, since anvil-brep has no constructors for them.
- **Booleans.** `boolean::boolean`, whose list of bodies is the protocol's solid set; an empty list
  is the empty solid.
- **Blends.** `blend::blend_chain` on the named edges, all in one call, each found with
  `blend::edge_near` at the protocol's `10·ε`. Fillets and equal chamfers are its `Fillet` and
  `Chamfer`. A chamfer by two distances is its `ChamferAngle`: a setback `d1` along the named face,
  leaving it at `atan(d2/d1)`. Several radii or distances at once are unsupported, since a call
  takes one.
- **Changing a solid.** `offset::offset_faces` with the named faces' distance and 0 for the others,
  `shell::shell` inward, and `draft::draft_faces` about the neutral plane. Faces are found with
  `boolean::face_contains`; a point on several faces is unsupported.
- **Moving.** A rigid motion with `Solid::transformed`.
- **Queries.** The checker is `check::validate`. The classifier is `boolean::contains_point`, which
  counts the boundary in and has no answer of its own for a point on it. The mesh is
  `tess::tessellate` at the asked chord. STEP is `anvil_step::export::write`, AnvilCAD's AP214
  writer.

Listed as unsupported, since anvil-brep 0.1.2 has no one call of the protocol's shape for it:

- The five primitives (the suite sends their stand-ins), screws, lofts through several sections,
  sweeps along a spline, and slabs.
- The booleans of many, and an operation on an empty or several-piece solid: each call takes one
  body.
- Variable, conic, curvature-continuous and set-back fillets, fillets between faces, full rounds,
  and blends of several sizes.
- Rounded offsets (`offset_faces` keeps the topology and its edges sharp), face deletes, moves and
  thickens.
- A mirror, a scale, a stretch or a shear: `mirror::mirrored` and `scale::scaled` are calls of their
  own, and a motion that turns and mirrors or scales would be two.
- Inertia, distance, interference, Betti numbers, sections, projections, splits, rays, normals,
  curvatures, nearest points, bounding boxes (`Solid::bounds` pads curved faces), displays, sewing
  and reading STEP.

anvil-brep's tolerance is its own constant `tol::LINEAR`, 1e-6 mm, which it offers no setting for.
The adapter declares it and changes nothing.

## Setup

The Rust toolchain [`rust-toolchain.toml`](rust-toolchain.toml) pins. Building fetches anvil-brep
and anvil-step from AnvilCAD's repository at release 0.1.2, the commit
[`Cargo.toml`](Cargo.toml) pins.

## Usage

The suite runs the adapter as a separate process. Build the adapter here, then run the suite from a
checkout of [brep-conformance](https://github.com/springcraft-ai/brep-conformance) beside this one:

```bash
cargo build --release
cd ../brep-conformance
cargo build --release -p brep-conformance
target/release/brep-conformance --adapter ../brep-conformance-anvilcad/target/release/anvilcad-adapter \
    --out anvil-brep.json
```

## Licences

The adapter in this repository is under the MIT licence: see [LICENSE](LICENSE).

That licence covers the adapter and nothing else. It does not cover the kernel, and nothing here
relicenses the kernel.

anvil-brep and anvil-step are separate works under their own licence. AnvilCAD's workspace declares
`MIT OR Apache-2.0`; its repository is the authority on that, not this page.

Building the adapter downloads anvil-brep and anvil-step from AnvilCAD's repository and links them
in. A built adapter therefore contains them and is subject to their terms as well as this
repository's.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
