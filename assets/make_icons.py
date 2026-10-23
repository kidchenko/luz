"""Turn `luz --dump-icons <dir>` output into the committed icon assets.

    luz --dump-icons target/icons
    python assets/make_icons.py target/icons

Writes, next to this script:
  icon.ico       every size up to 256, PNG-compressed entries (Vista+)
  icon-256.png   for the Chocolatey/winget listings and the README
  icon-1024.png  the master build.sh turns into the macOS .icns, drawn on
                 Apple's icon grid (an 824px tile with room for its shadow)

and the landing page's copies in docs/img: icon-256.png and glyph-*.png.

and a PNG of every tray face into the dump directory, for looking at.

Standard library only: the app draws its own pixels, so the only job here is
container formats.
"""

import pathlib
import struct
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def png(rgba: bytes, edge: int) -> bytes:
    raw = b"".join(b"\x00" + rgba[y * edge * 4:(y + 1) * edge * 4] for y in range(edge))

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", edge, edge, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(images: list[tuple[int, bytes]]) -> bytes:
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for edge, data in images:
        # 0 in the width/height byte means 256.
        dim = 0 if edge >= 256 else edge
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    return header + entries + blobs


def main(dump: pathlib.Path) -> None:
    def load(name: str, edge: int) -> bytes:
        data = (dump / f"{name}-{edge}.rgba").read_bytes()
        assert len(data) == edge * edge * 4, f"{name}-{edge}: wrong size"
        return data

    (HERE / "icon.ico").write_bytes(ico([(e, png(load("app", e), e)) for e in ICO_SIZES]))
    (HERE / "icon-256.png").write_bytes(png(load("app", 256), 256))
    (HERE / "icon-1024.png").write_bytes(png(load("mac", 1024), 1024))

    docs = HERE.parent / "docs" / "img"
    docs.mkdir(parents=True, exist_ok=True)
    (docs / "icon-256.png").write_bytes(png(load("app", 256), 256))
    for face in ("day", "evening", "bedtime", "paused"):
        (docs / f"glyph-{face}.png").write_bytes(png(load(f"glyph-{face}", 128), 128))

    for path in sorted(dump.glob("tray-*.rgba")):
        edge = int(path.stem.rsplit("-", 1)[1])
        path.with_suffix(".png").write_bytes(png(path.read_bytes(), edge))

    print(f"wrote icon.ico, icon-256.png, icon-1024.png to {HERE}")
    print(f"wrote icon-256.png, glyph-*.png to {docs}")
    print(f"tray previews in {dump}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: make_icons.py <dump-dir>")
    main(pathlib.Path(sys.argv[1]))
