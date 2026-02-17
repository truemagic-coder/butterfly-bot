#!/usr/bin/env python3

from pathlib import Path

from PIL import Image


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    source_icon = root / "assets" / "icon.png"
    if not source_icon.exists():
        raise FileNotFoundError(f"Missing source icon: {source_icon}")

    sizes = [16, 24, 32, 48, 64, 128, 256, 512]

    with Image.open(source_icon) as image:
        base = image.convert("RGBA")
        for size in sizes:
            output = (
                root
                / "assets"
                / "icons"
                / "hicolor"
                / f"{size}x{size}"
                / "apps"
                / "butterfly-bot.png"
            )
            output.parent.mkdir(parents=True, exist_ok=True)
            resized = base.resize((size, size), Image.Resampling.LANCZOS)
            resized.save(output, format="PNG", optimize=True)

    print(f"Generated {len(sizes)} icon sizes from {source_icon}")


if __name__ == "__main__":
    main()
