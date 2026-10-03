"""Значки пакета Microsoft Store из значка программы (нужен Pillow).

  node scripts/showcase.js "file:///…/icon.html" icon1024.png 1024x1024 --still --dpr 1 --transparent
  python scripts/store-assets.py icon1024.png crates/app/store/Assets

icon.html — страница с одним <img> app-icon.svg (crates/app/icons-src) размером 1024×1024.
"""
import os
import sys

from PIL import Image

src, out = sys.argv[1], sys.argv[2]
icon = Image.open(src).convert('RGBA')
os.makedirs(out, exist_ok=True)


def save(name, w, h, share):
    """Значок по центру прозрачной картинки w×h, высотой share от неё."""
    canvas = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    side = round(min(w, h) * share)
    canvas.alpha_composite(icon.resize((side, side), Image.LANCZOS), ((w - side) // 2, (h - side) // 2))
    canvas.save(os.path.join(out, name), optimize=True)


for scale in (100, 125, 150, 200, 400):
    k = scale / 100
    save(f'Square44x44Logo.scale-{scale}.png', round(44 * k), round(44 * k), 1)
    save(f'Square150x150Logo.scale-{scale}.png', round(150 * k), round(150 * k), 0.62)
    save(f'Wide310x150Logo.scale-{scale}.png', round(310 * k), round(150 * k), 0.62)
    save(f'StoreLogo.scale-{scale}.png', round(50 * k), round(50 * k), 1)
# Панель задач и «Пуск»: точные размеры, без подложки.
for size in (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256):
    save(f'Square44x44Logo.targetsize-{size}.png', size, size, 1)
    save(f'Square44x44Logo.targetsize-{size}_altform-unplated.png', size, size, 1)
    save(f'Square44x44Logo.targetsize-{size}_altform-lightunplated.png', size, size, 1)
print(len(os.listdir(out)), 'files ->', out)
