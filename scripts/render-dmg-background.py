#!/usr/bin/env python3
"""Render Luma's DMG artwork. Uses Pillow and macOS system fonts; no CI step.

python3 scripts/render-dmg-background.py
The committed 2x PNG embeds 144 dpi so Finder uses a 720 x 480 point image.
"""
from pathlib import Path
import argparse
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, default=ROOT / 'src-tauri/dmg/background.png')
args = parser.parse_args()
SCALE = 2
WIDTH, HEIGHT = 720, 480
FONT_ROOT = Path('/System/Library/Fonts')
pingfang = next(Path('/System/Library/AssetsV2').glob('com_apple_MobileAsset_Font*/*/AssetData/PingFang.ttc'), None)
if pingfang is None:
    pingfang = FONT_ROOT / 'PingFang.ttc'
if not pingfang.exists():
    raise SystemExit('Regeneration requires macOS with the PingFang system font installed.')

def font(size, chinese=False, weight=False):
    if chinese:
        return ImageFont.truetype(str(pingfang), round(size*SCALE), index=11 if weight else 3)
    result = ImageFont.truetype(str(FONT_ROOT / 'SFNS.ttf'), round(size*SCALE))
    if weight:
        result.set_variation_by_name('Semibold')
    return result

image = Image.new('RGB', (WIDTH*SCALE, HEIGHT*SCALE), '#F5F5F0')
# A quiet celadon wash supports the existing dark green Luma mark.
pixels = image.load()
for y in range(HEIGHT*SCALE):
    for x in range(WIDTH*SCALE):
        dx, dy = x/SCALE-620, y/SCALE-40
        glow = max(0.0, 1.0-(dx*dx/(490*490)+dy*dy/(400*400)))**2
        base, tint = (245,245,240), (220,233,223)
        pixels[x,y] = tuple(round(a+(b-a)*glow*.65) for a,b in zip(base,tint))
draw = ImageDraw.Draw(image)
def line(points, color, width=1):
    draw.line([(round(x*SCALE),round(y*SCALE)) for x,y in points], fill=color, width=round(width*SCALE))
def text(x,y,value,size=14,color='#53645C',chinese=False,weight=False):
    draw.text((round(x*SCALE),round(y*SCALE)),value,font=font(size,chinese,weight),fill=color,anchor='lt')

def centered_text(cx,y,value,size=14,color='#53645C',chinese=False):
    width = draw.textlength(value,font=font(size,chinese))/SCALE
    text(cx-width/2,y,value,size,color,chinese)

logo = Image.open(ROOT / 'src-tauri/icons/128x128@2x.png').convert('RGBA')
logo = logo.resize((32*SCALE,32*SCALE),Image.Resampling.LANCZOS)
image.paste(logo,(44*SCALE,32*SCALE),logo)
text(87,36,'Luma',23,'#1D2C25',weight=True)
platform_width = draw.textlength('macOS', font=font(11))/SCALE
text(674-platform_width,43,'macOS',11,'#6A7970')
# The headline is intentionally separate from Finder's real app/folder labels.
text(46,100,'让文件各归其位。',28,'#1D2C25',chinese=True,weight=True)
text(46,145,'将 Luma 拖入右侧「应用程序」，即可完成安装。',14,'#53645C',chinese=True)
text(46,169,'Drag Luma to Applications to install.',12,'#6A7970')

for cx in (190,530):
    box=(int((cx-100)*SCALE),202*SCALE,int((cx+100)*SCALE),385*SCALE)
    draw.rounded_rectangle(box,radius=20*SCALE,fill='#FFFFFF',outline='#E1E6DE',width=SCALE)
# Real Finder icons sit at (190, 280) and (530, 280), at 128 points.
# The arrow is artwork only; the application and Applications alias stay native.
line([(318,280),(402,280)],'#71927F',2)
line([(391,269),(402,280),(391,291)],'#71927F',2)
centered_text(360,308,'拖入',11,'#6A7970',chinese=True)
line([(46,408),(674,408)],'#DDE3D9')
text(46,427,'安装后，从「应用程序」打开 Luma。',12,'#53645C',chinese=True)
text(46,448,'Then open Luma from Applications.',11,'#78847B')
args.output.parent.mkdir(parents=True,exist_ok=True)
image.save(args.output, dpi=(144,144), optimize=True)
print(args.output)
