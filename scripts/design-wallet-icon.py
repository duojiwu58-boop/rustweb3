"""Rebuild the original Web3Rust vector mark and Windows icon (requires Pillow)."""
from pathlib import Path
import struct
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "desktop" / "assets"
ASSETS.mkdir(parents=True, exist_ok=True)
SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
INK = "#17191B"
LIME = "#CAEF45"
POINTS = ((56, 82), (88, 174), (128, 122), (168, 174), (200, 82))

ASSETS.joinpath("wallet.svg").write_text('''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <title>Web3Rust Wallet — Connected W</title>
  <rect x="8" y="8" width="240" height="240" rx="56" fill="#17191B"/>
  <rect x="16" y="16" width="224" height="224" rx="49" fill="none" stroke="#343B2D" stroke-width="2"/>
  <path d="M56 82 L88 174 L128 122 L168 174 L200 82" fill="none" stroke="#CAEF45" stroke-width="22" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
''', encoding="utf-8")

def render(size):
    scale = 8
    ratio = size * scale / 256
    image = Image.new("RGBA", (size * scale, size * scale))
    draw = ImageDraw.Draw(image)
    def box(values):
        return tuple(round(v * ratio) for v in values)
    draw.rounded_rectangle(box((8, 8, 248, 248)), radius=round(56 * ratio), fill=INK)
    if size >= 32:
        draw.rounded_rectangle(box((16, 16, 240, 240)), radius=round(49 * ratio),
                               outline="#343B2D", width=max(1, round(2 * ratio)))
    width = round((24 if size <= 24 else 22) * ratio)
    points = [box(p) for p in POINTS]
    draw.line(points, fill=LIME, width=width, joint="curve")
    radius = width / 2
    for x, y in points:
        draw.ellipse((x-radius, y-radius, x+radius, y+radius), fill=LIME)
    result = image.resize((size, size), Image.Resampling.LANCZOS)
    result.putalpha(result.getchannel("A").point(lambda a: 0 if a < 4 else a))
    return result

frames = [render(size) for size in SIZES]
# DIB frames keep native Windows/Delphi readers compatible, including 256px.
entries, payloads = [], []
offset = 6 + 16 * len(frames)
for frame in frames:
    size = frame.width
    stride = ((size + 31) // 32) * 4
    mask = bytearray(stride * size)
    for y in range(size):
        for x in range(size):
            if frame.getpixel((x, y))[3] == 0:
                mask[(size-1-y)*stride+x//8] |= 0x80 >> (x % 8)
    pixels = frame.transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes("raw", "BGRA")
    header = struct.pack("<IiiHHIIiiII", 40, size, size*2, 1, 32, 0, len(pixels), 0, 0, 0, 0)
    payload = header + pixels + mask
    entries.append(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(payload), offset))
    payloads.append(payload)
    offset += len(payload)
ASSETS.joinpath("wallet.ico").write_bytes(struct.pack("<HHH", 0, 1, len(frames)) + b"".join(entries + payloads))
render(512).save(ASSETS / "wallet.png")
preview = Image.new("RGB", (1000, 520), "#F5F5F3")
draw = ImageDraw.Draw(preview)
font = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 24)
small = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 16)
draw.text((48, 30), "Web3Rust Wallet / Connected W", fill=INK, font=font)
preview.paste(render(256), (48, 104), render(256))
draw.rounded_rectangle((354, 104, 952, 360), radius=24, fill=INK)
for i, size in enumerate((16, 24, 32, 48, 64, 96)):
    x = 382 + i * 90
    icon = render(size)
    preview.paste(icon, (x + (80-size)//2, 210-size//2), icon)
    draw.text((x+22, 290), str(size), fill="#D1D5CD", font=small)
draw.text((48, 410), "Graphite  #17191B       Lime  #CAEF45", fill=INK, font=font)
draw.text((48, 458), "Transparent corners / 10 native sizes / optical weight at 16–24px", fill="#667064", font=small)
preview.save(ASSETS / "wallet-preview.png")
with Image.open(ASSETS / "wallet.ico") as check:
    assert check.ico.sizes() == {(s, s) for s in SIZES}
    for size in SIZES:
        assert check.ico.getimage((size, size)).getpixel((0, 0))[3] == 0
print("Created wallet.svg, wallet.ico (10 sizes), wallet.png and wallet-preview.png")
