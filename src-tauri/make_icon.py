#!/usr/bin/env python3
"""wiki-ya 应用图标：Y 形知识汇聚节点 + 靛紫渐变圆角方。

设计说明：
- 主体是字母 Y 的三叉连接图（wiki-ya 的 Y）：三条边从外围三个节点
  汇聚到中心大节点——「知识汇聚到一处」的品牌意象，也是知识图谱的
  极简表达。
- 背景：靛→紫对角渐变 + 中心柔光，macOS 风格圆角方。
- 4x 超采样绘制再降采样，保证小尺寸（32px）下边缘依然锐利。
"""
import math

from PIL import Image, ImageDraw, ImageFilter

S = 1024          # 输出尺寸
SS = 4            # 超采样倍数
W = S * SS        # 画布尺寸（超采样空间）
R = int(W * 0.225)  # 圆角半径（macOS 图标观感）

# ---- 配色 ----
C_TOP = (99, 102, 241)      # indigo-500
C_BOT = (139, 92, 246)      # violet-500
C_DEEP = (49, 46, 129)      # indigo-900（渐变底角）
C_LINE = (255, 255, 255)
C_NODE = (255, 255, 255)
C_ACCENT = (165, 243, 252)  # cyan-200 中心高光

# ---- 背景：对角渐变（indigo-900 → indigo → violet）----
bg = Image.new("RGB", (W, W))
px = bg.load()
for y in range(W):
    t = y / (W - 1)
    for x in range(W):
        dx = x / (W - 1) - 0.5
        k = 1 + dx * 0.25
        r = int(C_DEEP[0] * (1 - t) ** 1.6 + C_TOP[0] * t ** 0.8 + C_BOT[0] * t)
        g = int(C_DEEP[1] * (1 - t) ** 1.6 + C_TOP[1] * t ** 0.8 + C_BOT[1] * t)
        b = int(C_DEEP[2] * (1 - t) ** 1.6 + C_TOP[2] * t ** 0.8 + C_BOT[2] * t)
        px[x, y] = (
            max(0, min(255, int(r * k))),
            max(0, min(255, int(g * k))),
            max(0, min(255, int(b * k))),
        )

# 中心柔光晕
glow = Image.new("L", (W, W), 0)
gd = ImageDraw.Draw(glow)
gd.ellipse([W * 0.18, W * 0.14, W * 0.92, W * 0.86], fill=90)
glow = glow.filter(ImageFilter.GaussianBlur(W * 0.12))
bg = Image.composite(Image.new("RGB", (W, W), (199, 210, 254)), bg, glow)

img = bg.convert("RGBA")

# ---- 圆角方形裁剪（macOS 风格）----
mask = Image.new("L", (W, W), 0)
md = ImageDraw.Draw(mask)
md.rounded_rectangle([0, 0, W, W], radius=R, fill=255)

canvas = Image.new("RGBA", (W, W), (0, 0, 0, 0))
canvas.paste(img, (0, 0), mask)
d = ImageDraw.Draw(canvas)

# ---- Y 形知识节点 ----
CX, CY = W * 0.50, W * 0.585          # 中心节点（略偏下，视觉重心）
LW = int(W * 0.052)                    # 线宽
NODE_R = int(W * 0.105)                # 中心节点半径
LEAF_R = int(W * 0.062)                # 外围节点半径
ARM = W * 0.255                        # 臂长

# 三个外围节点：左上 215°、右上 325°、正下 90°（构成 Y）
angles = [215, 325, 90]
leaves = []
for a in angles:
    rad = math.radians(a)
    lx = CX + math.cos(rad) * ARM
    ly = CY - math.sin(rad) * ARM
    leaves.append((lx, ly))

# 连线（圆头）：先画粗线，端点用圆补齐
for (lx, ly) in leaves:
    d.line([(CX, CY), (lx, ly)], fill=C_LINE, width=LW)
    d.ellipse([CX - LW / 2, CY - LW / 2, CX + LW / 2, CY + LW / 2], fill=C_LINE)
    d.ellipse([lx - LW / 2, ly - LW / 2, lx + LW / 2, ly + LW / 2], fill=C_LINE)

# 中心节点：白圆 + 靛色内芯 + 青色高光点（一颗汇聚的知识星）
d.ellipse([CX - NODE_R, CY - NODE_R, CX + NODE_R, CY + NODE_R], fill=C_NODE)
core = NODE_R * 0.52
d.ellipse([CX - core, CY - core, CX + core, CY + core], fill=(79, 70, 229))
hl = core * 0.30
d.ellipse(
    [CX - core * 0.15 - hl, CY - core * 0.45 - hl,
     CX - core * 0.15 + hl, CY - core * 0.45 + hl],
    fill=C_ACCENT,
)

# 外围节点内芯：小靛点
for (lx, ly) in leaves:
    lr = LEAF_R * 0.34
    d.ellipse([lx - lr, ly - lr, lx + lr, ly + lr], fill=(99, 102, 241))

# ---- 降采样输出 ----
final = canvas.resize((S, S), Image.LANCZOS)
final.save("icons/app-icon.png")
print("app-icon.png generated (1024x1024)")
