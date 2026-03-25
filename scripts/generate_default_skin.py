#!/usr/bin/env python3
"""Generate all BMP sprite sheets for the RetroAmp default skin.

Design: deep navy/charcoal base with teal-cyan accents, inspired by the
polish of BenuAmp Silver, the boldness of Solar Flare, and the smooth
gradients of Aoi Yuki.

All dimensions match the Winamp 2.x classic skin specification.
"""

import os
from PIL import Image, ImageDraw  # noqa: ImageFont not needed — we use pixel font

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "default-skin")

# -- Colour palette -------------------------------------------------------
# Deep navy base with cyan/teal accents and warm highlights

BG_DEEPEST = (14, 18, 30)    # absolute darkest
BG_DARK    = (20, 26, 40)    # main background
BG_MID     = (28, 36, 54)    # surface / panels
BG_LIGHT   = (38, 48, 68)    # raised elements
BG_LIGHTER = (50, 62, 85)    # highlights
BORDER_DK  = (45, 55, 78)    # border dark
BORDER_LT  = (65, 78, 105)   # border light
HIGHLIGHT  = (80, 95, 130)   # bright edge

# Accents
CYAN       = (0, 210, 200)   # primary accent (cyan-teal)
CYAN_BRIGHT= (80, 240, 230)  # bright cyan
CYAN_DIM   = (0, 130, 125)   # dim cyan
CYAN_DARK  = (0, 80, 76)     # very dim cyan

# Warm accent for active/current
WARM       = (255, 180, 60)  # amber/gold
WARM_DIM   = (180, 120, 30)

# Digit display
DIGIT_BG   = (10, 14, 22)    # LCD background
DIGIT_ON   = (0, 220, 200)   # segment ON
DIGIT_DIM  = (15, 30, 40)    # segment OFF (barely visible)

# Status colours
RED_SOFT   = (200, 70, 70)
RED_BRIGHT = (240, 90, 90)
GREEN_SOFT = (60, 180, 100)


# -- Pixel font (3x5 bitmap, 1px spacing) ---------------------------------
# Each char is a list of 5 rows, each row is a 3-bit mask (MSB=left).
PIXEL_FONT = {
    'A': [0b111, 0b101, 0b111, 0b101, 0b101],
    'B': [0b110, 0b101, 0b110, 0b101, 0b110],
    'C': [0b111, 0b100, 0b100, 0b100, 0b111],
    'D': [0b110, 0b101, 0b101, 0b101, 0b110],
    'E': [0b111, 0b100, 0b110, 0b100, 0b111],
    'F': [0b111, 0b100, 0b110, 0b100, 0b100],
    'G': [0b111, 0b100, 0b101, 0b101, 0b111],
    'H': [0b101, 0b101, 0b111, 0b101, 0b101],
    'I': [0b111, 0b010, 0b010, 0b010, 0b111],
    'J': [0b011, 0b001, 0b001, 0b101, 0b111],
    'K': [0b101, 0b101, 0b110, 0b101, 0b101],
    'L': [0b100, 0b100, 0b100, 0b100, 0b111],
    'M': [0b101, 0b111, 0b111, 0b101, 0b101],
    'N': [0b101, 0b111, 0b111, 0b101, 0b101],
    'O': [0b111, 0b101, 0b101, 0b101, 0b111],
    'P': [0b111, 0b101, 0b111, 0b100, 0b100],
    'Q': [0b111, 0b101, 0b101, 0b111, 0b001],
    'R': [0b111, 0b101, 0b111, 0b110, 0b101],
    'S': [0b111, 0b100, 0b111, 0b001, 0b111],
    'T': [0b111, 0b010, 0b010, 0b010, 0b010],
    'U': [0b101, 0b101, 0b101, 0b101, 0b111],
    'V': [0b101, 0b101, 0b101, 0b101, 0b010],
    'W': [0b101, 0b101, 0b111, 0b111, 0b101],
    'X': [0b101, 0b101, 0b010, 0b101, 0b101],
    'Y': [0b101, 0b101, 0b010, 0b010, 0b010],
    'Z': [0b111, 0b001, 0b010, 0b100, 0b111],
    '0': [0b111, 0b101, 0b101, 0b101, 0b111],
    '1': [0b010, 0b110, 0b010, 0b010, 0b111],
    '2': [0b111, 0b001, 0b111, 0b100, 0b111],
    '3': [0b111, 0b001, 0b111, 0b001, 0b111],
    '4': [0b101, 0b101, 0b111, 0b001, 0b001],
    '5': [0b111, 0b100, 0b111, 0b001, 0b111],
    '6': [0b111, 0b100, 0b111, 0b101, 0b111],
    '7': [0b111, 0b001, 0b001, 0b001, 0b001],
    '8': [0b111, 0b101, 0b111, 0b101, 0b111],
    '9': [0b111, 0b101, 0b111, 0b001, 0b111],
    ' ': [0b000, 0b000, 0b000, 0b000, 0b000],
}

def px_text(draw, x, y, text, color):
    """Draw pixel-font text. Each char is 3px wide + 1px gap = 4px per char."""
    cx = x
    for ch in text.upper():
        glyph = PIXEL_FONT.get(ch)
        if glyph is None:
            cx += 4
            continue
        for row_idx, row_bits in enumerate(glyph):
            for col in range(3):
                if row_bits & (0b100 >> col):
                    draw.point((cx + col, y + row_idx), fill=color)
        cx += 4

def px_text_width(text):
    """Width in pixels for pixel-font text."""
    return len(text) * 4 - 1  # 3px char + 1px gap, minus trailing gap

def px_text_centered(draw, x, y, w, h, text, color):
    """Draw pixel-font text centered in a rectangle."""
    tw = px_text_width(text)
    tx = x + (w - tw) // 2
    ty = y + (h - 5) // 2
    px_text(draw, tx, ty, text, color)


def save(img: Image.Image, name: str):
    path = os.path.join(OUT, name)
    img.save(path)
    print(f"  {name} ({img.width}x{img.height})")


def lerp(c1, c2, t):
    """Linearly interpolate between two RGB tuples."""
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t) for i in range(3))


def gradient_v(draw, x, y, w, h, c1, c2):
    for row in range(h):
        c = lerp(c1, c2, row / max(h - 1, 1))
        draw.line([(x, y + row), (x + w - 1, y + row)], fill=c)


def gradient_h(draw, x, y, w, h, c1, c2):
    for col in range(w):
        c = lerp(c1, c2, col / max(w - 1, 1))
        draw.line([(x + col, y), (x + col, y + h - 1)], fill=c)


def gradient_v_multi(draw, x, y, w, h, stops):
    """Multi-stop vertical gradient. stops = [(pos, color), ...] with pos 0..1."""
    for row in range(h):
        t = row / max(h - 1, 1)
        # Find surrounding stops
        for i in range(len(stops) - 1):
            p0, c0 = stops[i]
            p1, c1 = stops[i + 1]
            if p0 <= t <= p1:
                local_t = (t - p0) / max(p1 - p0, 0.001)
                c = lerp(c0, c1, local_t)
                draw.line([(x, y + row), (x + w - 1, y + row)], fill=c)
                break


def bevel_smooth(draw, x, y, w, h, base, depth=20):
    """Smooth beveled rectangle with anti-aliased edges."""
    # Fill with gradient for depth
    top = lerp(base, (255,255,255), depth/255)
    bot = lerp(base, (0,0,0), depth/255)
    gradient_v(draw, x+1, y+1, w-2, h-2, top, bot)
    # Edges
    draw.line([(x, y), (x+w-1, y)], fill=lerp(base, (255,255,255), depth*1.5/255))
    draw.line([(x, y), (x, y+h-1)], fill=lerp(base, (255,255,255), depth*1.2/255))
    draw.line([(x+w-1, y+1), (x+w-1, y+h-1)], fill=lerp(base, (0,0,0), depth*1.2/255))
    draw.line([(x+1, y+h-1), (x+w-1, y+h-1)], fill=lerp(base, (0,0,0), depth*1.5/255))


def sunken_smooth(draw, x, y, w, h, base, depth=15):
    """Sunken inset with smooth shading."""
    top = lerp(base, (0,0,0), depth/255)
    bot = lerp(base, (255,255,255), depth*0.5/255)
    gradient_v(draw, x+1, y+1, w-2, h-2, top, bot)
    draw.line([(x, y), (x+w-1, y)], fill=lerp(base, (0,0,0), depth*1.5/255))
    draw.line([(x, y), (x, y+h-1)], fill=lerp(base, (0,0,0), depth*1.2/255))
    draw.line([(x+w-1, y+1), (x+w-1, y+h-1)], fill=lerp(base, (255,255,255), depth*0.8/255))
    draw.line([(x+1, y+h-1), (x+w-1, y+h-1)], fill=lerp(base, (255,255,255), depth*0.8/255))


def button_rich(draw, x, y, w, h, pressed=False):
    """Rich button with multi-stop gradient, like the reference skins."""
    if pressed:
        stops = [
            (0.0, lerp(BG_MID, (0,0,0), 0.15)),
            (0.3, BG_MID),
            (1.0, lerp(BG_MID, (255,255,255), 0.05)),
        ]
        gradient_v_multi(draw, x+1, y+1, w-2, h-2, stops)
        draw.line([(x, y), (x+w-1, y)], fill=lerp(BG_MID, (0,0,0), 0.2))
        draw.line([(x, y), (x, y+h-1)], fill=lerp(BG_MID, (0,0,0), 0.2))
        draw.line([(x+w-1, y+1), (x+w-1, y+h-1)], fill=BORDER_DK)
        draw.line([(x+1, y+h-1), (x+w-1, y+h-1)], fill=BORDER_DK)
    else:
        stops = [
            (0.0, lerp(BG_LIGHTER, (255,255,255), 0.12)),
            (0.15, BG_LIGHTER),
            (0.5, BG_LIGHT),
            (0.85, lerp(BG_LIGHT, (0,0,0), 0.1)),
            (1.0, lerp(BG_LIGHT, (0,0,0), 0.2)),
        ]
        gradient_v_multi(draw, x+1, y+1, w-2, h-2, stops)
        draw.line([(x, y), (x+w-1, y)], fill=HIGHLIGHT)
        draw.line([(x, y), (x, y+h-1)], fill=lerp(HIGHLIGHT, BG_LIGHTER, 0.5))
        draw.line([(x+w-1, y+1), (x+w-1, y+h-1)], fill=BORDER_DK)
        draw.line([(x+1, y+h-1), (x+w-1, y+h-1)], fill=lerp(BORDER_DK, (0,0,0), 0.15))


# == MAIN.BMP (275x116) ===================================================

def gen_main():
    img = Image.new("RGB", (275, 116), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Background: multi-stop gradient for depth
    gradient_v_multi(draw, 0, 0, 275, 116, [
        (0.0, BG_MID),
        (0.12, BG_DARK),
        (0.7, BG_DARK),
        (1.0, BG_DEEPEST),
    ])

    # Outer border with subtle highlight on top/left
    draw.rectangle([0, 0, 274, 115], outline=BORDER_DK)
    draw.line([(1, 0), (273, 0)], fill=BORDER_LT)
    draw.line([(0, 1), (0, 114)], fill=lerp(BORDER_DK, BORDER_LT, 0.5))

    # Accent line under titlebar area
    draw.line([(1, 14), (273, 14)], fill=CYAN_DARK)
    draw.line([(1, 15), (273, 15)], fill=lerp(CYAN_DARK, BG_DARK, 0.7))

    # Visualizer area (inset)
    sunken_smooth(draw, 24, 43, 76, 16, DIGIT_BG, depth=20)

    # Song title scroll area
    sunken_smooth(draw, 111, 24, 153, 12, DIGIT_BG, depth=18)

    # Info text area (bitrate/freq)
    sunken_smooth(draw, 111, 43, 153, 16, DIGIT_BG, depth=18)

    # Time display
    sunken_smooth(draw, 36, 26, 63, 13, DIGIT_BG, depth=22)

    # Clutterbar
    gradient_v(draw, 10, 22, 8, 37, BG_DEEPEST, lerp(BG_DEEPEST, BG_DARK, 0.5))
    draw.rectangle([10, 22, 17, 58], outline=lerp(BG_DEEPEST, BORDER_DK, 0.5))

    # Seek bar groove
    sunken_smooth(draw, 16, 72, 249, 10, lerp(BG_DEEPEST, BG_DARK, 0.3), depth=12)

    # Volume/balance areas (subtle sunken)
    sunken_smooth(draw, 107, 57, 69, 14, lerp(BG_DEEPEST, BG_DARK, 0.4), depth=10)
    sunken_smooth(draw, 177, 57, 61, 14, lerp(BG_DEEPEST, BG_DARK, 0.4), depth=10)

    # Separator lines
    draw.line([(1, 86), (273, 86)], fill=BORDER_DK)
    draw.line([(1, 87), (273, 87)], fill=lerp(BORDER_DK, BG_DARK, 0.7))

    save(img, "main.bmp")


# == TITLEBAR.BMP (302x56) ================================================

def gen_titlebar():
    img = Image.new("RGB", (302, 56), BG_DARK)
    draw = ImageDraw.Draw(img)

    # -- Selected (active) title bar: y=0, x=27..301 --
    gradient_v_multi(draw, 27, 0, 275, 14, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.08)),
        (0.3, BG_LIGHTER),
        (0.7, BG_LIGHT),
        (1.0, BG_MID),
    ])
    # Cyan accent stripe at bottom
    draw.line([(27, 12), (301, 12)], fill=CYAN_DARK)
    draw.line([(27, 13), (301, 13)], fill=lerp(CYAN_DARK, BG_MID, 0.6))
    # Title text "RETROAMP" in accent
    title = "RETROAMP"
    tx = 27 + 90
    for i, ch in enumerate(title):
        cx = tx + i * 9
        if cx + 6 < 302:
            # Anti-aliased character block
            draw.rectangle([cx+1, 3, cx+5, 10], fill=CYAN)
            draw.rectangle([cx+2, 4, cx+4, 9], fill=CYAN_BRIGHT)
            # Add slight glow
            for dy in [-1, 8]:
                draw.line([(cx+1, 3+dy), (cx+5, 3+dy)], fill=CYAN_DARK)

    # -- Inactive title bar: y=15 --
    gradient_v_multi(draw, 27, 15, 275, 14, [
        (0.0, BG_MID),
        (0.5, lerp(BG_DARK, BG_MID, 0.3)),
        (1.0, BG_DARK),
    ])
    draw.line([(27, 28), (301, 28)], fill=BORDER_DK)

    # -- Shade mode selected: y=29 --
    gradient_v_multi(draw, 27, 29, 275, 14, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.06)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])
    draw.line([(27, 42), (301, 42)], fill=CYAN_DARK)

    # -- Shade mode inactive: y=42 --
    gradient_v(draw, 27, 42, 275, 14, BG_MID, BG_DARK)

    # -- Title bar buttons (9x9) in left 27px --
    # Use bright, high-contrast icons so they're visible at any scale.

    # Options button (x=0,y=0 normal; x=0,y=9 pressed) — hamburger menu
    bevel_smooth(draw, 0, 0, 9, 9, BG_LIGHT, 18)
    for yy in [2, 4, 6]:
        draw.line([(2, yy), (6, yy)], fill=CYAN)
    sunken_smooth(draw, 0, 9, 9, 9, BG_MID, 12)
    for yy in [11, 13, 15]:
        draw.line([(2, yy), (6, yy)], fill=CYAN_BRIGHT)

    # Minimize (x=9) — underscore
    bevel_smooth(draw, 9, 0, 9, 9, BG_LIGHT, 18)
    draw.line([(11, 6), (16, 6)], fill=CYAN)
    draw.line([(11, 7), (16, 7)], fill=CYAN_DIM)
    sunken_smooth(draw, 9, 9, 9, 9, BG_MID, 12)
    draw.line([(11, 15), (16, 15)], fill=CYAN_BRIGHT)
    draw.line([(11, 16), (16, 16)], fill=CYAN)

    # Shade (x=0, y=18) — double horizontal lines
    bevel_smooth(draw, 0, 18, 9, 9, BG_LIGHT, 18)
    draw.line([(2, 20), (6, 20)], fill=CYAN)
    draw.line([(2, 21), (6, 21)], fill=CYAN)
    draw.line([(2, 24), (6, 24)], fill=CYAN)
    draw.line([(2, 25), (6, 25)], fill=CYAN)
    sunken_smooth(draw, 9, 18, 9, 9, BG_MID, 12)
    draw.line([(11, 20), (15, 20)], fill=CYAN_BRIGHT)
    draw.line([(11, 21), (15, 21)], fill=CYAN_BRIGHT)
    draw.line([(11, 24), (15, 24)], fill=CYAN_BRIGHT)
    draw.line([(11, 25), (15, 25)], fill=CYAN_BRIGHT)

    # Close (x=18) — bold X
    bevel_smooth(draw, 18, 0, 9, 9, BG_LIGHT, 18)
    # Draw a 2px-thick X for visibility
    for i in range(5):
        draw.point((20+i, 2+i), fill=RED_BRIGHT)
        draw.point((24-i, 2+i), fill=RED_BRIGHT)
        draw.point((21+i, 2+i), fill=RED_SOFT)   # thicken
        draw.point((23-i, 2+i), fill=RED_SOFT)   # thicken

    sunken_smooth(draw, 18, 9, 9, 9, BG_MID, 12)
    for i in range(5):
        draw.point((20+i, 11+i), fill=(255, 120, 120))
        draw.point((24-i, 11+i), fill=(255, 120, 120))
        draw.point((21+i, 11+i), fill=RED_BRIGHT)
        draw.point((23-i, 11+i), fill=RED_BRIGHT)

    save(img, "titlebar.bmp")


# == CBUTTONS.BMP (136x36) ================================================

def gen_cbuttons():
    img = Image.new("RGB", (136, 36), BG_DARK)
    draw = ImageDraw.Draw(img)

    btns = [(0, 23, 18), (23, 23, 18), (46, 23, 18), (69, 23, 18),
            (92, 22, 18), (114, 22, 16)]

    for x, w, h in btns:
        button_rich(draw, x, 0, w, h, pressed=False)
        button_rich(draw, x, h, w, h, pressed=True)

    # Icon colour: brighter on normal, slightly dimmer on pressed
    ic = CYAN
    ic_p = CYAN_BRIGHT  # pressed icons slightly brighter (inverted feel)
    ic_aa = CYAN_DIM    # anti-alias shade

    # Previous |<< (x=0..22)
    for yo, c, aa in [(0, ic, ic_aa), (18, ic_p, CYAN)]:
        # Double triangle + bar
        draw.polygon([(8, 4+yo), (8, 13+yo), (3, 8+yo)], fill=c)
        draw.polygon([(16, 4+yo), (16, 13+yo), (11, 8+yo)], fill=c)
        draw.line([(2, 4+yo), (2, 13+yo)], fill=c)
        draw.line([(3, 5+yo), (3, 12+yo)], fill=aa)  # AA edge

    # Play > (x=23..45)
    for yo, c, aa in [(0, ic, ic_aa), (18, ic_p, CYAN)]:
        draw.polygon([(28, 3+yo), (28, 14+yo), (39, 8+yo)], fill=c)
        # Anti-alias the diagonal edges
        for i in range(6):
            draw.point((29+i, 4+i+yo), fill=aa)
            draw.point((29+i, 13-i+yo), fill=aa)

    # Pause || (x=46..68)
    for yo, c in [(0, ic), (18, ic_p)]:
        draw.rectangle([51, 4+yo, 55, 13+yo], fill=c)
        draw.rectangle([58, 4+yo, 62, 13+yo], fill=c)

    # Stop (x=69..91)
    for yo, c in [(0, ic), (18, ic_p)]:
        draw.rectangle([74, 4+yo, 84, 13+yo], fill=c)

    # Next >>| (x=92..113)
    for yo, c, aa in [(0, ic, ic_aa), (18, ic_p, CYAN)]:
        draw.polygon([(96, 4+yo), (96, 13+yo), (101, 8+yo)], fill=c)
        draw.polygon([(103, 4+yo), (103, 13+yo), (108, 8+yo)], fill=c)
        draw.line([(110, 4+yo), (110, 13+yo)], fill=c)
        draw.line([(109, 5+yo), (109, 12+yo)], fill=aa)

    # Eject (x=114..135)
    for yo, c, aa in [(0, ic, ic_aa), (16, ic_p, CYAN)]:
        draw.polygon([(120, 10+yo), (130, 10+yo), (125, 3+yo)], fill=c)
        draw.line([(120, 12+yo), (130, 12+yo)], fill=c)
        draw.line([(120, 13+yo), (130, 13+yo)], fill=aa)
        # AA on triangle edges
        draw.point((121, 9+yo), fill=aa)
        draw.point((129, 9+yo), fill=aa)

    save(img, "cbuttons.bmp")


# == NUMBERS.BMP (99x13) and NUMS_EX.BMP (108x13) =========================

def draw_digit_aa(draw, x, y, digit, on_color=DIGIT_ON, off_color=DIGIT_DIM):
    """Draw anti-aliased 7-segment digit with intermediate shades."""
    segs = {
        0: 'abcdef', 1: 'bc', 2: 'abdeg', 3: 'abcdg',
        4: 'bcfg', 5: 'acdfg', 6: 'acdefg', 7: 'abc',
        8: 'abcdefg', 9: 'abcdfg',
    }
    active = segs.get(digit, '')

    # Intermediate shades for anti-aliasing
    on_mid = lerp(on_color, off_color, 0.3)    # slightly dimmer than on
    on_glow = lerp(on_color, (255,255,255), 0.2)  # bright center

    def draw_seg(name, on):
        c = on_color if on else off_color
        c_mid = on_mid if on else off_color
        c_glow = on_glow if on else off_color

        if name == 'a':  # top horizontal
            draw.line([(x+3, y+1), (x+6, y+1)], fill=c)
            draw.point((x+2, y+1), fill=c_mid)
            draw.point((x+7, y+1), fill=c_mid)
            if on: draw.line([(x+4, y+1), (x+5, y+1)], fill=c_glow)
        elif name == 'b':  # top-right vertical
            draw.line([(x+7, y+2), (x+7, y+5)], fill=c)
            if on:
                draw.point((x+7, y+3), fill=c_glow)
                draw.point((x+7, y+4), fill=c_glow)
        elif name == 'c':  # bottom-right vertical
            draw.line([(x+7, y+7), (x+7, y+10)], fill=c)
            if on:
                draw.point((x+7, y+8), fill=c_glow)
                draw.point((x+7, y+9), fill=c_glow)
        elif name == 'd':  # bottom horizontal
            draw.line([(x+3, y+11), (x+6, y+11)], fill=c)
            draw.point((x+2, y+11), fill=c_mid)
            draw.point((x+7, y+11), fill=c_mid)
            if on: draw.line([(x+4, y+11), (x+5, y+11)], fill=c_glow)
        elif name == 'e':  # bottom-left vertical
            draw.line([(x+1, y+7), (x+1, y+10)], fill=c)
            if on:
                draw.point((x+1, y+8), fill=c_glow)
                draw.point((x+1, y+9), fill=c_glow)
        elif name == 'f':  # top-left vertical
            draw.line([(x+1, y+2), (x+1, y+5)], fill=c)
            if on:
                draw.point((x+1, y+3), fill=c_glow)
                draw.point((x+1, y+4), fill=c_glow)
        elif name == 'g':  # middle horizontal
            draw.line([(x+3, y+6), (x+6, y+6)], fill=c)
            draw.point((x+2, y+6), fill=c_mid)
            draw.point((x+7, y+6), fill=c_mid)
            if on: draw.line([(x+4, y+6), (x+5, y+6)], fill=c_glow)

    for s in 'abcdefg':
        draw_seg(s, s in active)


def gen_numbers():
    img = Image.new("RGB", (99, 13), DIGIT_BG)
    draw = ImageDraw.Draw(img)
    for i in range(10):
        draw_digit_aa(draw, i * 9, 0, i)
    save(img, "numbers.bmp")


def gen_nums_ex():
    img = Image.new("RGB", (108, 13), DIGIT_BG)
    draw = ImageDraw.Draw(img)
    for i in range(10):
        draw_digit_aa(draw, i * 9, 0, i)
    # Minus sign at x=90
    draw.line([(93, 6), (96, 6)], fill=DIGIT_ON)
    draw.point((92, 6), fill=lerp(DIGIT_ON, DIGIT_BG, 0.5))
    draw.point((97, 6), fill=lerp(DIGIT_ON, DIGIT_BG, 0.5))
    save(img, "nums_ex.bmp")


# == PLAYPAUS.BMP (33x9) ==================================================

def gen_playpaus():
    img = Image.new("RGB", (33, 9), BG_DARK)
    draw = ImageDraw.Draw(img)
    # Playing: green triangle
    draw.polygon([(1, 1), (1, 7), (7, 4)], fill=GREEN_SOFT)
    # Paused: two bars
    draw.rectangle([10, 1, 12, 7], fill=CYAN)
    draw.rectangle([15, 1, 17, 7], fill=CYAN)
    # Stopped: square (dim)
    draw.rectangle([19, 1, 26, 7], fill=CYAN_DIM)
    # Work indicator
    draw.rectangle([27, 0, 29, 8], fill=BG_DARK)
    draw.rectangle([30, 0, 32, 8], fill=CYAN)
    save(img, "playpaus.bmp")


# == POSBAR.BMP (307x10) ==================================================

def gen_posbar():
    img = Image.new("RGB", (307, 10), BG_DARK)
    draw = ImageDraw.Draw(img)
    # Slider groove
    sunken_smooth(draw, 0, 0, 248, 10, lerp(BG_DEEPEST, BG_DARK, 0.3), depth=14)
    # Thumb normal (at x=248)
    bevel_smooth(draw, 248, 0, 29, 10, BG_LIGHTER, 16)
    draw.line([(256, 3), (256, 6)], fill=CYAN_DIM)
    draw.line([(262, 3), (262, 6)], fill=CYAN_DIM)
    draw.line([(259, 2), (259, 7)], fill=CYAN)
    # Thumb pressed (at x=278)
    sunken_smooth(draw, 278, 0, 29, 10, BG_MID, 10)
    draw.line([(286, 3), (286, 6)], fill=CYAN)
    draw.line([(292, 3), (292, 6)], fill=CYAN)
    draw.line([(289, 2), (289, 7)], fill=CYAN_BRIGHT)
    save(img, "posbar.bmp")


# == VOLUME.BMP (68x433) ==================================================

def gen_volume():
    img = Image.new("RGB", (68, 433), BG_DARK)
    draw = ImageDraw.Draw(img)
    for i in range(28):
        y = i * 15
        draw.rectangle([0, y, 67, y + 14], fill=BG_DARK)
        sunken_smooth(draw, 4, y + 2, 60, 10, lerp(BG_DEEPEST, BG_DARK, 0.3), depth=10)
        # Fill bar with multi-stop gradient
        fill_w = int((i / 27) * 56)
        if fill_w > 0:
            for col in range(fill_w):
                t = col / max(fill_w - 1, 1)
                c = lerp(CYAN_DARK, CYAN, t)
                draw.line([(6 + col, y + 4), (6 + col, y + 9)], fill=c)
            # Bright leading edge
            draw.line([(5 + fill_w, y + 4), (5 + fill_w, y + 9)], fill=CYAN_BRIGHT)
    # Thumb normal
    bevel_smooth(draw, 15, 422, 14, 11, BG_LIGHTER, 16)
    draw.line([(20, 425), (20, 429)], fill=CYAN_DIM)
    draw.line([(24, 425), (24, 429)], fill=CYAN_DIM)
    draw.line([(22, 424), (22, 430)], fill=CYAN)
    # Thumb pressed
    sunken_smooth(draw, 0, 422, 14, 11, BG_MID, 10)
    draw.line([(5, 425), (5, 429)], fill=CYAN)
    draw.line([(9, 425), (9, 429)], fill=CYAN)
    draw.line([(7, 424), (7, 430)], fill=CYAN_BRIGHT)
    save(img, "volume.bmp")


# == BALANCE.BMP (47x433) =================================================

def gen_balance():
    img = Image.new("RGB", (47, 433), BG_DARK)
    draw = ImageDraw.Draw(img)
    for i in range(28):
        y = i * 15
        draw.rectangle([9, y, 46, y + 14], fill=BG_DARK)
        sunken_smooth(draw, 12, y + 2, 32, 10, lerp(BG_DEEPEST, BG_DARK, 0.3), depth=10)
        # Center marker
        draw.line([(28, y + 3), (28, y + 10)], fill=CYAN_DIM)
        draw.point((28, y + 6), fill=CYAN)
    # Thumb normal
    bevel_smooth(draw, 15, 422, 14, 11, BG_LIGHTER, 16)
    draw.line([(20, 425), (20, 429)], fill=CYAN_DIM)
    draw.line([(24, 425), (24, 429)], fill=CYAN_DIM)
    draw.line([(22, 424), (22, 430)], fill=CYAN)
    # Thumb pressed
    sunken_smooth(draw, 0, 422, 14, 11, BG_MID, 10)
    draw.line([(5, 425), (5, 429)], fill=CYAN)
    draw.line([(9, 425), (9, 429)], fill=CYAN)
    draw.line([(7, 424), (7, 430)], fill=CYAN_BRIGHT)
    save(img, "balance.bmp")


# == SHUFREP.BMP (92x85) ==================================================

def gen_shufrep():
    img = Image.new("RGB", (92, 85), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Repeat: 28x15, 4 states
    for row, active in [(0, False), (15, True), (30, False), (45, True)]:
        pressed = row >= 30
        button_rich(draw, 0, row, 28, 15, pressed)
        c = CYAN if active else CYAN_DIM
        # Loop arrows icon
        draw.arc([5, row+3, 23, row+12], 180, 0, fill=c, width=1)
        draw.arc([5, row+4, 23, row+12], 0, 180, fill=c, width=1)
        # Arrow tips
        draw.polygon([(20, row+3), (20, row+7), (23, row+5)], fill=c)

    # Shuffle: 47x15, 4 states
    for row, active in [(0, False), (15, True), (30, False), (45, True)]:
        pressed = row >= 30
        button_rich(draw, 28, row, 47, 15, pressed)
        c = CYAN if active else CYAN_DIM
        aa = lerp(c, BG_LIGHT if not pressed else BG_MID, 0.5)
        draw.line([(34, row+5), (60, row+10)], fill=c)
        draw.line([(34, row+10), (60, row+5)], fill=c)
        # Anti-alias
        draw.line([(35, row+5), (61, row+10)], fill=aa)
        # Arrow tips
        draw.polygon([(60, row+4), (60, row+7), (63, row+5)], fill=c)
        draw.polygon([(60, row+9), (60, row+12), (63, row+10)], fill=c)

    # EQ: 23x12, 4 states
    for col, y, pressed, active in [
        (0, 61, False, False), (46, 61, False, True),
        (0, 73, True, False), (46, 73, True, True),
    ]:
        button_rich(draw, col, y, 23, 12, pressed)
        c = CYAN if active else CYAN_DIM
        px_text_centered(draw, col, y, 23, 12, "EQ", c)

    # PL: 23x12, 4 states
    for col, y, pressed, active in [
        (23, 61, False, False), (69, 61, False, True),
        (23, 73, True, False), (69, 73, True, True),
    ]:
        button_rich(draw, col, y, 23, 12, pressed)
        c = CYAN if active else CYAN_DIM
        px_text_centered(draw, col, y, 23, 12, "PL", c)

    save(img, "shufrep.bmp")


# == MONOSTER.BMP (56x24) =================================================

def gen_monoster():
    img = Image.new("RGB", (56, 24), BG_DARK)
    draw = ImageDraw.Draw(img)
    # Stereo active / inactive
    px_text_centered(draw, 0, 0, 29, 12, "STEREO", CYAN)
    px_text_centered(draw, 0, 12, 29, 12, "STEREO", CYAN_DARK)
    # Mono active / inactive
    px_text_centered(draw, 29, 0, 27, 12, "MONO", CYAN)
    px_text_centered(draw, 29, 12, 27, 12, "MONO", CYAN_DARK)
    save(img, "monoster.bmp")


# == TEXT.BMP (155x18) =====================================================

def gen_text():
    """Bitmap font for the scrolling title. 5x6 cells, 31 per row, 3 rows.
    Characters are drawn with the pixel font, centered in each 5x6 cell."""
    img = Image.new("RGB", (155, 18), BG_DARK)
    draw = ImageDraw.Draw(img)
    charmap_row0 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ"@   '
    charmap_row1 = '0123456789...:()-\'!_+\\/[]^&%,=$#'
    charmap_row2 = '                               '

    # Extended pixel font for special chars in text.bmp
    extra = {
        '"': [0b101, 0b101, 0b000, 0b000, 0b000],
        '@': [0b111, 0b101, 0b111, 0b100, 0b111],
        '.': [0b000, 0b000, 0b000, 0b000, 0b010],
        ':': [0b000, 0b010, 0b000, 0b010, 0b000],
        '(': [0b010, 0b100, 0b100, 0b100, 0b010],
        ')': [0b010, 0b001, 0b001, 0b001, 0b010],
        '-': [0b000, 0b000, 0b111, 0b000, 0b000],
        "'": [0b010, 0b010, 0b000, 0b000, 0b000],
        '!': [0b010, 0b010, 0b010, 0b000, 0b010],
        '_': [0b000, 0b000, 0b000, 0b000, 0b111],
        '+': [0b000, 0b010, 0b111, 0b010, 0b000],
        '\\': [0b100, 0b100, 0b010, 0b001, 0b001],
        '/': [0b001, 0b001, 0b010, 0b100, 0b100],
        '[': [0b110, 0b100, 0b100, 0b100, 0b110],
        ']': [0b011, 0b001, 0b001, 0b001, 0b011],
        '^': [0b010, 0b101, 0b000, 0b000, 0b000],
        '&': [0b110, 0b101, 0b010, 0b101, 0b110],
        '%': [0b101, 0b001, 0b010, 0b100, 0b101],
        ',': [0b000, 0b000, 0b000, 0b010, 0b100],
        '=': [0b000, 0b111, 0b000, 0b111, 0b000],
        '$': [0b011, 0b110, 0b010, 0b011, 0b110],
        '#': [0b101, 0b111, 0b101, 0b111, 0b101],
    }
    all_glyphs = {**PIXEL_FONT, **extra}

    for row_idx, chars in enumerate([charmap_row0, charmap_row1, charmap_row2]):
        for col_idx, ch in enumerate(chars[:31]):
            x = col_idx * 5 + 1  # center 3px glyph in 5px cell
            y = row_idx * 6
            glyph = all_glyphs.get(ch.upper()) or all_glyphs.get(ch)
            if glyph:
                for ry, row_bits in enumerate(glyph):
                    for col in range(3):
                        if row_bits & (0b100 >> col):
                            draw.point((x + col, y + ry), fill=CYAN)

    save(img, "text.bmp")


# == EQMAIN.BMP (275x315) =================================================

def gen_eqmain():
    img = Image.new("RGB", (275, 315), BG_DARK)
    draw = ImageDraw.Draw(img)

    # EQ window background (0,0, 275x116)
    gradient_v_multi(draw, 0, 0, 275, 116, [
        (0.0, BG_MID),
        (0.12, BG_DARK),
        (0.7, BG_DARK),
        (1.0, BG_DEEPEST),
    ])
    draw.rectangle([0, 0, 274, 115], outline=BORDER_DK)
    draw.line([(1, 0), (273, 0)], fill=BORDER_LT)
    draw.line([(1, 14), (273, 14)], fill=CYAN_DARK)

    # Slider area
    sunken_smooth(draw, 21, 38, 233, 64, DIGIT_BG, depth=18)
    # Graph area
    sunken_smooth(draw, 86, 17, 113, 19, DIGIT_BG, depth=16)

    # Title bars
    gradient_v_multi(draw, 0, 134, 275, 14, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.08)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])
    draw.line([(0, 147), (274, 147)], fill=CYAN_DARK)

    gradient_v(draw, 0, 149, 275, 14, BG_MID, BG_DARK)
    draw.line([(0, 162), (274, 162)], fill=BORDER_DK)

    # Close buttons — bold X, same style as titlebar close
    bevel_smooth(draw, 0, 116, 9, 9, BG_LIGHT, 18)
    for i in range(5):
        draw.point((2+i, 118+i), fill=RED_BRIGHT)
        draw.point((6-i, 118+i), fill=RED_BRIGHT)
        draw.point((3+i, 118+i), fill=RED_SOFT)
        draw.point((5-i, 118+i), fill=RED_SOFT)
    sunken_smooth(draw, 0, 125, 9, 9, BG_MID, 12)
    for i in range(5):
        draw.point((2+i, 127+i), fill=(255, 120, 120))
        draw.point((6-i, 127+i), fill=(255, 120, 120))
        draw.point((3+i, 127+i), fill=RED_BRIGHT)
        draw.point((5-i, 127+i), fill=RED_BRIGHT)

    # ON/AUTO buttons
    for bx, by, lbl, pr, act in [
        (10,119,"ON",False,False), (128,119,"ON",True,False),
        (69,119,"ON",False,True), (187,119,"ON",True,True),
        (36,119,"AUTO",False,False), (154,119,"AUTO",True,False),
        (95,119,"AUTO",False,True), (213,119,"AUTO",True,True),
    ]:
        w = 26 if lbl == "ON" else 32
        button_rich(draw, bx, by, w, 12, pr)
        c = CYAN if act else CYAN_DIM
        px_text_centered(draw, bx, by, w, 12, lbl, c)

    # Slider track backgrounds
    for i in range(28):
        col = i % 14
        row = i // 14
        x = 13 + col * 15
        y = 164 + row * 65
        gradient_v(draw, x, y, 15, 65, lerp(DIGIT_BG, BG_DARK, 0.3), lerp(DIGIT_BG, BG_DARK, 0.5))
        draw.line([(x+7, y), (x+7, y+64)], fill=BORDER_DK)
        for t in range(0, 65, 8):
            draw.line([(x+5, y+t), (x+9, y+t)], fill=lerp(BORDER_DK, BG_DARK, 0.5))

    # Slider thumbs
    bevel_smooth(draw, 0, 164, 11, 11, BG_LIGHTER, 16)
    draw.line([(3, 169), (7, 169)], fill=CYAN)
    bevel_smooth(draw, 0, 176, 11, 11, HIGHLIGHT, 18)
    draw.line([(3, 181), (7, 181)], fill=CYAN_BRIGHT)

    # Presets button
    button_rich(draw, 224, 164, 44, 12, False)
    px_text_centered(draw, 224, 164, 44, 12, "PRESETS", CYAN_DIM)
    button_rich(draw, 224, 176, 44, 12, True)
    px_text_centered(draw, 224, 176, 44, 12, "PRESETS", CYAN)

    # Graph background & line colors
    sunken_smooth(draw, 0, 294, 113, 19, DIGIT_BG, depth=12)
    for i in range(19):
        t = i / 18
        c = lerp(CYAN_BRIGHT, CYAN_DARK, t)
        draw.point((115, 294 + i), fill=c)

    draw.line([(0, 314), (112, 314)], fill=CYAN_DIM)
    save(img, "eqmain.bmp")


# == PLEDIT.BMP (204x110) =================================================

def gen_pledit():
    img = Image.new("RGB", (280, 110), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Selected title bar pieces (y=0)
    gradient_v_multi(draw, 0, 0, 25, 20, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.06)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])
    draw.rectangle([0, 0, 24, 19], outline=BORDER_DK)

    gradient_v_multi(draw, 26, 0, 100, 20, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.06)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])
    draw.line([(26, 19), (125, 19)], fill=CYAN_DARK)

    gradient_v_multi(draw, 127, 0, 25, 20, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.06)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])

    gradient_v_multi(draw, 153, 0, 25, 20, [
        (0.0, lerp(BG_LIGHTER, (255,255,255), 0.06)),
        (0.5, BG_LIGHT),
        (1.0, BG_MID),
    ])
    draw.rectangle([153, 0, 177, 19], outline=BORDER_DK)
    for i in range(5):
        draw.point((165+i, 5+i), fill=RED_BRIGHT)
        draw.point((169-i, 5+i), fill=RED_BRIGHT)
        draw.point((166+i, 5+i), fill=RED_SOFT)
        draw.point((168-i, 5+i), fill=RED_SOFT)

    # Inactive title bar (y=21)
    for x, w in [(0, 25), (26, 100), (127, 25), (153, 25)]:
        gradient_v(draw, x, 21, w, 20, BG_MID, BG_DARK)
    draw.line([(26, 40), (125, 40)], fill=BORDER_DK)

    # Left edge (0,42, 12x29)
    gradient_h(draw, 0, 42, 12, 29, BG_MID, BG_DARK)
    draw.line([(0, 42), (0, 70)], fill=BORDER_DK)

    # Right edge (31,42, 20x29)
    gradient_h(draw, 31, 42, 20, 29, BG_DARK, BG_MID)
    draw.line([(50, 42), (50, 70)], fill=BORDER_DK)

    # Scrollbar handles
    bevel_smooth(draw, 52, 53, 8, 18, BG_LIGHTER, 14)
    draw.line([(55, 57), (55, 66)], fill=CYAN_DIM)
    bevel_smooth(draw, 61, 53, 8, 18, HIGHLIGHT, 16)
    draw.line([(64, 57), (64, 66)], fill=CYAN)

    # Close/shade pressed buttons
    sunken_smooth(draw, 52, 42, 9, 9, BG_MID, 10)
    for i in range(5):
        draw.point((54+i, 44+i), fill=(255, 120, 120))
        draw.point((58-i, 44+i), fill=(255, 120, 120))
        draw.point((55+i, 44+i), fill=RED_BRIGHT)
        draw.point((57-i, 44+i), fill=RED_BRIGHT)
    sunken_smooth(draw, 62, 42, 9, 9, BG_MID, 10)
    draw.line([(64, 44), (68, 44)], fill=CYAN_BRIGHT)
    draw.line([(64, 45), (68, 45)], fill=CYAN_BRIGHT)
    draw.line([(64, 47), (68, 47)], fill=CYAN_BRIGHT)
    draw.line([(64, 48), (68, 48)], fill=CYAN_BRIGHT)

    # Bottom left (0,72, 125x38)
    gradient_v_multi(draw, 0, 72, 125, 38, [
        (0.0, BG_MID),
        (0.3, BG_DARK),
        (1.0, BG_DEEPEST),
    ])
    draw.rectangle([0, 72, 124, 109], outline=BORDER_DK)

    # Bottom right (126,72, 150x38)
    gradient_v_multi(draw, 126, 72, 150, 38, [
        (0.0, BG_MID),
        (0.3, BG_DARK),
        (1.0, BG_DEEPEST),
    ])
    draw.rectangle([126, 72, 275, 109], outline=BORDER_DK)
    # Resize grip in bottom-right corner
    for i in range(4):
        draw.line([(266-i*3, 107), (272, 101+i*3)], fill=CYAN_DIM)
        draw.line([(267-i*3, 107), (272, 102+i*3)], fill=lerp(CYAN_DIM, BG_DARK, 0.5))

    # Bottom tile (179,0, 25x38)
    gradient_v_multi(draw, 179, 0, 25, 38, [
        (0.0, BG_MID),
        (0.5, BG_DARK),
        (1.0, BG_DEEPEST),
    ])

    save(img, "pledit.bmp")


# ==========================================================================

def main():
    os.makedirs(OUT, exist_ok=True)
    print(f"Generating default skin in {OUT}")
    gen_main()
    gen_titlebar()
    gen_cbuttons()
    gen_numbers()
    gen_nums_ex()
    gen_playpaus()
    gen_posbar()
    gen_volume()
    gen_balance()
    gen_shufrep()
    gen_monoster()
    gen_text()
    gen_eqmain()
    gen_pledit()
    print("Done!")


if __name__ == "__main__":
    main()
