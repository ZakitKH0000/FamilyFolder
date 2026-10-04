"""Картинки для страницы на GitHub из кадров и снимков scripts/showcase.js (нужен Pillow).

  python scripts/showcase.py anim <папка кадров> <файл.webp> [--width 1080] [--fps 25] [--start 0] [--end 0] [--quality 80]
  python scripts/showcase.py gif <папка кадров> <файл.gif> [--width 640] [--fps 12] [--start 0] [--end 0]
  python scripts/showcase.py window <снимок.png> <файл.png> [--radius 16]   скруглить углы, добавить тень
  python scripts/showcase.py row <файл.png> <окно1.png> <окно2.png> ...       окна (уже с тенью) в ряд
"""
import json
import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter


def args(rest, **defaults):
    opt = dict(defaults)
    for i in range(0, len(rest), 2):
        key = rest[i].lstrip('-')
        opt[key] = type(defaults[key])(rest[i + 1])
    return opt


def anim(src, out, rest):
    """Кадры идут неравномерно — берём для каждого момента последний готовый кадр."""
    opt = args(rest, width=1080, fps=25.0, start=0.0, end=0.0, quality=80)
    frames = json.load(open(os.path.join(src, 'frames.json'), encoding='utf-8'))
    end = opt['end'] or frames[-1]['t']
    images, j, n = [], 0, int((end - opt['start']) * opt['fps'])
    for i in range(n):
        t = opt['start'] + i / opt['fps']
        while j + 1 < len(frames) and frames[j + 1]['t'] <= t:
            j += 1
        img = Image.open(os.path.join(src, frames[j]['name'])).convert('RGB')
        h = round(img.height * opt['width'] / img.width)
        images.append(img.resize((opt['width'], h), Image.LANCZOS))
    images[0].save(out, save_all=True, append_images=images[1:], duration=round(1000 / opt['fps']),
                   loop=0, quality=opt['quality'], method=4)
    print(f'{out}: {len(images)} кадров, {os.path.getsize(out) // 1024} КБ')


def gif(src, out, rest):
    """То же для площадок без WebP (Пикабу и т. п.): GIF с общей палитрой — без мерцания фона."""
    opt = args(rest, width=640, fps=12.0, start=0.0, end=0.0)
    frames = json.load(open(os.path.join(src, 'frames.json'), encoding='utf-8'))
    end = opt['end'] or frames[-1]['t']
    images, j, n = [], 0, int((end - opt['start']) * opt['fps'])
    for i in range(n):
        t = opt['start'] + i / opt['fps']
        while j + 1 < len(frames) and frames[j + 1]['t'] <= t:
            j += 1
        img = Image.open(os.path.join(src, frames[j]['name'])).convert('RGB')
        h = round(img.height * opt['width'] / img.width)
        images.append(img.resize((opt['width'], h), Image.LANCZOS))
    # Палитра по нескольким кадрам сразу, одна на всю анимацию.
    sample = images[:: max(1, len(images) // 8)]
    sheet = Image.new('RGB', (images[0].width, images[0].height * len(sample)))
    for k, im in enumerate(sample):
        sheet.paste(im, (0, k * im.height))
    pal = sheet.quantize(255, method=Image.Quantize.MEDIANCUT)
    out_frames = [im.quantize(palette=pal, dither=Image.Dither.NONE) for im in images]
    out_frames[0].save(out, save_all=True, append_images=out_frames[1:], duration=round(1000 / opt['fps']), loop=0)
    print(f'{out}: {len(out_frames)} кадров, {os.path.getsize(out) // 1024} КБ')


def window(src, out, rest):
    opt = args(rest, radius=16)
    img = Image.open(src).convert('RGBA')
    w, h = img.size
    mask = Image.new('L', (w, h), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), opt['radius'], fill=255)
    img.putalpha(ImageChops.multiply(img.getchannel('A'), mask))
    m = 70
    canvas = Image.new('RGBA', (w + 2 * m, h + 2 * m), (0, 0, 0, 0))
    shadow = Image.new('L', canvas.size, 0)
    shadow.paste(mask.point(lambda v: v * 0.5), (m, m + 18))
    shadow = shadow.filter(ImageFilter.GaussianBlur(26))
    canvas.putalpha(shadow)
    canvas.alpha_composite(img, (m, m))
    canvas.save(out, optimize=True)
    print(f'{out}: {canvas.size}, {os.path.getsize(out) // 1024} КБ')


def row(out, srcs):
    imgs = [Image.open(s).convert('RGBA') for s in srcs]
    gap = -70
    w = sum(i.width for i in imgs) + gap * (len(imgs) - 1)
    h = max(i.height for i in imgs)
    canvas = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    x = 0
    for i in imgs:
        canvas.alpha_composite(i, (x, (h - i.height) // 2))
        x += i.width + gap
    canvas.save(out, optimize=True)
    print(f'{out}: {canvas.size}, {os.path.getsize(out) // 1024} КБ')


if __name__ == '__main__':
    cmd, *rest = sys.argv[1:]
    if cmd == 'anim':
        anim(rest[0], rest[1], rest[2:])
    elif cmd == 'gif':
        gif(rest[0], rest[1], rest[2:])
    elif cmd == 'window':
        window(rest[0], rest[1], rest[2:])
    elif cmd == 'row':
        row(rest[0], rest[1:])
