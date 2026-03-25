#!/usr/bin/env python3
"""Generate all BMP sprite sheets for the RetroAmp default skin.

Design: dark charcoal/navy with teal-green accents, clean modern-retro aesthetic.
All dimensions match the Winamp 2.x classic skin specification.
"""

import os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "default-skin")

# -- Colour palette -------------------------------------------------------

BG_DARK    = (22, 28, 42)     # deepest background
BG_MID     = (30, 38, 56)     # surface / panels
BG_LIGHT   = (42, 52, 72)     # raised elements
BORDER     = (55, 65, 90)     # subtle borders
HIGHLIGHT  = (70, 82, 110)    # hover / light edge
ACCENT     = (0, 200, 120)    # teal-green (matches viscolor)
ACCENT_DIM = (0, 140, 84)     # dimmer accent
TEXT_COL   = (200, 210, 225)  # text colour
WHITE      = (255, 255, 255)
BLACK      = (0, 0, 0)


def save(img: Image.Image, name: str):
    path = os.path.join(OUT, name)
    img.save(path)
    print(f"  wrote {name} ({img.width}x{img.height})")


def gradient_v(draw, x, y, w, h, c1, c2):
    """Vertical gradient from c1 (top) to c2 (bottom)."""
    for row in range(h):
        t = row / max(h - 1, 1)
        r = int(c1[0] + (c2[0] - c1[0]) * t)
        g = int(c1[1] + (c2[1] - c1[1]) * t)
        b = int(c1[2] + (c2[2] - c1[2]) * t)
        draw.line([(x, y + row), (x + w - 1, y + row)], fill=(r, g, b))


def gradient_h(draw, x, y, w, h, c1, c2):
    """Horizontal gradient from c1 (left) to c2 (right)."""
    for col in range(w):
        t = col / max(w - 1, 1)
        r = int(c1[0] + (c2[0] - c1[0]) * t)
        g = int(c1[1] + (c2[1] - c1[1]) * t)
        b = int(c1[2] + (c2[2] - c1[2]) * t)
        draw.line([(x + col, y), (x + col, y + h - 1)], fill=(r, g, b))


def bevel(draw, x, y, w, h, base, light_offset=20, dark_offset=20):
    """Draw a raised bevel rectangle."""
    light = tuple(min(c + light_offset, 255) for c in base)
    dark = tuple(max(c - dark_offset, 0) for c in base)
    draw.rectangle([x, y, x + w - 1, y + h - 1], fill=base)
    draw.line([(x, y), (x + w - 1, y)], fill=light)
    draw.line([(x, y), (x, y + h - 1)], fill=light)
    draw.line([(x + w - 1, y), (x + w - 1, y + h - 1)], fill=dark)
    draw.line([(x, y + h - 1), (x + w - 1, y + h - 1)], fill=dark)


def sunken(draw, x, y, w, h, base, offset=15):
    """Draw a sunken/inset rectangle."""
    light = tuple(min(c + offset, 255) for c in base)
    dark = tuple(max(c - offset, 0) for c in base)
    draw.rectangle([x, y, x + w - 1, y + h - 1], fill=base)
    draw.line([(x, y), (x + w - 1, y)], fill=dark)
    draw.line([(x, y), (x, y + h - 1)], fill=dark)
    draw.line([(x + w - 1, y + 1), (x + w - 1, y + h - 1)], fill=light)
    draw.line([(x + 1, y + h - 1), (x + w - 1, y + h - 1)], fill=light)


def button(draw, x, y, w, h, pressed=False):
    """Draw a button (raised or pressed)."""
    if pressed:
        sunken(draw, x, y, w, h, BG_MID, offset=12)
    else:
        bevel(draw, x, y, w, h, BG_LIGHT, light_offset=18, dark_offset=18)


# == MAIN.BMP (275x116) ===================================================

def gen_main():
    img = Image.new("RGB", (275, 116), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Background gradient
    gradient_v(draw, 0, 0, 275, 116, BG_DARK, (18, 24, 36))

    # Title bar area (top 14px is overlaid by titlebar.bmp, but we still fill)
    draw.rectangle([0, 0, 274, 13], fill=BG_MID)

    # Visualizer/spectrum area (inset at 24,43 size 76x16)
    sunken(draw, 24, 43, 76, 16, (12, 16, 24))

    # Clutterbar area left (11x43 at 10,22)
    sunken(draw, 10, 22, 8, 37, BG_DARK)

    # Song title area (inset at 111,24 size 153x14)
    sunken(draw, 111, 24, 153, 12, (12, 16, 24))

    # Info text area (bitrate/freq)
    sunken(draw, 111, 43, 153, 16, (12, 16, 24))

    # Time display background (at 48,26 size 63x13)
    sunken(draw, 36, 26, 63, 13, (8, 10, 16))

    # Control buttons area
    draw.rectangle([16, 88, 129, 107], fill=BG_DARK)

    # Volume area
    draw.rectangle([107, 57, 175, 70], fill=BG_DARK)

    # Balance area
    draw.rectangle([177, 57, 237, 70], fill=BG_DARK)

    # Seek bar area
    draw.rectangle([16, 72, 264, 81], fill=BG_DARK)

    # Bottom area for shuffle/repeat/eq/pl buttons
    draw.rectangle([164, 89, 274, 107], fill=BG_DARK)

    # Subtle border around the whole window
    draw.rectangle([0, 0, 274, 115], outline=BORDER)

    # Accent line under titlebar
    draw.line([(1, 14), (273, 14)], fill=ACCENT_DIM)

    save(img, "main.bmp")


# == TITLEBAR.BMP (302x56) ================================================

def gen_titlebar():
    img = Image.new("RGB", (302, 56), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Selected (active) title bar: row y=0, starts at x=27
    gradient_h(draw, 27, 0, 275, 14, BG_MID, BG_LIGHT)
    draw.line([(27, 13), (301, 13)], fill=ACCENT_DIM)
    # Title text area — "RETROAMP"
    for i, ch in enumerate("R E T R O A M P"):
        px = 27 + 80 + i * 7
        if px < 302 and ch != ' ':
            draw.rectangle([px, 3, px + 4, 10], fill=ACCENT)

    # Inactive title bar: row y=15
    gradient_h(draw, 27, 15, 275, 14, BG_DARK, BG_MID)
    draw.line([(27, 28), (301, 28)], fill=BORDER)

    # Shade mode selected: row y=29
    gradient_h(draw, 27, 29, 275, 14, BG_MID, BG_LIGHT)
    draw.line([(27, 42), (301, 42)], fill=ACCENT_DIM)

    # Shade mode inactive: row y=42
    gradient_h(draw, 27, 42, 275, 14, BG_DARK, BG_MID)

    # -- Title bar buttons (9x9 each) in the left 27x27 area --

    # Options button (x=0,y=0 normal; x=0,y=9 pressed)
    bevel(draw, 0, 0, 9, 9, BG_LIGHT, 15, 15)
    draw.rectangle([2, 3, 6, 3], fill=ACCENT_DIM)
    draw.rectangle([2, 5, 6, 5], fill=ACCENT_DIM)
    sunken(draw, 0, 9, 9, 9, BG_MID)
    draw.rectangle([2, 12, 6, 12], fill=ACCENT)
    draw.rectangle([2, 14, 6, 14], fill=ACCENT)

    # Minimize button (x=9,y=0 normal; x=9,y=9 pressed)
    bevel(draw, 9, 0, 9, 9, BG_LIGHT, 15, 15)
    draw.line([(11, 6), (15, 6)], fill=ACCENT_DIM)
    sunken(draw, 9, 9, 9, 9, BG_MID)
    draw.line([(11, 15), (15, 15)], fill=ACCENT)

    # Shade button (x=0,y=18 normal; x=9,y=18 pressed)
    bevel(draw, 0, 18, 9, 9, BG_LIGHT, 15, 15)
    draw.line([(2, 22), (6, 22)], fill=ACCENT_DIM)
    draw.line([(2, 24), (6, 24)], fill=ACCENT_DIM)
    sunken(draw, 9, 18, 9, 9, BG_MID)
    draw.line([(11, 22), (15, 22)], fill=ACCENT)
    draw.line([(11, 24), (15, 24)], fill=ACCENT)

    # Close button (x=18,y=0 normal; x=18,y=9 pressed)
    bevel(draw, 18, 0, 9, 9, BG_LIGHT, 15, 15)
    for i in range(5):
        draw.point((20 + i, 2 + i), fill=(200, 80, 80))
        draw.point((24 - i, 2 + i), fill=(200, 80, 80))
    sunken(draw, 18, 9, 9, 9, BG_MID)
    for i in range(5):
        draw.point((20 + i, 11 + i), fill=(255, 100, 100))
        draw.point((24 - i, 11 + i), fill=(255, 100, 100))

    save(img, "titlebar.bmp")


# == CBUTTONS.BMP (136x36) ================================================

def gen_cbuttons():
    img = Image.new("RGB", (136, 36), BG_DARK)
    draw = ImageDraw.Draw(img)

    btns = [(0, 23, 18), (23, 23, 18), (46, 23, 18), (69, 23, 18),
            (92, 22, 18), (114, 22, 16)]

    for x, w, h in btns:
        button(draw, x, 0, w, h, pressed=False)
        button(draw, x, h, w, h, pressed=True)

    # Previous: |<<
    for yo in [0, 18]:
        c = ACCENT
        draw.polygon([(9, 5+yo), (9, 12+yo), (4, 8+yo)], fill=c)
        draw.polygon([(17, 5+yo), (17, 12+yo), (12, 8+yo)], fill=c)
        draw.line([(3, 5+yo), (3, 12+yo)], fill=c)

    # Play: >
    for yo in [0, 18]:
        draw.polygon([(28, 4+yo), (28, 13+yo), (38, 8+yo)], fill=ACCENT)

    # Pause: ||
    for yo in [0, 18]:
        draw.rectangle([51, 4+yo, 54, 13+yo], fill=ACCENT)
        draw.rectangle([57, 4+yo, 60, 13+yo], fill=ACCENT)

    # Stop: square
    for yo in [0, 18]:
        draw.rectangle([74, 4+yo, 83, 13+yo], fill=ACCENT)

    # Next: >>|
    for yo in [0, 18]:
        draw.polygon([(96, 5+yo), (96, 12+yo), (101, 8+yo)], fill=ACCENT)
        draw.polygon([(103, 5+yo), (103, 12+yo), (108, 8+yo)], fill=ACCENT)
        draw.line([(110, 5+yo), (110, 12+yo)], fill=ACCENT)

    # Eject: triangle + line
    for yo in [0, 16]:
        draw.polygon([(120, 10+yo), (130, 10+yo), (125, 4+yo)], fill=ACCENT)
        draw.line([(120, 12+yo), (130, 12+yo)], fill=ACCENT)

    save(img, "cbuttons.bmp")


# == NUMBERS.BMP and NUMS_EX.BMP ==========================================

def draw_digit(draw, x, y, digit, color=ACCENT):
    """Draw a 7-segment style digit at (x,y) in a 9x13 cell."""
    segs = {
        0: 'abcdef', 1: 'bc', 2: 'abdeg', 3: 'abcdg',
        4: 'bcfg', 5: 'acdfg', 6: 'acdefg', 7: 'abc',
        8: 'abcdefg', 9: 'abcdfg',
    }
    active = segs.get(digit, '')
    dim = tuple(max(c - 160, 0) for c in color)

    def seg(name, on):
        c = color if on else dim
        w = 2  # segment width
        if name == 'a':
            draw.line([(x+3, y+1), (x+6, y+1)], fill=c)
        elif name == 'b':
            draw.line([(x+7, y+2), (x+7, y+5)], fill=c)
        elif name == 'c':
            draw.line([(x+7, y+7), (x+7, y+10)], fill=c)
        elif name == 'd':
            draw.line([(x+3, y+11), (x+6, y+11)], fill=c)
        elif name == 'e':
            draw.line([(x+1, y+7), (x+1, y+10)], fill=c)
        elif name == 'f':
            draw.line([(x+1, y+2), (x+1, y+5)], fill=c)
        elif name == 'g':
            draw.line([(x+3, y+6), (x+6, y+6)], fill=c)

    for s in 'abcdefg':
        seg(s, s in active)


def gen_numbers():
    img = Image.new("RGB", (99, 13), (8, 10, 16))
    draw = ImageDraw.Draw(img)
    for i in range(10):
        draw_digit(draw, i * 9, 0, i)
    save(img, "numbers.bmp")


def gen_nums_ex():
    img = Image.new("RGB", (108, 13), (8, 10, 16))
    draw = ImageDraw.Draw(img)
    for i in range(10):
        draw_digit(draw, i * 9, 0, i)
    # Minus sign at x=90
    draw.line([(93, 6), (96, 6)], fill=ACCENT)
    save(img, "nums_ex.bmp")


# == PLAYPAUS.BMP (33x9) ==================================================

def gen_playpaus():
    img = Image.new("RGB", (33, 9), BG_DARK)
    draw = ImageDraw.Draw(img)
    # Playing indicator: green triangle
    draw.polygon([(1, 1), (1, 7), (7, 4)], fill=ACCENT)
    # Paused indicator: two bars
    draw.rectangle([10, 1, 12, 7], fill=ACCENT)
    draw.rectangle([15, 1, 17, 7], fill=ACCENT)
    # Stopped indicator: square
    draw.rectangle([19, 1, 26, 7], fill=ACCENT_DIM)
    # Not working / working indicators
    draw.rectangle([27, 0, 29, 8], fill=BG_DARK)
    draw.rectangle([30, 0, 32, 8], fill=ACCENT)
    save(img, "playpaus.bmp")


# == POSBAR.BMP (307x10) ==================================================

def gen_posbar():
    img = Image.new("RGB", (307, 10), BG_DARK)
    draw = ImageDraw.Draw(img)
    sunken(draw, 0, 0, 248, 10, BG_DARK, offset=8)
    gradient_h(draw, 1, 1, 246, 8, (12, 16, 24), (16, 20, 30))
    bevel(draw, 248, 0, 29, 10, BG_LIGHT, 12, 12)
    draw.rectangle([253, 3, 253, 6], fill=ACCENT)
    draw.rectangle([261, 3, 261, 6], fill=ACCENT)
    sunken(draw, 278, 0, 29, 10, BG_MID, offset=8)
    draw.rectangle([283, 3, 283, 6], fill=ACCENT)
    draw.rectangle([291, 3, 291, 6], fill=ACCENT)
    save(img, "posbar.bmp")


# == VOLUME.BMP (68x433) ==================================================

def gen_volume():
    img = Image.new("RGB", (68, 433), BG_DARK)
    draw = ImageDraw.Draw(img)
    for i in range(28):
        y = i * 15
        draw.rectangle([0, y, 67, y + 14], fill=BG_DARK)
        sunken(draw, 4, y + 2, 60, 10, (12, 16, 24), offset=6)
        fill_w = int((i / 27) * 56)
        if fill_w > 0:
            gradient_h(draw, 6, y + 4, fill_w, 6, ACCENT_DIM, ACCENT)
    bevel(draw, 15, 422, 14, 11, BG_LIGHT, 14, 14)
    draw.line([(20, 425), (20, 429)], fill=ACCENT)
    draw.line([(24, 425), (24, 429)], fill=ACCENT)
    sunken(draw, 0, 422, 14, 11, BG_MID, offset=8)
    draw.line([(5, 425), (5, 429)], fill=ACCENT)
    draw.line([(9, 425), (9, 429)], fill=ACCENT)
    save(img, "volume.bmp")


# == BALANCE.BMP (47x433) =================================================

def gen_balance():
    img = Image.new("RGB", (47, 433), BG_DARK)
    draw = ImageDraw.Draw(img)
    for i in range(28):
        y = i * 15
        draw.rectangle([9, y, 46, y + 14], fill=BG_DARK)
        sunken(draw, 12, y + 2, 32, 10, (12, 16, 24), offset=6)
        draw.line([(28, y + 4), (28, y + 9)], fill=ACCENT_DIM)
    bevel(draw, 15, 422, 14, 11, BG_LIGHT, 14, 14)
    draw.line([(20, 425), (20, 429)], fill=ACCENT)
    draw.line([(24, 425), (24, 429)], fill=ACCENT)
    sunken(draw, 0, 422, 14, 11, BG_MID, offset=8)
    draw.line([(5, 425), (5, 429)], fill=ACCENT)
    draw.line([(9, 425), (9, 429)], fill=ACCENT)
    save(img, "balance.bmp")


# == SHUFREP.BMP (92x85) ==================================================

def gen_shufrep():
    img = Image.new("RGB", (92, 85), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Repeat button: x=0, 28x15, 4 rows
    for row, active in [(0, False), (15, True), (30, False), (45, True)]:
        pressed = row >= 30
        button(draw, 0, row, 28, 15, pressed)
        c = ACCENT if active else ACCENT_DIM
        # Loop arrow icon
        draw.arc([4, row+3, 24, row+12], 0, 360, fill=c)

    # Shuffle button: x=28, 47x15, 4 rows
    for row, active in [(0, False), (15, True), (30, False), (45, True)]:
        pressed = row >= 30
        button(draw, 28, row, 47, 15, pressed)
        c = ACCENT if active else ACCENT_DIM
        draw.line([(34, row+5), (46, row+10)], fill=c)
        draw.line([(34, row+10), (46, row+5)], fill=c)

    # EQ button: 23x12
    for col, y, pressed, active in [
        (0, 61, False, False), (46, 61, False, True),
        (0, 73, True, False), (46, 73, True, True),
    ]:
        button(draw, col, y, 23, 12, pressed)
        c = ACCENT if active else ACCENT_DIM
        draw.text((col+4, y+2), "EQ", fill=c)

    # PL button: 23x12
    for col, y, pressed, active in [
        (23, 61, False, False), (69, 61, False, True),
        (23, 73, True, False), (69, 73, True, True),
    ]:
        button(draw, col, y, 23, 12, pressed)
        c = ACCENT if active else ACCENT_DIM
        draw.text((col+4, y+2), "PL", fill=c)

    save(img, "shufrep.bmp")


# == MONOSTER.BMP (56x24) =================================================

def gen_monoster():
    img = Image.new("RGB", (56, 24), BG_DARK)
    draw = ImageDraw.Draw(img)
    # Stereo active (0,0) / inactive (0,12)
    draw.text((2, 1), "STEREO", fill=ACCENT)
    draw.text((2, 13), "STEREO", fill=ACCENT_DIM)
    # Mono active (29,0) / inactive (29,12)
    draw.text((31, 1), "MONO", fill=ACCENT)
    draw.text((31, 13), "MONO", fill=ACCENT_DIM)
    save(img, "monoster.bmp")


# == TEXT.BMP (155x18) =====================================================

def gen_text():
    """Bitmap font for scrolling title text. 5x6 cells, 31 per row, 3 rows."""
    img = Image.new("RGB", (155, 18), BG_DARK)
    draw = ImageDraw.Draw(img)

    charmap_row0 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ"@   '
    charmap_row1 = '0123456789...:()-\'!_+\\/[]^&%,=$#'
    charmap_row2 = '                               '

    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 6)
    except Exception:
        font = ImageFont.load_default()

    for row_idx, chars in enumerate([charmap_row0, charmap_row1, charmap_row2]):
        for col_idx, ch in enumerate(chars[:31]):
            x = col_idx * 5
            y = row_idx * 6
            if ch.strip():
                draw.text((x, y), ch, fill=ACCENT, font=font)

    save(img, "text.bmp")


# == EQMAIN.BMP (275x315) =================================================

def gen_eqmain():
    img = Image.new("RGB", (275, 315), BG_DARK)
    draw = ImageDraw.Draw(img)

    # EQ_WINDOW_BACKGROUND (0,0, 275x116)
    gradient_v(draw, 0, 0, 275, 116, BG_DARK, (18, 24, 36))
    draw.rectangle([0, 0, 274, 115], outline=BORDER)
    draw.line([(1, 14), (273, 14)], fill=ACCENT_DIM)
    sunken(draw, 21, 38, 233, 64, (12, 16, 24), offset=6)
    sunken(draw, 86, 17, 113, 19, (12, 16, 24), offset=6)

    # EQ_TITLE_BAR_SELECTED (0,134, 275x14)
    gradient_h(draw, 0, 134, 275, 14, BG_MID, BG_LIGHT)
    draw.line([(0, 147), (274, 147)], fill=ACCENT_DIM)

    # EQ_TITLE_BAR inactive (0,149, 275x14)
    gradient_h(draw, 0, 149, 275, 14, BG_DARK, BG_MID)
    draw.line([(0, 162), (274, 162)], fill=BORDER)

    # Close buttons
    bevel(draw, 0, 116, 9, 9, BG_LIGHT, 15, 15)
    for i in range(5):
        draw.point((2 + i, 118 + i), fill=(200, 80, 80))
        draw.point((6 - i, 118 + i), fill=(200, 80, 80))
    sunken(draw, 0, 125, 9, 9, BG_MID)
    for i in range(5):
        draw.point((2 + i, 127 + i), fill=(255, 100, 100))
        draw.point((6 - i, 127 + i), fill=(255, 100, 100))

    # ON/AUTO buttons
    for bx, by, lbl, pr, act in [
        (10,119,"ON",False,False), (128,119,"ON",True,False),
        (69,119,"ON",False,True), (187,119,"ON",True,True),
        (36,119,"AUTO",False,False), (154,119,"AUTO",True,False),
        (95,119,"AUTO",False,True), (213,119,"AUTO",True,True),
    ]:
        w = 26 if lbl == "ON" else 32
        button(draw, bx, by, w, 12, pr)
        c = ACCENT if act else ACCENT_DIM
        draw.text((bx+3, by+2), lbl, fill=c)

    # Slider backgrounds (13,164, 209x129)
    for i in range(28):
        col = i % 14
        row = i // 14
        x = 13 + col * 15
        y = 164 + row * 65
        gradient_v(draw, x, y, 15, 65, (12, 16, 24), (16, 20, 30))
        draw.line([(x + 7, y), (x + 7, y + 64)], fill=BORDER)
        for t in range(0, 65, 8):
            draw.line([(x + 5, y + t), (x + 9, y + t)], fill=BORDER)

    # Slider thumbs
    bevel(draw, 0, 164, 11, 11, BG_LIGHT, 16, 16)
    draw.line([(2, 169), (8, 169)], fill=ACCENT)
    bevel(draw, 0, 176, 11, 11, HIGHLIGHT, 16, 16)
    draw.line([(2, 181), (8, 181)], fill=ACCENT)

    # Presets button
    button(draw, 224, 164, 44, 12, False)
    draw.text((228, 166), "PRESETS", fill=ACCENT_DIM)
    button(draw, 224, 176, 44, 12, True)
    draw.text((228, 178), "PRESETS", fill=ACCENT)

    # Graph background
    sunken(draw, 0, 294, 113, 19, (10, 14, 20), offset=6)

    # Graph line colors (vertical strip)
    for i in range(19):
        t = i / 18
        r = int(ACCENT[0] * (1 - t))
        g = int(ACCENT[1] * (1 - t) + 100 * t)
        b = int(ACCENT[2] * (1 - t))
        draw.point((115, 294 + i), fill=(r, g, b))

    # Preamp line
    draw.line([(0, 314), (112, 314)], fill=ACCENT_DIM)

    save(img, "eqmain.bmp")


# == PLEDIT.BMP (204x110) =================================================

def gen_pledit():
    img = Image.new("RGB", (204, 110), BG_DARK)
    draw = ImageDraw.Draw(img)

    # Selected title bar pieces (y=0)
    gradient_v(draw, 0, 0, 25, 20, BG_MID, BG_LIGHT)
    draw.rectangle([0, 0, 24, 19], outline=BORDER)

    gradient_h(draw, 26, 0, 100, 20, BG_MID, BG_LIGHT)
    draw.line([(26, 19), (125, 19)], fill=ACCENT_DIM)

    gradient_v(draw, 127, 0, 25, 20, BG_MID, BG_LIGHT)
    gradient_v(draw, 153, 0, 25, 20, BG_MID, BG_LIGHT)
    draw.rectangle([153, 0, 177, 19], outline=BORDER)
    # Close X
    for i in range(5):
        draw.point((165 + i, 5 + i), fill=(200, 80, 80))
        draw.point((169 - i, 5 + i), fill=(200, 80, 80))

    # Inactive title bar (y=21)
    gradient_v(draw, 0, 21, 25, 20, BG_DARK, BG_MID)
    gradient_h(draw, 26, 21, 100, 20, BG_DARK, BG_MID)
    draw.line([(26, 40), (125, 40)], fill=BORDER)
    gradient_v(draw, 127, 21, 25, 20, BG_DARK, BG_MID)
    gradient_v(draw, 153, 21, 25, 20, BG_DARK, BG_MID)

    # Left edge tile (0,42, 12x29)
    gradient_h(draw, 0, 42, 12, 29, BG_MID, BG_DARK)
    draw.line([(0, 42), (0, 70)], fill=BORDER)

    # Right edge tile (31,42, 20x29)
    gradient_h(draw, 31, 42, 20, 29, BG_DARK, BG_MID)
    draw.line([(50, 42), (50, 70)], fill=BORDER)

    # Scrollbar handles
    bevel(draw, 52, 53, 8, 18, BG_LIGHT, 12, 12)
    draw.line([(55, 57), (55, 66)], fill=ACCENT_DIM)
    bevel(draw, 61, 53, 8, 18, HIGHLIGHT, 12, 12)
    draw.line([(64, 57), (64, 66)], fill=ACCENT)

    # Close/shade selected buttons
    sunken(draw, 52, 42, 9, 9, BG_MID)
    for i in range(5):
        draw.point((54 + i, 44 + i), fill=(255, 100, 100))
        draw.point((58 - i, 44 + i), fill=(255, 100, 100))
    sunken(draw, 62, 42, 9, 9, BG_MID)
    draw.line([(64, 45), (68, 45)], fill=ACCENT)
    draw.line([(64, 47), (68, 47)], fill=ACCENT)

    # Bottom left (0,72, 125x38)
    gradient_v(draw, 0, 72, 125, 38, BG_MID, BG_DARK)
    draw.rectangle([0, 72, 124, 109], outline=BORDER)

    # Bottom right (126,72, 150x38)
    gradient_v(draw, 126, 72, 78, 38, BG_MID, BG_DARK)
    draw.rectangle([126, 72, 203, 109], outline=BORDER)
    for i in range(3):
        draw.line([(195 - i*3, 107), (201, 101 + i*3)], fill=ACCENT_DIM)

    # Bottom tile (179,0, 25x38)
    gradient_v(draw, 179, 0, 25, 38, BG_MID, BG_DARK)

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
