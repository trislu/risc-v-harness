#!/usr/bin/env python3
"""Assemble PNG frames into a GIF for assets/.

Usage:
    python3 xtask/assemble-gif.py assets/frames/vscode-fix assets/vscode-fix.gif [--fps 4] [--width 960]

Frames are sorted by file name, so `01-typing.png`, `02-squiggle.png`, ...
gives the intended order. Requires Pillow, which is only needed for producing
assets and is not a build dependency.
"""

import argparse
import pathlib
import sys

try:
    from PIL import Image
except ImportError:  # pragma: no cover - the message is the point
    sys.exit("assemble-gif: Pillow is required (pip install pillow)")


def frame_paths(directory: pathlib.Path) -> list[pathlib.Path]:
    frames = sorted(p for p in directory.iterdir() if p.suffix.lower() == ".png")
    if not frames:
        sys.exit(f"assemble-gif: no PNG frames in {directory}")
    return frames


def load(path: pathlib.Path, width: int) -> Image.Image:
    image = Image.open(path).convert("RGB")
    if image.width > width:
        height = round(image.height * width / image.width)
        image = image.resize((width, height), Image.LANCZOS)
    return image


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("frames", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--fps", type=int, default=4, help="frames per second")
    parser.add_argument("--width", type=int, default=960, help="downscale width")
    parser.add_argument("--loop", type=int, default=0, help="0 loops forever")
    args = parser.parse_args()

    images = [load(path, args.width) for path in frame_paths(args.frames)]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # A single adaptive palette across all frames keeps the colours stable and
    # stops the GIF from shimmering when it loops.
    palette = images[0].quantize(colors=128, method=Image.Quantize.MEDIANCUT)
    frames = [image.quantize(palette=palette, dither=Image.Dither.NONE) for image in images]
    frames[0].save(
        args.output,
        save_all=True,
        append_images=frames[1:],
        duration=round(1000 / args.fps),
        loop=args.loop,
        optimize=True,
    )
    size = args.output.stat().st_size
    print(f"assemble-gif: {args.output} ({len(frames)} frames, {size // 1024} KiB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
