# Python API and JSON reference

fusion-kit v0.1 provides a read-only API for F3D and F3Z files on standard CPython 3.11 and later. Use `fusion_kit.readfile(path)` or `fusion_kit.read(data)` to open an archive. Construct `Limits` directly to configure resource limits. The reader creates the result objects as immutable views.

## Inputs and operations

```python
from pathlib import Path
import fusion_kit

archive = fusion_kit.readfile(Path("assembly.f3z"), strict=False)
info = archive.read_package()
for doc in info.documents:
    print(doc.id, doc.document_guid, doc.status)
    if doc.metadata is not None and doc.metadata.manifest is not None:
        print(doc.metadata.manifest.name)
if info.root_document_id is not None:
    child = archive.open_document(info.root_document_id, strict=False)
    preview = child.get_preview(strict=False)
report = archive.verify(strict=False)
print(info.exit_code, report.exit_code)
```

See the runnable examples for [document and package JSON](../examples/inspect_fusion.py) and [previews for individual documents](../examples/package_members.py).

| Operation | Scope and return value |
|---|---|
| `readfile(str / PathLike[str])` or `read(bytes)` | An owned snapshot and ZIP index. In-memory input must be immutable `bytes`. |
| `archive.members` or `archive.read_member(name)` | List the outer ZIP members or return one member as `bytes`, checking decompression limits, size, and CRC. |
| `archive.read_document()` | F3D `DocumentInfo` with the manifest, assets, segments, and spans for unparsed regions. |
| `archive.read_package()` | F3Z `PackageInfo` with the root, document dependencies, and checks against the redirection data. |
| `archive.documents` | `list[DocumentInfo]` for F3D or `list[PackageDocument]` for F3Z. |
| `archive.references` or `archive.root_document_id` | Package references or the root document ID. Raises an exception for unsupported scopes. |
| `archive.open_document(id, *, strict=None)` | The child `Archive` for a member path in the F3Z package. |
| `archive.get_preview(*, document_id=None, strict=None)` | Image bytes for the root or selected document. Returns `None` for an absent preview and `b""` for an empty member. |
| `archive.read_span(region)` | The original bytes of an unparsed region after checking the source hash, byte range, and region hash. |
| `archive.verify(*, strict=True, recursive=True)` | ZIP verification, including embedded archives in an F3Z. Pass `recursive=False` to verify only the outer ZIP. |

`readfile()` and `read()` build the ZIP index. The reader parses document and package metadata on demand. Use `verify()` for CRC checks and `read_package()` for dependency checks. `archive.status`, `archive.issues`, and `archive.to_dict()` describe the index. Read operation-specific diagnostics through `read_document().issues`, `read_package().issues`, or `verify().issues`.

An F3D document uses its Fusion GUID as its ID. An F3Z `PackageDocument.id` is a member path scoped to `package_sha256`. The Fusion GUID is available as `document_guid`. A numeric graph ID is scoped to its `graph_index`. `PackageInfo.root_document_id` is `None` when the root is unresolved. Document and reference counts describe dependencies between designs.

## Strict mode and status

| Setting | Behavior |
|---|---|
| `readfile/read(strict=True)` | Raises for unsupported entries encountered during ZIP indexing. Metadata is parsed on demand. |
| `read_document/read_package/open_document/get_preview(strict=None)` | Inherits the setting from the archive. An explicit Boolean applies to that operation. |
| `documents/references/root_document_id` | Uses the archive's strict setting. |
| `verify(strict=True)` | Uses its own strict setting. Verification failures raise an exception with the available report. |

A child archive created by `open_document()` inherits the strict setting selected for that call. The parent retains its settings and cached data. With `strict=False`, the reader can return an `invalid` result for inconsistent references when it can safely examine them. Corrupt or unreadable inputs and resource-limit failures can still raise exceptions.

`None` represents an unparsed collection. An empty list represents a parsed collection with no items. A layer omitted from the request has status `not_requested`.

| Status | Exit code | Meaning |
|---|---:|---|
| `complete` | 0 | The requested operation completed. |
| `partial` | 2 | The reader retained partial results or reported a warning. |
| `unsupported` | 2 | The input uses an unsupported schema or feature. |
| `invalid` | 1 | The reader detected corruption or an integrity failure. |

Result views and exceptions use the same exit-code mapping. Use the reported status for the entire operation.

## Exceptions and diagnostics

`FormatError`, `UnsupportedFeatureError`, `ReferenceResolutionError`, and `ResourceLimitError` derive from `FusionError`. Missing files and permission failures raise Python's `FileNotFoundError` and `PermissionError`. Unknown members or document IDs raise `KeyError`. Invalid `Limits` values raise `ValueError` or `OverflowError`. With `--json`, the CLI reports file I/O and argument errors as `invalid` results.

Exceptions from parsing, member reads, and verification expose the attributes below. Argument type errors and invalid `Limits` settings use Python's argument exceptions. Use `getattr()` when accessing additional attributes on built-in exceptions.

| Attributes | Meaning |
|---|---|
| `code`, `stage`, `source`, `member`, `offset`, `offset_basis`, `context`, `message` | Fields of the primary diagnostic. |
| `diagnostic`, `issues` | The primary diagnostic as a dictionary and a read-only view of the available diagnostics. |
| `status`, `exit_code` | Status and exit code of the requested operation. Strict mode preserves `partial` and exit code 2 when it raises for a partial result. |
| `report` | The verification report collected so far, or `None` when unavailable. |
| `result` | JSON error representation with `schema_version`, `status`, `issues`, and `report`. |

`member` identifies an archive member or the expected name of a missing member. `context` describes a logical location in JSON or a graph. Treat it as diagnostic text. For example, a diagnostic from an embedded redirection stream can have `source="parent!child.f3d"` and `member="RedirectionsStream.dat"`. Use the source SHA-256 and package member ID for persistent identification. The `source` string is a display name.

Offsets are zero-based. An `offset_basis` of `"archive"` identifies a ZIP header or the start of a compressed payload. A basis of `"member"` identifies a cursor or candidate location in the decompressed manifest. Both fields are `None` when a precise position is unavailable. The offset identifies where the reader detected the issue.

## JSON and typing

`to_dict()` returns a detached copy. `to_json()` returns a JSON string with the same schema. You can edit the returned dictionaries and lists independently of the reader.

`fusion_kit.types` exports literals such as `Status` and `ExitCode`, and typed dictionaries including `ArchiveJSON`, `DocumentJSON`, `PackageJSON`, `ValidationJSON`, and `ErrorJSON`. The current `schema_version` is 1. Unparsed sidecar values use the recursive `JSONValue` type. Unknown reference kinds are retained as `str | None` alongside the original JSON.

## CLI

```bash
fusion-kit inspect model.f3d --json
fusion-kit inspect model.f3d --document --json
fusion-kit inspect assembly.f3z --package --json
fusion-kit verify assembly.f3z --json
fusion-kit verify assembly.f3z --outer-only --json
```

The CLI includes partial results when available and uses the exit codes in the status table. With `--json`, it writes one JSON result to stdout, including for argument errors. `--help` and `--version` print text and return exit code 0.

`--document` and `--package` are mutually exclusive. Argument errors return `invalid` and exit code 1.
