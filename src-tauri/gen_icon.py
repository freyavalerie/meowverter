"""Generate Meowverter app icons: OLED-black rounded tile + neon RGB gradient play mark."""
import math
from PIL import Image, ImageDraw, ImageFilter

S = 1024  # master size


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def make_master():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # rounded-square OLED-black tile
    radius = int(S * 0.235)
    tile = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    td = ImageDraw.Draw(tile)
    td.rounded_rectangle([0, 0, S - 1, S - 1], radius=radius, fill=(8, 8, 10, 255))
    # subtle inner vignette ring
    td.rounded_rectangle([6, 6, S - 7, S - 7], radius=radius - 6, outline=(28, 28, 34, 255), width=4)
    img.alpha_composite(tile)

    # neon gradient glow behind the mark (magenta -> cyan diagonal)
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    c0 = (255, 0, 153)   # hot magenta
    c1 = (0, 225, 255)   # cyan
    cx, cy, r = S * 0.5, S * 0.5, S * 0.30
    for i in range(120):
        t = i / 119
        col = lerp(c0, c1, t)
        rad = r * (1 - t * 0.15)
        ang = t * math.pi
        ox = math.cos(ang) * S * 0.04
        oy = math.sin(ang) * S * 0.04
        gd.ellipse([cx - rad + ox, cy - rad + oy, cx + rad + ox, cy + rad + oy],
                   fill=(col[0], col[1], col[2], 5))
    glow = glow.filter(ImageFilter.GaussianBlur(S * 0.05))
    # clip glow to tile
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=radius, fill=255)
    img.paste(glow, (0, 0), Image.composite(glow.split()[3], Image.new("L", (S, S), 0), mask))

    # the play triangle with a vertical RGB gradient fill
    tri_layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    grad = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    gp = grad.load()
    # pride flag colours, mapped across the triangle's vertical extent
    stops = [(228, 3, 3), (255, 140, 0), (255, 237, 0),
             (0, 128, 38), (0, 76, 255), (115, 41, 130)]
    segs = len(stops) - 1
    y_top = S * 0.5 - S * 0.34 * 0.5
    y_bot = S * 0.5 + S * 0.34 * 0.5
    for y in range(S):
        t = min(max((y - y_top) / (y_bot - y_top), 0.0), 1.0) * segs
        i = min(int(t), segs - 1)
        col = lerp(stops[i], stops[i + 1], t - i)
        for x in range(S):
            gp[x, y] = (col[0], col[1], col[2], 255)
    tmask = Image.new("L", (S, S), 0)
    tm = ImageDraw.Draw(tmask)
    # play triangle, slightly rounded look via polygon
    w = S * 0.30
    h = S * 0.34
    cx2 = S * 0.54
    cy2 = S * 0.5
    pts = [(cx2 - w * 0.5, cy2 - h * 0.5),
           (cx2 - w * 0.5, cy2 + h * 0.5),
           (cx2 + w * 0.62, cy2)]
    tm.polygon(pts, fill=255)
    tri_layer = Image.composite(grad, tri_layer, tmask)
    # glow on the triangle edge
    edge = tmask.filter(ImageFilter.GaussianBlur(14))
    glowtri = Image.new("RGBA", (S, S), (255, 80, 200, 0))
    img.paste(Image.new("RGBA", (S, S), (255, 80, 200, 130)), (0, 0), edge)
    img.alpha_composite(tri_layer)

    return img


def main():
    master = make_master()
    out = "icons"
    master.resize((512, 512), Image.LANCZOS).save(f"{out}/icon.png")
    for sz in (32, 128, 256):
        name = "128x128@2x" if sz == 256 else f"{sz}x{sz}"
        master.resize((sz, sz), Image.LANCZOS).save(f"{out}/{name}.png")
    # multi-size ICO for the Windows executable resource
    ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    master.save(f"{out}/icon.ico", sizes=ico_sizes)
    print("icons written")


if __name__ == "__main__":
    main()
