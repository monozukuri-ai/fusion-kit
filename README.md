# fusion-kit

fusion-kit is a Python package for reading Autodesk Fusion F3D and F3Z files locally. It exposes a Rust parser through a Python API and a command-line tool. Both run independently of Autodesk Fusion and cloud services.

- List and read ZIP members, and verify their uncompressed sizes and CRCs. Supports Stored, Deflate, and Zstandard compression.
- Read F3D manifests, assets, segments, and preview images.
- List documents in an F3Z package, identify the root document, and inspect and validate XREF and DERIVED references.

## Installation

fusion-kit requires standard CPython 3.11 or later. Install the wheel for your platform, or build from source with Rust and a C compiler. [Supported platforms and installation](docs/distribution.md)

## Usage

```python
import fusion_kit

archive = fusion_kit.readfile("model.f3d")
document = archive.read_document()
if document.manifest is not None:
    print(document.manifest.name)
report = archive.verify()
print(report.status, report.members_verified)
```

```bash
fusion-kit inspect model.f3d --document --json
fusion-kit inspect assembly.f3z --package --json
fusion-kit verify assembly.f3z --json
```

F3Z verification includes the embedded F3D archives. To verify only the outer ZIP, pass `--outer-only` on the command line or call `verify(recursive=False)` in Python. [Python API, CLI, and JSON reference](docs/api.md) · [Python example](examples/inspect_fusion.py)

## Scope

fusion-kit provides read-only access to archive structure and document metadata. F3Z document references describe dependencies between designs. Geometry, B-rep, component placements, design history, and STEP or GLB conversion are outside the current scope.

Opening an archive reads its ZIP index. Call `verify()` to check every member's CRC and size.

## License

fusion-kit is licensed under the MIT License. Distributions include dependency notices and credits for the format references. [MIT License](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.txt) · [Rust standard library notices](RUST_STDLIB_COPYRIGHT.html)
