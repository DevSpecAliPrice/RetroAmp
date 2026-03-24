#!/usr/bin/env python3
"""Generate the RetroAmp built-in default skin.

Creates minimal BMP sprite sheets for a functional Winamp 2.x classic skin.
The palette is a dark charcoal theme with green accents — clearly distinct
from any existing Winamp skin.

Run:  python3 scripts/generate_default_skin.py
"""

import os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "default-skin")
os.makedirs(OUT, exist_ok=True)

# -- Palette --
BG = (34, 34, 40)          # Dark charcoal background
BG_LIGHT = (48, 48, 56)    # Slightly lighter panel areas
ACCENT = (0, 200, 120)     # Green accent
ACCENT_DIM = (0, 120, 72)  # Dimmed accent
BUTTON = (58, 58, 68)      # Button face
BUTTON_PRESS = (38, 38, 48) # Button pressed
TEXT_CLR = (0, 200, 120)   # Text colour (green)
TEXT_DIM = (0, 100, 60)    # Dim text
BORDER = (70, 70, 82)      # Subtle border
THUMB = (100, 100, 115)    # Slider thumb
THUMB_PRESS = (130, 130, 150)
BLACK = (0, 0, 0)
WHITE = (200, 200, 210)
SLIDER_TRACK = (28, 28, 34)


def save(img: Image.Image, name: str):
    path = os.path.join(OUT, name)
    img.save(path)
    print(f"  {name} ({img.width}x{img.height})")


def draw_button_outline(draw, x, y, w, h, pressed=False):
    """Draw a subtle raised/pressed button outline."""
    c = BUTTON_PRESS if pressed else BUTTON
    draw.rectangle([x, y, x + w - 1, y + h - 1], fill=c, outline=BORDER)


# ============================================================
# main.bmp  (275 x 116)
# ============================================================
def gen_main():
    img = Image.new("RGB", (275, 116), BG)
    draw = ImageDraw.Draw(img)
    # Title bar region (y=0..13) — darker strip
    draw.rectangle([0, 0, 274, 13], fill=BG_LIGHT)
    # Visualizer area placeholder (x=24, y=43, 76x16)
    draw.rectangle([24, 43, 99, 58], fill=BLACK)
    # Divider lines
    draw.line([(0, 14), (274, 14)], fill=BORDER)
    draw.line([(0, 71), (274, 71)], fill=BORDER)
    save(img, "main.bmp")


# ============================================================
# titlebar.bmp  (275+27 x 56)
# Title bars start at x=27. Buttons are at x=0..26.
# ============================================================
def gen_titlebar():
    w, h = 302, 56
    img = Image.new("RGB", (w, h), BG)
    draw = ImageDraw.Draw(img)

    # Active title bar (27, 0, 275x14)
    draw.rectangle([27, 0, 301, 13], fill=BG_LIGHT)
    # "RETROAMP" label
    draw_tiny_text(draw, 30 + 90, 4, "retroamp", TEXT_CLR)

    # Inactive title bar (27, 15, 275x14)
    draw.rectangle([27, 15, 301, 28], fill=BG)
    draw_tiny_text(draw, 30 + 90, 19, "retroamp", TEXT_DIM)

    # Shade background active (27, 29, 275x14)
    draw.rectangle([27, 29, 301, 42], fill=BG_LIGHT)
    # Shade background inactive (27, 42, 275x14)
    draw.rectangle([27, 42, 301, 55], fill=BG)

    # Title bar buttons — 3 columns of 9x9 at (0,0), (9,0), (18,0)
    # Row 0 = normal, Row 1 = pressed (y+9), Row 2 = shade variants (y+18)
    for col, cx in enumerate([0, 9, 18]):
        # Normal state
        draw.rectangle([cx, 0, cx + 8, 8], fill=BUTTON, outline=BORDER)
        # Pressed state
        draw.rectangle([cx, 9, cx + 8, 17], fill=BUTTON_PRESS, outline=BORDER)
        # Shade normal
        draw.rectangle([cx, 18, cx + 8, 26], fill=BUTTON, outline=BORDER)

    # Close button markers (col 2 = x=18)
    draw.line([(20, 2), (24, 6)], fill=ACCENT)
    draw.line([(24, 2), (20, 6)], fill=ACCENT)
    draw.line([(20, 11), (24, 15)], fill=ACCENT_DIM)
    draw.line([(24, 11), (20, 15)], fill=ACCENT_DIM)

    # Minimize marker (col 1 = x=9)
    draw.line([(11, 5), (15, 5)], fill=ACCENT)
    draw.line([(11, 14), (15, 14)], fill=ACCENT_DIM)

    # Shade marker (col 0 = x=0, row 2)
    draw.rectangle([2, 20, 6, 24], outline=ACCENT)

    save(img, "titlebar.bmp")


# ============================================================
# cbuttons.bmp  (136 x 36)
# ============================================================
def gen_cbuttons():
    img = Image.new("RGB", (137, 36), BG)
    draw = ImageDraw.Draw(img)

    buttons = [
        (0, 23, "|<"),     # Previous
        (23, 23, ">"),     # Play
        (46, 23, "||"),    # Pause
        (69, 23, "[]"),    # Stop
        (92, 22, ">|"),    # Next
        (114, 22, "^"),    # Eject
    ]

    for bx, bw, label in buttons:
        # Normal (y=0)
        draw_button_outline(draw, bx, 0, bw, 18)
        # Pressed (y=18)
        draw_button_outline(draw, bx, 18, bw, 18, pressed=True)

    # Draw transport icons as simple shapes
    # Previous |<< (x=0)
    draw.rectangle([6, 4, 8, 13], fill=ACCENT)
    draw.polygon([(10, 4), (18, 9), (10, 13)], fill=ACCENT)
    draw.rectangle([6, 22, 8, 31], fill=ACCENT_DIM)
    draw.polygon([(10, 22), (18, 27), (10, 31)], fill=ACCENT_DIM)

    # Play > (x=23)
    draw.polygon([(29, 4), (40, 9), (29, 13)], fill=ACCENT)
    draw.polygon([(29, 22), (40, 27), (29, 31)], fill=ACCENT_DIM)

    # Pause || (x=46)
    draw.rectangle([52, 4, 55, 13], fill=ACCENT)
    draw.rectangle([58, 4, 61, 13], fill=ACCENT)
    draw.rectangle([52, 22, 55, 31], fill=ACCENT_DIM)
    draw.rectangle([58, 22, 61, 31], fill=ACCENT_DIM)

    # Stop [] (x=69)
    draw.rectangle([75, 4, 84, 13], fill=ACCENT)
    draw.rectangle([75, 22, 84, 31], fill=ACCENT_DIM)

    # Next >>| (x=92)
    draw.polygon([(96, 4), (104, 9), (96, 13)], fill=ACCENT)
    draw.rectangle([106, 4, 108, 13], fill=ACCENT)
    draw.polygon([(96, 22), (104, 27), (96, 31)], fill=ACCENT_DIM)
    draw.rectangle([106, 22, 108, 31], fill=ACCENT_DIM)

    # Eject ^ (x=114)
    draw.polygon([(121, 3), (130, 10), (121, 10)], fill=ACCENT)
    draw.polygon([(121, 21), (130, 28), (121, 28)], fill=ACCENT_DIM)

    save(img, "cbuttons.bmp")


# ============================================================
# numbers.bmp  (90 x 13)
# Digits 0-9, each 9x13 px
# ============================================================
def gen_numbers():
    img = Image.new("RGB", (99, 13), BLACK)
    draw = ImageDraw.Draw(img)

    digit_patterns = {
        0: [(1,0,7,0),(0,1,0,11),(7,1,7,11),(1,12,7,12)],
        1: [(4,0,4,12)],
        2: [(1,0,7,0),(7,1,7,5),(1,6,7,6),(0,7,0,11),(1,12,7,12)],
        3: [(1,0,7,0),(7,1,7,5),(2,6,7,6),(7,7,7,11),(1,12,7,12)],
        4: [(0,0,0,5),(7,0,7,12),(1,6,6,6)],
        5: [(1,0,7,0),(0,1,0,5),(1,6,7,6),(7,7,7,11),(1,12,7,12)],
        6: [(1,0,7,0),(0,1,0,11),(1,6,7,6),(7,7,7,11),(1,12,7,12)],
        7: [(1,0,7,0),(7,1,7,12)],
        8: [(1,0,7,0),(0,1,0,5),(7,1,7,5),(1,6,7,6),(0,7,0,11),(7,7,7,11),(1,12,7,12)],
        9: [(1,0,7,0),(0,1,0,5),(7,1,7,11),(1,6,7,6),(1,12,7,12)],
    }

    for d in range(10):
        ox = d * 9
        for seg in digit_patterns[d]:
            x1, y1, x2, y2 = seg
            draw.line([(ox + x1, y1), (ox + x2, y2)], fill=ACCENT)

    save(img, "numbers.bmp")


# ============================================================
# nums_ex.bmp  (108 x 13) — extended digits (0-9, minus, blank)
# ============================================================
def gen_nums_ex():
    img = Image.new("RGB", (108, 13), BLACK)
    draw = ImageDraw.Draw(img)

    digit_patterns = {
        0: [(1,0,7,0),(0,1,0,11),(7,1,7,11),(1,12,7,12)],
        1: [(4,0,4,12)],
        2: [(1,0,7,0),(7,1,7,5),(1,6,7,6),(0,7,0,11),(1,12,7,12)],
        3: [(1,0,7,0),(7,1,7,5),(2,6,7,6),(7,7,7,11),(1,12,7,12)],
        4: [(0,0,0,5),(7,0,7,12),(1,6,6,6)],
        5: [(1,0,7,0),(0,1,0,5),(1,6,7,6),(7,7,7,11),(1,12,7,12)],
        6: [(1,0,7,0),(0,1,0,11),(1,6,7,6),(7,7,7,11),(1,12,7,12)],
        7: [(1,0,7,0),(7,1,7,12)],
        8: [(1,0,7,0),(0,1,0,5),(7,1,7,5),(1,6,7,6),(0,7,0,11),(7,7,7,11),(1,12,7,12)],
        9: [(1,0,7,0),(0,1,0,5),(7,1,7,11),(1,6,7,6),(1,12,7,12)],
    }

    for d in range(10):
        ox = d * 9
        for seg in digit_patterns[d]:
            x1, y1, x2, y2 = seg
            draw.line([(ox + x1, y1), (ox + x2, y2)], fill=ACCENT)

    # Minus sign at slot 10 (x=90)
    draw.line([(92, 6), (96, 6)], fill=ACCENT)
    # Slot 11 (x=99) = blank (already black)

    save(img, "nums_ex.bmp")


# ============================================================
# playpaus.bmp  (33 x 9)
# ============================================================
def gen_playpaus():
    img = Image.new("RGB", (33, 9), BLACK)
    draw = ImageDraw.Draw(img)

    # Playing indicator (0,0,9x9) — green triangle
    draw.polygon([(1, 1), (7, 4), (1, 7)], fill=ACCENT)
    # Paused indicator (9,0,9x9) — two bars
    draw.rectangle([10, 1, 12, 7], fill=ACCENT)
    draw.rectangle([14, 1, 16, 7], fill=ACCENT)
    # Stopped indicator (18,0,9x9) — square
    draw.rectangle([19, 1, 25, 7], fill=ACCENT_DIM)
    # Not-working (27,0,3x9) — dim
    draw.rectangle([27, 0, 29, 8], fill=BG)
    # Working (30,0,3x9) — bright dot
    draw.rectangle([30, 3, 32, 5], fill=ACCENT)

    save(img, "playpaus.bmp")


# ============================================================
# posbar.bmp  (307 x 10)
# ============================================================
def gen_posbar():
    img = Image.new("RGB", (307, 10), BG)
    draw = ImageDraw.Draw(img)

    # Background track (0,0, 248x10)
    draw.rectangle([0, 3, 247, 6], fill=SLIDER_TRACK)
    draw.rectangle([0, 4, 247, 5], fill=BORDER)

    # Thumb normal (248,0, 29x10)
    draw.rectangle([248, 0, 276, 9], fill=THUMB, outline=BORDER)
    draw.line([(256, 2), (256, 7)], fill=WHITE)

    # Thumb pressed (278,0, 29x10)
    draw.rectangle([278, 0, 306, 9], fill=THUMB_PRESS, outline=ACCENT)
    draw.line([(286, 2), (286, 7)], fill=WHITE)

    save(img, "posbar.bmp")


# ============================================================
# volume.bmp  (68 x 433)
# 28 frames of 68x15 (=420px), then thumbs at y=422
# ============================================================
def gen_volume():
    img = Image.new("RGB", (68, 433), BG)
    draw = ImageDraw.Draw(img)

    for i in range(28):
        y = i * 15
        # Track groove
        draw.rectangle([4, y + 6, 49, y + 8], fill=SLIDER_TRACK)
        # Fill bar up to current level
        fill_w = int((i / 27) * 45)
        if fill_w > 0:
            draw.rectangle([4, y + 6, 4 + fill_w, y + 8], fill=ACCENT_DIM)

    # Thumb normal (15, 422, 14x11)
    draw.rectangle([15, 422, 28, 432], fill=THUMB, outline=BORDER)
    # Thumb pressed (0, 422, 14x11)
    draw.rectangle([0, 422, 13, 432], fill=THUMB_PRESS, outline=ACCENT)

    save(img, "volume.bmp")


# ============================================================
# balance.bmp  (47 x 433) — same layout idea as volume
# ============================================================
def gen_balance():
    img = Image.new("RGB", (47, 433), BG)
    draw = ImageDraw.Draw(img)

    for i in range(28):
        y = i * 15
        draw.rectangle([12, y + 6, 35, y + 8], fill=SLIDER_TRACK)
        # Centre marker
        draw.line([(23, y + 5), (23, y + 9)], fill=BORDER)

    # Thumb normal (15, 422, 14x11)
    draw.rectangle([15, 422, 28, 432], fill=THUMB, outline=BORDER)
    # Thumb pressed (0, 422, 14x11)
    draw.rectangle([0, 422, 13, 432], fill=THUMB_PRESS, outline=ACCENT)

    save(img, "balance.bmp")


# ============================================================
# shufrep.bmp  (92 x 85)
# ============================================================
def gen_shufrep():
    img = Image.new("RGB", (92, 85), BG)
    draw = ImageDraw.Draw(img)

    # Repeat button (0,0, 28x15) x4 rows
    for row, (fill, outline) in enumerate([
        (BUTTON, BORDER),           # normal
        (BUTTON_PRESS, BORDER),     # hover
        (ACCENT_DIM, ACCENT),       # active
        (ACCENT, ACCENT),           # active hover
    ]):
        y = row * 15
        draw.rectangle([0, y, 27, y + 14], fill=fill, outline=outline)
        # "R" label
        lc = BLACK if row >= 2 else TEXT_DIM
        draw_tiny_text(draw, 9, y + 4, "r", lc)

    # Shuffle button (28,0, 47x15) x4 rows
    for row, (fill, outline) in enumerate([
        (BUTTON, BORDER),
        (BUTTON_PRESS, BORDER),
        (ACCENT_DIM, ACCENT),
        (ACCENT, ACCENT),
    ]):
        y = row * 15
        draw.rectangle([28, y, 74, y + 14], fill=fill, outline=outline)
        lc = BLACK if row >= 2 else TEXT_DIM
        draw_tiny_text(draw, 40, y + 4, "s", lc)

    # EQ button (0,61, 23x12) and pressed (46,61)
    # active (0,73) and active pressed (46,73)
    for i, (x_off, fill, outline) in enumerate([
        (0, BUTTON, BORDER),
        (46, BUTTON_PRESS, BORDER),
        (0, ACCENT_DIM, ACCENT),
        (46, ACCENT, ACCENT),
    ]):
        y = 61 if i < 2 else 73
        draw.rectangle([x_off, y, x_off + 22, y + 11], fill=fill, outline=outline)
        lc = BLACK if i >= 2 else TEXT_DIM
        draw_tiny_text(draw, x_off + 7, y + 3, "eq", lc)

    # Playlist button (23,61, 23x12) and pressed (69,61)
    # active (23,73) and active pressed (69,73)
    for i, (x_off, fill, outline) in enumerate([
        (23, BUTTON, BORDER),
        (69, BUTTON_PRESS, BORDER),
        (23, ACCENT_DIM, ACCENT),
        (69, ACCENT, ACCENT),
    ]):
        y = 61 if i < 2 else 73
        draw.rectangle([x_off, y, x_off + 22, y + 11], fill=fill, outline=outline)
        lc = BLACK if i >= 2 else TEXT_DIM
        draw_tiny_text(draw, x_off + 6, y + 3, "pl", lc)

    save(img, "shufrep.bmp")


# ============================================================
# monoster.bmp  (56 x 24)
# ============================================================
def gen_monoster():
    img = Image.new("RGB", (56, 24), BG)
    draw = ImageDraw.Draw(img)

    # Stereo selected (0,0, 29x12)
    draw_tiny_text(draw, 3, 3, "stereo", ACCENT)
    # Stereo normal (0,12, 29x12)
    draw_tiny_text(draw, 3, 15, "stereo", TEXT_DIM)

    # Mono selected (29,0, 27x12)
    draw_tiny_text(draw, 32, 3, "mono", ACCENT)
    # Mono normal (29,12, 27x12)
    draw_tiny_text(draw, 32, 15, "mono", TEXT_DIM)

    save(img, "monoster.bmp")


# ============================================================
# text.bmp  (155 x 18) — 5x6 character grid
# ============================================================
def gen_text():
    img = Image.new("RGB", (155, 18), BLACK)
    draw = ImageDraw.Draw(img)

    # We need to render tiny 5x6 characters. Use a pixel font approach.
    # Each character is defined as a set of pixel coordinates within a 5x6 grid.
    font_data = get_pixel_font()

    # Row 0 (y=0): a-z, then ", @, space
    row0 = "abcdefghijklmnopqrstuvwxyz"
    for i, ch in enumerate(row0):
        draw_pixel_char(draw, i * 5, 0, ch, font_data, TEXT_CLR)
    # " at x=130
    draw_pixel_char(draw, 130, 0, '"', font_data, TEXT_CLR)
    # @ at x=135
    draw_pixel_char(draw, 135, 0, '@', font_data, TEXT_CLR)
    # Space at x=150 — leave blank

    # Row 1 (y=6): 0-9, then symbols
    row1_chars = '0123456789...:()-\'!_+\\/[]^&%,=$#'
    # Actual positions from charmap.ts:
    # 0-9 at x=0..45, then symbols at specific x offsets
    for i in range(10):
        draw_pixel_char(draw, i * 5, 6, str(i), font_data, TEXT_CLR)
    symbols_row1 = [
        ('.', 50), ('.', 55), (':', 60), ('(', 65), (')', 70),
        ('-', 75), ("'", 80), ('!', 85), ('_', 90), ('+', 95),
        ('\\', 100), ('/', 105), ('[', 110), (']', 115), ('^', 120),
        ('&', 125), ('%', 130), (',', 135), ('=', 140), ('$', 145),
        ('#', 150),
    ]
    for ch, x in symbols_row1:
        draw_pixel_char(draw, x, 6, ch, font_data, TEXT_CLR)

    # Row 2 (y=12): international chars
    draw_pixel_char(draw, 0, 12, 'a', font_data, TEXT_CLR)  # Å placeholder
    draw_pixel_char(draw, 5, 12, 'o', font_data, TEXT_CLR)  # Ö placeholder
    draw_pixel_char(draw, 10, 12, 'a', font_data, TEXT_CLR) # Ä placeholder
    draw_pixel_char(draw, 15, 12, '?', font_data, TEXT_CLR)
    draw_pixel_char(draw, 20, 12, '*', font_data, TEXT_CLR)

    save(img, "text.bmp")


# ============================================================
# viscolor.txt — visualiser colour palette
# ============================================================
def gen_viscolor():
    colors = [
        "0,0,0",        # 0  background
        "0,200,120",    # 1  peak (bright green)
        "0,180,108",
        "0,160,96",
        "0,140,84",
        "0,120,72",     # 5
        "0,100,60",
        "0,80,48",
        "0,60,36",
        "0,40,24",
        "0,30,18",      # 10
        "0,20,12",
        "0,16,10",
        "0,12,8",
        "0,10,6",
        "0,8,4",        # 15
        "0,6,4",
        "0,200,120",    # 17 peak dot colour
        "0,0,0",        # 18 grid
        "0,40,24",      # 19 grid foreground
        "0,200,120",    # 20 oscilloscope
        "0,0,0",        # 21 osc bg
        "0,120,72",     # 22 osc line 3
        "0,80,48",      # 23 osc line 5
    ]
    path = os.path.join(OUT, "viscolor.txt")
    with open(path, "w") as f:
        for line in colors:
            f.write(line + "\n")
    print(f"  viscolor.txt")


# ============================================================
# pledit.txt — playlist colours
# ============================================================
def gen_pledit():
    path = os.path.join(OUT, "pledit.txt")
    with open(path, "w") as f:
        f.write("[Text]\n")
        f.write("Normal=#00C878\n")
        f.write("Current=#00E68C\n")
        f.write("NormalBG=#222228\n")
        f.write("SelectedBG=#303038\n")
        f.write("Font=Arial\n")
    print(f"  pledit.txt")


# ============================================================
# Tiny pixel font helpers
# ============================================================

def draw_tiny_text(draw, x, y, text, color):
    """Draw text using ImageDraw at a small size (best-effort)."""
    for i, ch in enumerate(text):
        # Simple 1px marks for very tiny labels
        cx = x + i * 4
        if ch != ' ':
            draw.rectangle([cx, y, cx + 2, y + 3], fill=color)


def get_pixel_font():
    """Return a dict mapping chars to lists of (x,y) pixel positions in a 5x6 grid."""
    f = {}
    # Letters - simple 3x5 pixel patterns in a 5x6 cell (offset 1,0)
    f['a'] = [(1,1),(2,1),(3,1),(0,2),(4,2),(0,3),(1,3),(2,3),(3,3),(4,3),(0,4),(4,4),(0,5),(4,5)]
    f['b'] = [(0,0),(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(1,2),(2,2),(3,2),(0,3),(4,3),(0,4),(4,4),(0,5),(1,5),(2,5),(3,5)]
    f['c'] = [(1,1),(2,1),(3,1),(0,2),(0,3),(0,4),(1,5),(2,5),(3,5)]
    f['d'] = [(0,0),(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(0,3),(4,3),(0,4),(4,4),(0,5),(1,5),(2,5),(3,5)]
    f['e'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(0,1),(0,2),(1,2),(2,2),(3,2),(0,3),(0,4),(0,5),(1,5),(2,5),(3,5),(4,5)]
    f['f'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(0,1),(0,2),(1,2),(2,2),(3,2),(0,3),(0,4),(0,5)]
    f['g'] = [(1,0),(2,0),(3,0),(0,1),(0,2),(3,2),(4,2),(0,3),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['h'] = [(0,0),(4,0),(0,1),(4,1),(0,2),(1,2),(2,2),(3,2),(4,2),(0,3),(4,3),(0,4),(4,4),(0,5),(4,5)]
    f['i'] = [(1,0),(2,0),(3,0),(2,1),(2,2),(2,3),(2,4),(1,5),(2,5),(3,5)]
    f['j'] = [(2,0),(3,0),(4,0),(3,1),(3,2),(3,3),(0,4),(3,4),(1,5),(2,5)]
    f['k'] = [(0,0),(4,0),(0,1),(3,1),(0,2),(1,2),(2,2),(0,3),(3,3),(0,4),(4,4),(0,5),(4,5)]
    f['l'] = [(0,0),(0,1),(0,2),(0,3),(0,4),(0,5),(1,5),(2,5),(3,5),(4,5)]
    f['m'] = [(0,0),(4,0),(0,1),(1,1),(3,1),(4,1),(0,2),(2,2),(4,2),(0,3),(4,3),(0,4),(4,4),(0,5),(4,5)]
    f['n'] = [(0,0),(4,0),(0,1),(1,1),(4,1),(0,2),(2,2),(4,2),(0,3),(3,3),(4,3),(0,4),(4,4),(0,5),(4,5)]
    f['o'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(0,3),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['p'] = [(0,0),(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(0,3),(1,3),(2,3),(3,3),(0,4),(0,5)]
    f['q'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(0,3),(4,3),(0,4),(3,4),(1,5),(2,5),(3,5),(4,5)]
    f['r'] = [(0,0),(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(0,3),(1,3),(2,3),(3,3),(0,4),(3,4),(0,5),(4,5)]
    f['s'] = [(1,0),(2,0),(3,0),(4,0),(0,1),(1,2),(2,2),(3,2),(4,3),(0,4),(1,4),(2,4),(3,4)]
    f['t'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(2,1),(2,2),(2,3),(2,4),(2,5)]
    f['u'] = [(0,0),(4,0),(0,1),(4,1),(0,2),(4,2),(0,3),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['v'] = [(0,0),(4,0),(0,1),(4,1),(0,2),(4,2),(1,3),(3,3),(1,4),(3,4),(2,5)]
    f['w'] = [(0,0),(4,0),(0,1),(4,1),(0,2),(4,2),(0,3),(2,3),(4,3),(0,4),(1,4),(3,4),(4,4),(0,5),(4,5)]
    f['x'] = [(0,0),(4,0),(1,1),(3,1),(2,2),(1,3),(3,3),(0,4),(4,4),(0,5),(4,5)]
    f['y'] = [(0,0),(4,0),(1,1),(3,1),(2,2),(2,3),(2,4),(2,5)]
    f['z'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(4,1),(3,2),(2,3),(1,4),(0,5),(1,5),(2,5),(3,5),(4,5)]

    # Digits
    f['0'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(3,2),(4,2),(0,3),(2,3),(4,3),(0,4),(1,4),(4,4),(1,5),(2,5),(3,5)]
    f['1'] = [(2,0),(1,1),(2,1),(2,2),(2,3),(2,4),(1,5),(2,5),(3,5)]
    f['2'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(3,2),(2,3),(1,4),(0,5),(1,5),(2,5),(3,5),(4,5)]
    f['3'] = [(0,0),(1,0),(2,0),(3,0),(4,1),(2,2),(3,2),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['4'] = [(0,0),(4,0),(0,1),(4,1),(0,2),(4,2),(1,3),(2,3),(3,3),(4,3),(4,4),(4,5)]
    f['5'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(0,1),(0,2),(1,2),(2,2),(3,2),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['6'] = [(1,0),(2,0),(3,0),(0,1),(0,2),(1,2),(2,2),(3,2),(0,3),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['7'] = [(0,0),(1,0),(2,0),(3,0),(4,0),(4,1),(3,2),(2,3),(2,4),(2,5)]
    f['8'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(1,2),(2,2),(3,2),(0,3),(4,3),(0,4),(4,4),(1,5),(2,5),(3,5)]
    f['9'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(4,2),(1,3),(2,3),(3,3),(4,3),(4,4),(1,5),(2,5),(3,5)]

    # Symbols
    f['.'] = [(2,5)]
    f[':'] = [(2,1),(2,4)]
    f['('] = [(3,0),(2,1),(2,2),(2,3),(2,4),(3,5)]
    f[')'] = [(1,0),(2,1),(2,2),(2,3),(2,4),(1,5)]
    f['-'] = [(1,3),(2,3),(3,3)]
    f["'"] = [(2,0),(2,1)]
    f['!'] = [(2,0),(2,1),(2,2),(2,3),(2,5)]
    f['_'] = [(0,5),(1,5),(2,5),(3,5),(4,5)]
    f['+'] = [(2,1),(1,2),(2,2),(3,2),(2,3)]
    f['\\'] = [(0,0),(1,1),(2,2),(2,3),(3,4),(4,5)]
    f['/'] = [(4,0),(3,1),(2,2),(2,3),(1,4),(0,5)]
    f['['] = [(2,0),(3,0),(2,1),(2,2),(2,3),(2,4),(2,5),(3,5)]
    f[']'] = [(1,0),(2,0),(2,1),(2,2),(2,3),(2,4),(1,5),(2,5)]
    f['^'] = [(2,0),(1,1),(3,1)]
    f['&'] = [(1,0),(2,0),(0,1),(3,1),(1,2),(2,2),(0,3),(2,3),(4,3),(0,4),(3,4),(1,5),(2,5),(4,5)]
    f['%'] = [(0,0),(1,0),(4,0),(0,1),(1,1),(3,1),(2,2),(1,3),(3,3),(4,3),(0,4),(3,4),(4,4)]
    f[','] = [(2,4),(1,5)]
    f['='] = [(1,2),(2,2),(3,2),(1,4),(2,4),(3,4)]
    f['$'] = [(1,0),(2,0),(3,0),(4,0),(0,1),(2,1),(1,2),(2,2),(3,2),(2,3),(4,3),(0,4),(1,4),(2,4),(3,4)]
    f['#'] = [(1,0),(3,0),(0,1),(1,1),(2,1),(3,1),(4,1),(1,2),(3,2),(0,3),(1,3),(2,3),(3,3),(4,3),(1,4),(3,4)]
    f['"'] = [(1,0),(3,0),(1,1),(3,1)]
    f['@'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(0,2),(2,2),(3,2),(4,2),(0,3),(2,3),(4,3),(0,4),(2,4),(3,4),(1,5),(2,5),(3,5),(4,5)]
    f['?'] = [(1,0),(2,0),(3,0),(0,1),(4,1),(3,2),(2,3),(2,5)]
    f['*'] = [(0,0),(4,0),(1,1),(3,1),(2,2),(1,3),(3,3),(0,4),(4,4)]
    f[' '] = []

    return f


def draw_pixel_char(draw, ox, oy, char, font_data, color):
    """Draw a single character from the pixel font at (ox, oy)."""
    pixels = font_data.get(char, font_data.get(' ', []))
    for px, py in pixels:
        draw.point((ox + px, oy + py), fill=color)


# ============================================================
# Generate all
# ============================================================
if __name__ == "__main__":
    print(f"Generating default skin in {OUT}/")
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
    gen_viscolor()
    gen_pledit()
    print("Done.")
