"""Small F3Z sidecars and F3Ds generated independently of the Rust parser."""

import json

from .manifest_fixtures import document
from .zip_fixtures import make_zip


def values(kind="XREF"):
    manifest = {"root": "root.f3d"}
    description = {
        "name": "Autodesk Design Description",
        "version": "0.1",
        "designDescription": {
            "designGraphs": [
                {
                    "rootIds": [1],
                    "designObjects": [
                        {
                            "id": 1,
                            "relativePath": "root.f3d",
                            "contentType": "f3d",
                            "references": [
                                {
                                    "type": kind,
                                    "ids": [2],
                                    "relationships": [
                                        {"id": 2, "metadata": {"neutronRole": "role"}}
                                    ],
                                },
                            ],
                        },
                        {
                            "id": 2,
                            "relativePath": "part.f3d",
                            "contentType": "f3d",
                            "references": [],
                        },
                    ],
                }
            ]
        },
    }
    row = {
        "type": kind,
        "from": "root.f3d",
        "relativePath": "part.f3d",
        "properties": [
            {"neutronRole": {"value": "role", "dataType": "STRING"}},
            {"neutronData": {"value": "independent-data", "dataType": "STRING"}},
        ],
    }
    tables = {
        "root.f3d": {
            "name": "RedirectionsStream",
            "schema-version": 0,
            "designs": [
                {"targetFileName": "root.f3d"},
                {"targetFileName": "part.f3d"},
            ],
            "references": [row],
        },
        "part.f3d": {
            "name": "RedirectionsStream",
            "schema-version": 0,
            "designs": [{"targetFileName": "part.f3d"}],
            "references": {},
        },
    }
    return manifest, description, tables


def encoded(value):
    return (
        value
        if isinstance(value, bytes)
        else json.dumps(value, ensure_ascii=False).encode()
    )


def package(data=None, *, overrides=None, extra=None, method=0):
    manifest, description, tables = values() if data is None else data
    overrides = overrides or {}
    entries = [
        ("Manifest.json", encoded(manifest)),
        ("DesignDescription.json", encoded(description)),
    ]
    for name, table in tables.items():
        child = document(
            extra=[("RedirectionsStream.dat", encoded(table))]
            if table is not None
            else []
        )
        entries.append((name, overrides.get(name, child)))
    entries.extend(extra or [])
    return make_zip(entries, method=method)[0]
