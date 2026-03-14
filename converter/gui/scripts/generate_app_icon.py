from math import cos, pi, sin
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageOps


SIZE = 1024
BACKGROUND_BOUNDS = (72, 72, 952, 952)
BACKGROUND_RADIUS = 224


def make_linear_gradient(size: tuple[int, int], start: str, end: str, angle: float) -> Image.Image:
    gradient = Image.linear_gradient("L").resize(size)
    gradient = gradient.rotate(angle, expand=True).resize(size)
    return ImageOps.colorize(gradient, start, end).convert("RGBA")


def make_radial_glow(size: tuple[int, int], color: tuple[int, int, int], alpha_scale: float = 1.0) -> Image.Image:
    gradient = Image.radial_gradient("L").resize(size)
    alpha = ImageOps.invert(gradient).point(lambda value: int(value * alpha_scale))
    glow = Image.new("RGBA", size, color + (0,))
    glow.putalpha(alpha)
    return glow


def rounded_mask(size: tuple[int, int], radius: int) -> Image.Image:
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((0, 0, size[0], size[1]), radius=radius, fill=255)
    return mask


def paste_with_mask(target: Image.Image, image: Image.Image, position: tuple[int, int], mask: Image.Image | None = None) -> None:
    target.alpha_composite(image, dest=position) if mask is None else target.paste(image, position, mask)


def add_shadow(layer: Image.Image, blur_radius: int, offset: tuple[int, int], color: tuple[int, int, int, int]) -> Image.Image:
    shadow = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    alpha = layer.getchannel("A")
    colored = Image.new("RGBA", layer.size, color)
    shadow.paste(colored, (0, 0), alpha)
    shadow = shadow.filter(ImageFilter.GaussianBlur(blur_radius))
    canvas = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    canvas.alpha_composite(shadow, dest=offset)
    canvas.alpha_composite(layer)
    return canvas


def make_card(width: int, height: int, radius: int, start: str, end: str) -> Image.Image:
    gradient = make_linear_gradient((width, height), start, end, 38)
    mask = rounded_mask((width, height), radius)
    card = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    card.paste(gradient, (0, 0), mask)
    return card


def draw_star(draw: ImageDraw.ImageDraw, center: tuple[int, int], outer_radius: int, inner_radius: int, fill: tuple[int, int, int, int]) -> None:
    points: list[tuple[float, float]] = []
    for index in range(8):
        angle = (-pi / 2) + index * (pi / 4)
        radius = outer_radius if index % 2 == 0 else inner_radius
        points.append((center[0] + cos(angle) * radius, center[1] + sin(angle) * radius))
    draw.polygon(points, fill=fill)


def make_icon() -> Image.Image:
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))

    background_size = (
        BACKGROUND_BOUNDS[2] - BACKGROUND_BOUNDS[0],
        BACKGROUND_BOUNDS[3] - BACKGROUND_BOUNDS[1],
    )
    background_mask = rounded_mask(background_size, BACKGROUND_RADIUS)
    background_gradient = make_linear_gradient(background_size, "#0E84FF", "#6958FF", 45)
    background = Image.new("RGBA", background_size, (0, 0, 0, 0))
    background.paste(background_gradient, (0, 0), background_mask)

    glow = make_radial_glow(background_size, (255, 255, 255), 0.34)
    glow = ImageChops.offset(glow, -140, -150)
    background.alpha_composite(glow)
    paste_with_mask(canvas, background, (BACKGROUND_BOUNDS[0], BACKGROUND_BOUNDS[1]))

    border_draw = ImageDraw.Draw(canvas)
    border_draw.rounded_rectangle((100, 100, 924, 924), radius=196, outline=(255, 255, 255, 36), width=2)

    left_card = make_card(298, 372, 86, "#52E6C2", "#29C97C")
    left_card_draw = ImageDraw.Draw(left_card)
    left_card_draw.rounded_rectangle((60, 90, 182, 112), radius=11, fill=(255, 255, 255, 220))
    left_card_draw.rounded_rectangle((60, 132, 228, 154), radius=11, fill=(255, 255, 255, 184))
    left_card_draw.ellipse((66, 40, 102, 76), fill=(255, 255, 255, 235))
    left_card_draw.ellipse((116, 40, 152, 76), fill=(255, 255, 255, 170))
    left_card = add_shadow(left_card, blur_radius=22, offset=(0, 20), color=(26, 38, 94, 58))
    left_card = left_card.rotate(-10, resample=Image.Resampling.BICUBIC, expand=True)
    paste_with_mask(canvas, left_card, (170, 278))

    right_card = make_card(296, 360, 86, "#8AB8FF", "#5F78FF")
    right_card_draw = ImageDraw.Draw(right_card)
    right_card_draw.rounded_rectangle((62, 82, 216, 104), radius=11, fill=(255, 255, 255, 210))
    right_card_draw.rounded_rectangle((62, 124, 174, 146), radius=11, fill=(255, 255, 255, 164))
    right_card_draw.rounded_rectangle((62, 166, 238, 188), radius=11, fill=(255, 255, 255, 164))
    right_card = add_shadow(right_card, blur_radius=22, offset=(0, 20), color=(26, 38, 94, 58))
    right_card = right_card.rotate(10, resample=Image.Resampling.BICUBIC, expand=True)
    paste_with_mask(canvas, right_card, (490, 182))

    document_layer = Image.new("RGBA", (432, 576), (0, 0, 0, 0))
    document_draw = ImageDraw.Draw(document_layer)
    document_draw.rounded_rectangle((0, 0, 432, 576), radius=112, fill=(255, 255, 255, 255))
    document_draw.pieslice((210, 0, 432, 222), start=270, end=360, fill=(221, 235, 255, 255))
    document_draw.rectangle((321, 0, 432, 114), fill=(234, 244, 255, 255))
    lines_gradient = make_linear_gradient((260, 220), "#117DFF", "#5A67FF", 0)
    line_mask = Image.new("L", (260, 220), 0)
    line_draw = ImageDraw.Draw(line_mask)
    line_draw.rounded_rectangle((0, 0, 190, 34), radius=17, fill=255)
    line_draw.rounded_rectangle((0, 64, 244, 92), radius=14, fill=230)
    line_draw.rounded_rectangle((0, 116, 212, 144), radius=14, fill=184)
    line_draw.rounded_rectangle((0, 168, 232, 196), radius=14, fill=148)
    lines_layer = Image.new("RGBA", (260, 220), (0, 0, 0, 0))
    lines_layer.paste(lines_gradient, (0, 0), line_mask)
    document_layer.alpha_composite(lines_layer, dest=(86, 166))
    document_draw.line((342, 396, 372, 426, 428, 364), fill=(41, 201, 124, 255), width=34, joint="curve")
    document_layer = add_shadow(document_layer, blur_radius=24, offset=(0, 24), color=(25, 41, 107, 46))
    paste_with_mask(canvas, document_layer, (296, 200))

    sparkle_draw = ImageDraw.Draw(canvas)
    draw_star(sparkle_draw, (796, 296), 50, 20, (255, 255, 255, 235))
    draw_star(sparkle_draw, (236, 796), 34, 14, (255, 255, 255, 186))

    return canvas


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    output_path = root / "src-tauri" / "icons" / "app-icon-1024.png"
    image = make_icon()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    image.save(output_path)
    print(output_path)


if __name__ == "__main__":
    main()
