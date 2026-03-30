from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
PNG_PATH = ASSETS / "pane-icon.png"
ICO_PATH = ASSETS / "pane-icon.ico"
GLYPH = "\u25a2"
BACKGROUND = "#000000"
FOREGROUND = "#ffffff"
CANVAS = 1024
TARGET_COVERAGE = 0.62
FONT_CANDIDATES = [
    Path(r"C:\Windows\Fonts\seguisym.ttf"),
    Path(r"C:\Windows\Fonts\segoeui.ttf"),
    Path(r"C:\Windows\Fonts\arial.ttf"),
]
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def load_font(size: int) -> ImageFont.FreeTypeFont:
    for candidate in FONT_CANDIDATES:
        if candidate.exists():
            return ImageFont.truetype(str(candidate), size=size)
    raise FileNotFoundError("No suitable Windows font found for Pane icon generation.")


def glyph_bbox(size: int) -> tuple[int, int, int, int]:
    font = load_font(size)
    probe = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(probe)
    return draw.textbbox((0, 0), GLYPH, font=font)


def pick_font_size() -> int:
    low, high = 128, 900
    best = low
    while low <= high:
        mid = (low + high) // 2
        left, top, right, bottom = glyph_bbox(mid)
        width = right - left
        height = bottom - top
        coverage = max(width, height) / CANVAS
        if coverage <= TARGET_COVERAGE:
            best = mid
            low = mid + 1
        else:
            high = mid - 1
    return best


def render_icon() -> Image.Image:
    font_size = pick_font_size()
    font = load_font(font_size)
    image = Image.new("RGBA", (CANVAS, CANVAS), BACKGROUND)
    draw = ImageDraw.Draw(image)
    left, top, right, bottom = draw.textbbox((0, 0), GLYPH, font=font)
    width = right - left
    height = bottom - top
    x = (CANVAS - width) / 2 - left
    y = (CANVAS - height) / 2 - top
    draw.text((x, y), GLYPH, fill=FOREGROUND, font=font)
    return image


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    image = render_icon()
    image.save(PNG_PATH)
    image.save(ICO_PATH, sizes=ICO_SIZES)
    print(f"Wrote {PNG_PATH}")
    print(f"Wrote {ICO_PATH}")


if __name__ == "__main__":
    main()
