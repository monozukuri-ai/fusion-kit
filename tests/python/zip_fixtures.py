"""Small independent ZIP32 fixtures, including raw-block Zstandard frames."""

import binascii
import struct
import zlib


def zstd_raw(data: bytes, *, window: int | None = None) -> bytes:
    assert len(data) <= 128 * 1024  # one raw block
    header = b"\x28\xb5\x2f\xfd"
    if window is not None:
        header += b"\x00" + bytes([window])
    elif len(data) < 256:
        header += b"\x20" + bytes([len(data)])
    else:
        header += b"\xa0" + struct.pack("<I", len(data))
    return header + ((len(data) << 3) | 1).to_bytes(3, "little") + data


def make_zip(entries=None, *, method=0, descriptor=False, compressed_override=None):
    entries = (
        [("Manifest.dat", b"manifest"), ("payload.bin", b"hello\x00world")]
        if entries is None
        else entries
    )
    local, central = bytearray(), bytearray()
    offsets = {}
    for name, data in entries:
        name = name.encode()
        flags = 0x800 | (8 if descriptor else 0)
        compressed = data
        if method == 8:
            compressor = zlib.compressobj(wbits=-15)
            compressed = compressor.compress(data) + compressor.flush()
        elif method == 93:
            compressed = zstd_raw(data)
        if compressed_override is not None and name.decode() in compressed_override:
            compressed = compressed_override[name.decode()]
        crc = binascii.crc32(data)
        at = len(local)
        local += struct.pack(
            "<4s5H3I2H",
            b"PK\x03\x04",
            20,
            flags,
            method,
            0,
            0,
            0 if descriptor else crc,
            0 if descriptor else len(compressed),
            0 if descriptor else len(data),
            len(name),
            0,
        )
        local += name
        offsets[name.decode()] = len(local)
        local += compressed
        if descriptor:
            local += struct.pack(
                "<4s3I", b"PK\x07\x08", crc, len(compressed), len(data)
            )
        central += struct.pack(
            "<4s6H3I5H2I",
            b"PK\x01\x02",
            20,
            20,
            flags,
            method,
            0,
            0,
            crc,
            len(compressed),
            len(data),
            len(name),
            0,
            0,
            0,
            0,
            0,
            at,
        )
        central += name
    result = (
        local
        + central
        + struct.pack(
            "<4s4H2IH",
            b"PK\x05\x06",
            0,
            0,
            len(entries),
            len(entries),
            len(central),
            len(local),
            0,
        )
    )
    return bytes(result), offsets


def patch_entry(data: bytes, name: str, *, method=None, size=None, flags=None):
    data = bytearray(data)
    for signature, name_offset, method_offset, size_offset, flags_offset in [
        (b"PK\x03\x04", 30, 8, 22, 6),
        (b"PK\x01\x02", 46, 10, 24, 8),
    ]:
        start = 0
        while (at := data.find(signature, start)) >= 0:
            start = at + 4
            if data[at + name_offset : at + name_offset + len(name)] != name.encode():
                continue
            for value, offset, fmt in [
                (method, method_offset, "<H"),
                (size, size_offset, "<I"),
                (flags, flags_offset, "<H"),
            ]:
                if value is not None:
                    struct.pack_into(fmt, data, at + offset, value)
    return bytes(data)
