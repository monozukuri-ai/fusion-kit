"""Exercise the installed Rust extension and its distribution contract."""

import importlib.machinery
import importlib.metadata
import importlib.resources
import tomllib
from pathlib import Path

import fusion_kit
import pytest
from fusion_kit import Limits, _native


def test_distribution_binding_and_core_share_workspace_version():
    manifest = Path(__file__).resolve().parents[2] / "Cargo.toml"
    expected = tomllib.loads(manifest.read_text())["workspace"]["package"]["version"]
    assert fusion_kit.__version__ == expected
    assert importlib.metadata.version("fusion-kit") == expected
    assert _native.__version__ == expected
    assert _native.__core_version__ == expected


def test_native_extension_and_typing_files_are_installed():
    assert any(
        _native.__file__.endswith(suffix)
        for suffix in importlib.machinery.EXTENSION_SUFFIXES
    )
    package = importlib.resources.files("fusion_kit")
    for name in ("py.typed", "__init__.pyi", "_native.pyi"):
        assert package.joinpath(name).is_file(), name
    assert fusion_kit.Limits is _native.Limits


def test_resource_policy_defaults_and_keyword_overrides():
    default = Limits()
    assert default.max_input_bytes == 536_870_912
    assert default.max_member_bytes == 268_435_456
    assert default.max_total_uncompressed_bytes == 1_073_741_824
    assert default.max_entries == 100_000
    assert default.max_nesting_depth == 2

    custom = Limits(max_input_bytes=1 << 40, max_entries=7, max_nesting_depth=1)
    assert custom.max_input_bytes == 1 << 40
    assert custom.max_entries == 7
    assert custom.max_nesting_depth == 1
    assert custom.max_member_bytes == default.max_member_bytes


@pytest.mark.parametrize(
    "field",
    [
        "max_input_bytes",
        "max_member_bytes",
        "max_total_uncompressed_bytes",
        "max_entries",
        "max_nesting_depth",
    ],
)
def test_invalid_policy_is_reported_as_value_error(field):
    with pytest.raises(ValueError, match=field):
        Limits(**{field: 0})


def test_member_and_total_budget_validation_reaches_python():
    with pytest.raises(ValueError, match="max_member_bytes"):
        Limits(max_member_bytes=3, max_total_uncompressed_bytes=2)
    valid = Limits(max_member_bytes=2, max_total_uncompressed_bytes=2)
    assert valid.max_member_bytes == valid.max_total_uncompressed_bytes == 2


def test_resource_policy_is_read_only():
    limits = Limits()
    with pytest.raises(AttributeError):
        limits.max_entries = 5
