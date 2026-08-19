#!/usr/bin/env python3
"""Write the cell sprite sheets.

    python3 tools/mksprites.py

One sheet per cell state. A sheet is 256x256: a 16x16 grid of 16x16 tiles, so a
cell's u8 UV picks one tile. Tile (0,0) is the default art; the rest is room for
multi-cell pictures, where a pane spanning a rectangle gives each cell a
different tile so the parts line up.

RGBA, where R is saturation, G is lightness and A is coverage. There is no hue:
it comes from the player at draw time, so one sheet serves every player. Edges
are hard -- no anti-aliasing, these are pixel art.

Standalone Python rather than a cargo example, because the crate embeds these
files with include_bytes! and so cannot build until they exist.
"""
import struct, zlib, pathlib

TILE, GRID = 16, 16
SIZE = TILE * GRID

# character -> (saturation, lightness, coverage)
INK = {
    " ": (0.00, 0.00, 0.00),
    ".": (0.85, 0.42, 0.35),
    "-": (0.85, 0.55, 0.75),
    "#": (0.85, 0.62, 1.00),
    "=": (0.70, 0.88, 1.00),
}

def blank():
    return [[(0, 0, 0, 0)] * SIZE for _ in range(SIZE)]

def stamp(sheet, tx, ty, rows):
    assert len(rows) == TILE, f"tile is {len(rows)} rows, not {TILE}"
    for y, row in enumerate(rows):
        assert len(row) == TILE, f"row {y} is {len(row)} wide, not {TILE}"
        for x, c in enumerate(row):
            s, l, a = INK[c]
            sheet[ty * TILE + y][tx * TILE + x] = (
                round(s * 255), round(l * 255), 0, round(a * 255))

def write(name, sheet):
    raw = b"".join(b"\x00" + b"".join(bytes(px) for px in row) for row in sheet)
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    path = pathlib.Path("assets/sprites") / f"{name}.png"
    path.write_bytes(png)
    print(f"wrote {path}  {SIZE}x{SIZE}")

EMPTY = ["                "] * 16

ALIVE = [
    "                ",
    "   ##########   ",
    "  ############  ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    " ############## ",
    "  ############  ",
    "   ##########   ",
    "                ",
]

PANE = [
    "================",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "=..............=",
    "================",
]

PANE_OVER_ALIVE = [
    "================",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "=.------------.=",
    "================",
]

# Dead and ice-free is deliberately blank: the file exists so the set is
# complete, and so there is somewhere to draw rubble later.
for name, art in [
    ("dead", EMPTY),
    ("alive", ALIVE),
    ("dead_ice", PANE),
    ("alive_ice", PANE_OVER_ALIVE),
]:
    sheet = blank()
    stamp(sheet, 0, 0, art)
    write(name, sheet)
