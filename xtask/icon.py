#!/usr/bin/env python3
"""Render the extension icon: a chip outline with `rv` inside.

The mark is drawn rather than exported from a design tool so the colours and
proportions stay editable here. Supersampled 4x and downsampled, because PIL
draws without antialiasing.

Run: python3 xtask/icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SIZE = 128
SCALE = 4
BACKGROUND = "#1b1f26"
ACCENT = "#4ea1ff"
INK = "#eef3f8"

FONTS = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
]

OUT = Path(__file__).resolve().parent.parent / "editors" / "vscode" / "media" / "icon.png"


def font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONTS:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit(f"no monospace font found in {FONTS}")


def main() -> None:
    s = SCALE
    image = Image.new("RGBA", (SIZE * s, SIZE * s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    draw.rounded_rectangle([0, 0, SIZE * s - 1, SIZE * s - 1], radius=26 * s, fill=BACKGROUND)

    # Eight pins, two per side, reaching into the chip body so the strokes join.
    for offset in (46, 82):
        draw.line([18 * s, offset * s, 32 * s, offset * s], fill=ACCENT, width=5 * s)
        draw.line([96 * s, offset * s, 110 * s, offset * s], fill=ACCENT, width=5 * s)
        draw.line([offset * s, 18 * s, offset * s, 32 * s], fill=ACCENT, width=5 * s)
        draw.line([offset * s, 96 * s, offset * s, 110 * s], fill=ACCENT, width=5 * s)

    draw.rounded_rectangle([28 * s, 28 * s, 100 * s, 100 * s], radius=14 * s, outline=ACCENT, width=6 * s)
    draw.text((64 * s, 65 * s), "rv", font=font(40 * s), fill=INK, anchor="mm")

    image.resize((SIZE, SIZE), Image.LANCZOS).save(OUT)
    print(f"wrote {OUT} ({SIZE}x{SIZE}, {OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
