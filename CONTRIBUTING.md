# Contributing

Issues and pull requests are welcome, and most of all from AnvilCAD's maintainers. The suite's
results come only from commits that the suite's maintainers have reviewed as pass-through.

## What belongs here

- **Corrections.** A place where the adapter drives anvil-brep wrongly, or calls something other
  than the kernel's one call for a function.
- **New calls.** A function the adapter lists as unsupported that a newer anvil-brep release now
  offers in one call, such as an exact volume.

What does not belong: work the kernel does not do itself. The adapter is pass-through, as the
suite's
[contract](https://github.com/springcraft-ai/brep-conformance/blob/main/doc/contract.md#what-an-adapter-may-do)
asks. A function assembled here out of several kernel calls would score this code and not
anvil-brep, so it stays unsupported.

A problem with a test, a check or the protocol belongs in
[brep-conformance](https://github.com/springcraft-ai/brep-conformance/issues). A problem in
anvil-brep itself belongs with [AnvilCAD](https://github.com/swperb/anvilcad/issues).

## Licence of contributions

Contributions are accepted under this repository's [MIT licence](LICENSE).

anvil-brep has a licence of its own, which the [README](README.md#licences) describes. Call the
kernel; do not copy its source, or any other project's, into this repository.
