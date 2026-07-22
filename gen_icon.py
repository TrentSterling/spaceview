"""Generate SpaceView icon assets from the SVG design."""
from PIL import Image, ImageDraw

def draw_icon(size):
    """Draw the treemap icon at given size, matching docs/assets/icon.svg."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    s = size / 96.0  # scale factor from 96x96 SVG viewBox

    # Background with rounded corners
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=int(16 * s), fill="#1a1a2e")

    # Treemap rectangles (rx=3 in SVG, approximate with rounded_rectangle)
    r = max(int(3 * s), 1)
    draw.rounded_rectangle([int(8*s), int(8*s), int(58*s), int(60*s)], radius=r, fill="#4361ee")    # big blue
    draw.rounded_rectangle([int(62*s), int(8*s), int(88*s), int(32*s)], radius=r, fill="#f72585")   # pink
    draw.rounded_rectangle([int(62*s), int(36*s), int(88*s), int(60*s)], radius=r, fill="#7209b7")  # purple
    draw.rounded_rectangle([int(8*s), int(64*s), int(42*s), int(88*s)], radius=r, fill="#4cc9f0")   # cyan
    draw.rounded_rectangle([int(46*s), int(64*s), int(88*s), int(88*s)], radius=r, fill="#3a0ca3")  # dark purple

    # Header strips (lighter)
    draw.rounded_rectangle([int(8*s), int(8*s), int(58*s), int(16*s)], radius=r, fill="#5a7df4")
    draw.rounded_rectangle([int(62*s), int(8*s), int(88*s), int(14*s)], radius=r, fill="#ff4da6")
    draw.rounded_rectangle([int(8*s), int(64*s), int(42*s), int(70*s)], radius=r, fill="#6dd5f5")

    return img

# Generate 256x256 PNG
icon256 = draw_icon(256)
icon256.save("assets/icon.png", "PNG")
print("Generated assets/icon.png (256x256)")

# Generate ICO with multiple sizes
sizes = [16, 32, 48, 256]
icons = [draw_icon(s) for s in sizes]
icons[0].save("assets/icon.ico", format="ICO", sizes=[(s, s) for s in sizes],
              append_images=icons[1:])
print(f"Generated assets/icon.ico ({', '.join(str(s) for s in sizes)})")
