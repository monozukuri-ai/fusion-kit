"""Cross-surface contracts rather than additional format grammar tests."""

import ast
import gc
import importlib.resources
import inspect
import json
import subprocess
import sys
import threading
import time
from types import UnionType
from typing import (
    ForwardRef,
    Literal,
    Union,
    get_args,
    get_origin,
    get_type_hints,
    is_typeddict,
)

import fusion_kit as fusion
import pytest
from fusion_kit import types as schema

from .manifest_fixtures import document, text
from .package_fixtures import package, values
from .zip_fixtures import make_zip


def validate(value, annotation):
    """Check the published JSON schema against real native return values."""
    if isinstance(annotation, ForwardRef):
        assert annotation.__forward_arg__ == "JSONValue"
        annotation = schema.JSONValue
    elif annotation == "JSONValue":
        annotation = schema.JSONValue
    origin, args = get_origin(annotation), get_args(annotation)
    if is_typeddict(annotation):
        fields = get_type_hints(annotation)
        assert isinstance(value, dict) and value.keys() == fields.keys()
        for name, field in fields.items():
            validate(value[name], field)
    elif origin in (UnionType, Union):
        for member in args:
            try:
                validate(value, member)
                return
            except AssertionError:
                pass
        raise AssertionError((value, annotation))
    elif origin is Literal:
        assert any(type(value) is type(v) and value == v for v in args)
    elif origin is list:
        assert isinstance(value, list)
        for member in value:
            validate(member, args[0])
    elif origin is dict:
        assert isinstance(value, dict)
        for key, member in value.items():
            validate(key, args[0])
            validate(member, args[1])
    else:
        assert type(value) is annotation, (value, annotation)


def model_instances():
    archive = fusion.read(document())
    doc = archive.read_document()
    pkg_archive = fusion.read(package())
    pkg = pkg_archive.read_package()
    return [
        fusion.Limits(),
        archive,
        archive.members[0],
        archive.verify(),
        doc,
        doc.manifest,
        doc.manifest.tail,
        doc.manifest.tail.terminal,
        doc.manifest.generation_extension,
        doc.assets[0],
        doc.assets[0].segments[0],
        doc.assets[0].segments[0].instances[0],
        pkg,
        pkg.documents[0],
        pkg.references[0],
        pkg.graphs[0],
        pkg.graphs[0].nodes[0],
        pkg.redirections[0],
        pkg.comparisons[0],
        pkg.raw_sidecars[0],
        fusion.read(document(version="99"), strict=False).read_document().issues[0],
    ]


def test_json_types_match_native_shapes_and_copies_are_independent():
    archive = fusion.read(document())
    package_archive = fusion.read(package())
    for view, contract in [
        (archive, schema.ArchiveJSON),
        (archive.read_document(), schema.DocumentJSON),
        (package_archive.read_package(), schema.PackageJSON),
        (package_archive.verify(), schema.ValidationJSON),
    ]:
        result = view.to_dict()
        validate(result, contract)
        assert result == json.loads(view.to_json())
        result["issues"].append({"mutated": True})
        assert view.to_dict()["issues"] == []
    sidecar = package_archive.read_package().raw_sidecars[0]
    sidecar.value["mutated"] = True
    assert "mutated" not in sidecar.value


def check_signature(func, callable_object):
    signature = inspect.signature(callable_object)
    pos = [a.arg for a in func.args.posonlyargs + func.args.args if a.arg != "self"]
    keyword = [a.arg for a in func.args.kwonlyargs]
    assert list(signature.parameters) == pos + keyword
    for name in keyword:
        assert signature.parameters[name].kind == inspect.Parameter.KEYWORD_ONLY
    defaults = func.args.defaults + func.args.kw_defaults
    expected = {
        a.arg: ast.literal_eval(default)
        for a, default in zip(
            (func.args.args[-len(func.args.defaults) :] if func.args.defaults else [])
            + func.args.kwonlyargs,
            defaults,
            strict=True,
        )
        if default is not None
    }
    for name, value in expected.items():
        assert signature.parameters[name].default == value


def test_stub_and_native_public_members_signatures_and_readonly_views_agree():
    source = importlib.resources.files("fusion_kit").joinpath("_native.pyi").read_text()
    classes = {
        node.name: node
        for node in ast.parse(source).body
        if isinstance(node, ast.ClassDef)
    }
    for func in ast.parse(source).body:
        if isinstance(func, ast.FunctionDef):
            check_signature(func, getattr(fusion, func.name))
    constructor = next(
        f
        for f in classes["Limits"].body
        if isinstance(f, ast.FunctionDef) and f.name == "__init__"
    )
    check_signature(constructor, fusion.Limits)
    instances = model_instances()
    assert {type(obj).__name__ for obj in instances} == {
        node.name
        for node in classes.values()
        if any(isinstance(d, ast.Name) and d.id == "final" for d in node.decorator_list)
    }
    for obj in instances:
        cls = type(obj)
        node = classes[cls.__name__]
        funcs = [f for f in node.body if isinstance(f, ast.FunctionDef)]
        public = {f.name for f in funcs if not f.name.startswith("_")}
        assert public == {name for name in dir(cls) if not name.startswith("_")}
        for func in funcs:
            if func.name.startswith("_"):
                continue
            if func.decorator_list:
                with pytest.raises(AttributeError):
                    setattr(obj, func.name, None)
            else:
                check_signature(func, getattr(obj, func.name))
        if cls is not fusion.Limits:
            with pytest.raises(TypeError):
                cls()
        with pytest.raises(TypeError):
            type("Subclass", (cls,), {})


def test_nested_diagnostic_locations_and_all_issues_survive_strict_failure():
    manifest, desc, tables = values()
    tables["root.f3d"]["references"][0]["properties"][0]["neutronRole"]["value"] = (
        "other"
    )
    archive = fusion.read(package((manifest, desc, tables)))
    info = archive.read_package(strict=False)
    note = next(d for d in info.issues if d.code == "reference_role_mismatch")
    assert (
        note.member == "root.f3d"
        and note.source == archive.source
        and note.context == "root.f3d"
    )
    with pytest.raises(fusion.ReferenceResolutionError) as exc:
        archive.read_package()
    error = exc.value
    assert error.status == "invalid" and error.exit_code == 1
    assert error.result["issues"] == info.to_dict()["issues"]
    assert [i.code for i in error.issues] == [i.code for i in info.issues]
    validate(error.result, schema.ErrorJSON)
    bad = values()
    bad[2]["root.f3d"]["references"][0]["properties"] = []
    infos = fusion.read(package(bad), strict=False).read_package().issues
    prop = next(d for d in infos if d.code == "reference_property")
    assert prop.member == "RedirectionsStream.dat"
    assert (
        prop.source.endswith("!root.f3d")
        and prop.context == "root.f3d!RedirectionsStream.dat"
    )
    graph = fusion.read(package(({}, desc, tables)), strict=False).read_package()
    graph_note = next(d for d in graph.issues if d.code == "root_missing")
    assert graph_note.member == graph_note.context == "Manifest.json"
    bad = values()
    bad[1]["designDescription"]["designGraphs"][0]["designObjects"][1][
        "contentType"
    ] = "f2d"
    graph = fusion.read(package(bad), strict=False).read_package()
    graph_note = next(
        d for d in graph.issues if d.code == "object_content_type_mismatch"
    )
    assert graph_note.member == "DesignDescription.json"
    assert graph_note.context == "DesignDescription.json.graph[0]"


def test_strict_partial_errors_retain_status_source_offset_and_nested_diagnostics():
    archive = fusion.read(document(ending=b"\x01" + text("", True)))
    result = archive.read_document(strict=False)
    with pytest.raises(fusion.UnsupportedFeatureError) as exc:
        archive.read_document()
    error = exc.value
    assert error.status == "partial" and error.exit_code == 2
    assert error.context is None and error.offset_basis == "member"
    assert error.result["issues"] == result.to_dict()["issues"]
    assert result.manifest.issues[0].source == archive.source
    partial = fusion.read(document(revision=99), strict=False).read_document()
    assert partial.assets[0].issues[0].source == "<bytes>"
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.get_preview(document_id="missing")
    # strict metadata failure precedes lookup, use a complete source below.


def test_lookup_span_and_io_errors_have_consistent_details(tmp_path):
    archive = fusion.read(document())
    for operation, cls, code in [
        (
            lambda: archive.get_preview(document_id="missing"),
            KeyError,
            "document.not_found",
        ),
        (lambda: archive.read_member("missing"), KeyError, "zip.member_not_found"),
        (
            lambda: fusion.readfile(tmp_path / "missing.f3d"),
            FileNotFoundError,
            "source.io",
        ),
        (
            lambda: fusion.read(document(preview=b"different")).read_span(
                archive.read_document().manifest.generation_extension
            ),
            fusion.FormatError,
            "span.source_mismatch",
        ),
    ]:
        with pytest.raises(cls) as exc:
            operation()
        assert (
            exc.value.code == code
            and exc.value.status == "invalid"
            and exc.value.exit_code == 1
        )
        assert exc.value.source is not None
        validate(exc.value.result, schema.ErrorJSON)


def test_explicit_strict_overrides_do_not_change_parent_policy():
    archive = fusion.read(package(values("FUTURE")))
    assert archive.read_package(strict=False).root_document_id == "root.f3d"
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.get_preview()
    assert archive.get_preview(strict=False) is not None
    child = archive.open_document("root.f3d", strict=False)
    assert child.read_document().status == "complete"
    with pytest.raises(fusion.UnsupportedFeatureError):
        archive.open_document("root.f3d")
    with pytest.raises(fusion.UnsupportedFeatureError):
        _ = archive.root_document_id


@pytest.mark.parametrize(
    "arguments",
    [
        [],
        ["verify"],
        ["inspect", "missing", "--invalid"],
        ["inspect", "missing", "--document", "--package"],
        ["verify", "missing", "--max-entries", "no"],
        ["verify", "missing", "--max-entries", "0"],
        ["verify", "missing", "--max-input-bytes", "-1"],
    ],
)
def test_cli_argument_errors_are_json_invalid_and_exit_one(arguments):
    done = subprocess.run(
        [sys.executable, "-I", "-m", "fusion_kit", *arguments, "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode == 1 and done.stderr == ""
    payload = json.loads(done.stdout)
    validate(payload, schema.ErrorJSON)
    assert (
        payload["status"] == "invalid"
        and payload["issues"][0]["code"] == "configuration.invalid"
    )


def test_verify_error_report_and_error_json_agree():
    raw = bytearray(document())
    archive = fusion.read(bytes(raw))
    offset = next(
        m.data_offset for m in archive.members if m.name.endswith("BulkStream.dat")
    )
    raw[offset] ^= 1
    broken = fusion.read(bytes(raw))
    with pytest.raises(fusion.FormatError) as exc:
        broken.verify()
    error = exc.value
    assert error.report.to_dict() == error.result["report"]
    assert error.report.exit_code == error.exit_code == 1
    assert error.report.nested_archives == "not_requested"
    assert error.offset_basis == "archive"
    assert [i.code for i in error.issues] == [i.code for i in error.report.issues]
    assert error.report.to_dict() == json.loads(error.report.to_json())


def test_native_work_allows_python_thread_progress_without_a_python_timeslice():
    raw = make_zip(
        [("Manifest.dat", b"m"), ("large.bin", b"x" * (32 * 1024 * 1024))], method=8
    )[0]
    archive = fusion.read(raw)
    ready, stop = threading.Event(), threading.Event()
    ticks = [0]

    def background():
        ready.wait()
        while not stop.is_set():
            ticks[0] += 1
            time.sleep(0)

    worker = threading.Thread(target=background, daemon=True)
    interval = sys.getswitchinterval()
    try:
        sys.setswitchinterval(60)
        worker.start()
        ready.set()
        report = archive.verify()
        stop.set()
        during = ticks[0]  # sampled before join can release the GIL
    finally:
        stop.set()
        sys.setswitchinterval(interval)
        worker.join(timeout=5)
    assert not worker.is_alive() and report.status == "complete"
    assert during > 0
    del archive
    gc.collect()
    assert report.members_verified == 2
