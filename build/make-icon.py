"""Generates build/icon.ico and build/icon.png (the app and installer icon).

Run with `python3 build/make-icon.py` (needs Pillow). The design matches the
tray icon in src/main/trayIcon.ts: two panels on a blue tile, the lower one
collapsed to its title bar. Small sizes use a simplified drawing so they stay
crisp in Explorer lists and the taskbar.
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SUPERSAMPLE = 4
TOP = (58, 168, 245)  # #3aa8f5
BOTTOM = (24, 98, 196)  # #1862c4
WHITE = (255, 255, 255)


def tile(size: int) -> Image.Image:
    """Rounded square with a vertical blue gradient."""
    s = size * SUPERSAMPLE
    gradient = Image.new("RGBA", (s, s))
    px = gradient.load()
    for y in range(s):
        t = y / (s - 1)
        row = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)) + (255,)
        for x in range(s):
            px[x, y] = row
    mask = Image.new("L", (s, s), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, s - 1, s - 1), radius=round(s * 0.22), fill=255)
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    out.paste(gradient, (0, 0), mask)
    return out


def draw_detailed(size: int) -> Image.Image:
    img = tile(size)
    s = img.width
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    u = s / 100  # 1 unit = 1% of the canvas
    r = round(3 * u)

    # Expanded panel: translucent body, solid title bar, two rows of icons.
    d.rounded_rectangle((14 * u, 15 * u, 86 * u, 63 * u), radius=r, fill=WHITE + (60,),
                        outline=WHITE + (150,), width=max(1, round(1.2 * u)))
    d.rounded_rectangle((14 * u, 15 * u, 86 * u, 25 * u), radius=r, fill=WHITE + (240,))
    for row in range(2):
        for col in range(3):
            x = 22 * u + col * 20 * u
            y = 31 * u + row * 16 * u
            d.rounded_rectangle((x, y, x + 16 * u, y + 11 * u), radius=round(2.2 * u), fill=WHITE + (235,))

    # Collapsed panel: title bar only, with a chevron.
    d.rounded_rectangle((14 * u, 70 * u, 86 * u, 82 * u), radius=r, fill=WHITE + (240,))
    cx, cy = 21 * u, 76 * u
    d.line((cx - 1.6 * u, cy - 2.6 * u, cx + 1.4 * u, cy, cx - 1.6 * u, cy + 2.6 * u),
           fill=BOTTOM + (255,), width=max(1, round(1.4 * u)), joint="curve")

    img.alpha_composite(layer)
    return img.resize((size, size), Image.LANCZOS)


def draw_simple(size: int) -> Image.Image:
    """Fewer, bolder shapes for 16–32 px."""
    img = tile(size)
    s = img.width
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    u = s / 16
    # Whole-unit coordinates on a 16-unit grid, so edges land on pixels at 16/32.
    box = lambda x0, y0, x1, y1, a: d.rectangle(  # noqa: E731
        (x0 * u, y0 * u, x1 * u - 1, y1 * u - 1), fill=WHITE + (a,))
    box(2, 2, 14, 9, 80)
    box(2, 2, 14, 4, 255)
    box(4, 5, 7, 8, 255)
    box(9, 5, 12, 8, 255)
    box(2, 11, 14, 13, 255)
    img.alpha_composite(layer)
    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
    images = [draw_simple(n) if n <= 32 else draw_detailed(n) for n in sizes]
    largest = images[-1]
    largest.save(HERE / "icon.ico", format="ICO", sizes=[(n, n) for n in sizes],
                 append_images=images[:-1])
    draw_detailed(512).save(HERE / "icon.png")
    print("wrote", HERE / "icon.ico", "and", HERE / "icon.png")


if __name__ == "__main__":
    main()
