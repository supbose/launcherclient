#!/usr/bin/env python3
"""PNG 生成图标：按尺寸输出文件，文件名后缀带尺寸，按系统惯例分目录。

用法：
    python png2ico.py                                  # 转换 DEFAULT_PNGS，生成全部系统图标
    python png2ico.py xxx.png yyy.png                  # 命令行指定 PNG（可多个，优先于 DEFAULT_PNGS）
    python png2ico.py D:\assets                        # 输入可为 PNG 文件或包含 PNG 的目录
    python png2ico.py xxx.png --systems windows linux  # 选择目标系统
    python png2ico.py xxx.png --sizes 16 32x32 256     # 自定义尺寸（应用到每个选中系统）
    python png2ico.py xxx.png --out-dir out            # 指定输出根目录
    python png2ico.py xxx.png --bg white               # 填充透明区域，生成不透明图标
    python png2ico.py xxx.png --radius 20              # 裁圆角（半径占尺寸 20%）

输出目录结构：<根目录>/<YYYYMMDD_HHMMSS>/<文件名>/<系统>/<尺寸>.ico，一次运行共用同一时间戳。
ICO 格式上限为 256x256，超过 256 的尺寸自动输出为 .png。
"""

import argparse
import sys
from datetime import datetime
from pathlib import Path

from PIL import Image, ImageColor, ImageDraw

# 各系统图标惯例尺寸；超过 ICO_MAX_SIZE 的尺寸输出为 PNG
SYSTEM_SIZES = {
    "windows": [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    "linux": [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (96, 96), (128, 128), (256, 256), (512, 512)],
    "macos": [(16, 16), (32, 32), (128, 128), (256, 256), (512, 512), (1024, 1024)],
}
ICO_MAX_SIZE = 256

# 在此指定要转换的 PNG 文件或目录（可多个，相对路径基于脚本所在目录；目录取其下所有 *.png），命令行传入参数时优先使用命令行
DEFAULT_PNGS = [
    "rocket_launch.png",
    "update_tool_icon_transparent.png",
]

# 背景色：None 保持透明；指定颜色（如 "white"、"#F0F0F0"）则填充透明区域，生成不透明图标
DEFAULT_BG = None

# 圆角半径（占图标尺寸的百分比，0-50）；None 或 0 不裁圆角
DEFAULT_RADIUS = 10


def apply_rounded(img: Image.Image, radius_pct: float) -> Image.Image:
    """按尺寸百分比裁圆角，4 倍超采样掩膜保证小尺寸边缘平滑。"""
    w, h = img.size
    r = min(w, h) * radius_pct / 100
    if r < 1:
        return img
    s = 4
    mask = Image.new("L", (w * s, h * s), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, w * s - 1, h * s - 1], radius=int(r * s), fill=255)
    mask = mask.resize((w, h), Image.LANCZOS)
    img = img.convert("RGBA")
    img.putalpha(mask)
    return img


def parse_size(text: str) -> tuple[int, int]:
    """解析尺寸参数，支持 "32" 或 "32x32"（不区分大小写）。"""
    parts = text.lower().replace("*", "x").split("x")
    if len(parts) == 1:
        return int(parts[0]), int(parts[0])
    return int(parts[0]), int(parts[1])


def collect_pngs(paths: list[Path]) -> tuple[list[Path], list[Path]]:
    """展开输入路径：文件直接收录，目录收录其下所有 PNG（不递归）。返回 (PNG 列表, 无效路径列表)。"""
    pngs, invalid = [], []
    for p in paths:
        if p.is_dir():
            pngs.extend(sorted(p.glob("*.png")))
        elif p.is_file():
            pngs.append(p)
        else:
            invalid.append(p)
    return pngs, invalid


def png_to_icons(png_path: Path, sizes: list[tuple[int, int]], out_dir: Path, bg: str | None = None, radius: float | None = None) -> list[Path]:
    img = Image.open(png_path)
    if img.mode != "RGBA":
        img = img.convert("RGBA")
    if bg:
        base = Image.new("RGB", img.size, ImageColor.getrgb(bg))
        base.paste(img, (0, 0), img)  # 以 alpha 为掩膜，透明区域露出背景色
        img = base

    outputs = []
    for w, h in sizes:
        if w > img.width or h > img.height:
            print(f"警告：{png_path.name} 尺寸 {img.size} 小于 {w}x{h}，将放大", file=sys.stderr)
        resized = img.resize((w, h), Image.LANCZOS)
        if radius:
            resized = apply_rounded(resized, radius)
        if max(w, h) <= ICO_MAX_SIZE:
            out_path = out_dir / f"{w}x{h}.ico"
            resized.save(out_path, format="ICO", sizes=((w, h),))
        else:  # 超出 ICO 格式 256 上限，输出 PNG
            out_path = out_dir / f"{w}x{h}.png"
            resized.save(out_path, format="PNG")
        outputs.append(out_path)
    return outputs


def main():
    parser = argparse.ArgumentParser(description="PNG 生成图标，按系统尺寸分目录，文件名后缀带尺寸")
    parser.add_argument("inputs", nargs="*", help="PNG 文件或目录（目录取其下所有 *.png，可多个），缺省时使用脚本内 DEFAULT_PNGS")
    parser.add_argument("--systems", nargs="+", choices=list(SYSTEM_SIZES), default=None, help="目标系统，缺省全部（windows/linux/macos）")
    parser.add_argument("--sizes", nargs="+", metavar="N", help="自定义尺寸（如 16 32x32 256），应用到每个选中系统；缺省按系统惯例")
    parser.add_argument("--out-dir", default=None, help="输出根目录，默认与输入 PNG 同目录，内部按时间戳和系统建子目录")
    parser.add_argument("--bg", default=None, help="背景色，如 white 或 #F0F0F0，缺省使用脚本内 DEFAULT_BG（None 保持透明）")
    parser.add_argument("--radius", type=float, default=None, help="圆角半径（占尺寸百分比，0-50），缺省使用脚本内 DEFAULT_RADIUS（None 不裁圆角）")
    args = parser.parse_args()

    custom_sizes = [parse_size(s) for s in args.sizes] if args.sizes else None
    systems = args.systems if args.systems else list(SYSTEM_SIZES)
    bg = args.bg if args.bg is not None else DEFAULT_BG
    radius = args.radius if args.radius is not None else DEFAULT_RADIUS
    if radius is not None and not 0 < radius <= 50:
        print(f"无效圆角半径：{radius}，需在 0-50（占尺寸百分比）之间", file=sys.stderr)
        sys.exit(1)
    if bg:
        try:
            ImageColor.getrgb(bg)  # 提前校验颜色格式
        except ValueError:
            print(f"无效背景色：{bg}，支持如 white、#F0F0F0", file=sys.stderr)
            sys.exit(1)

    if args.inputs:
        inputs = [Path(p) for p in args.inputs]
    else:
        inputs = [Path(__file__).parent / p for p in DEFAULT_PNGS]

    pngs, invalid = collect_pngs(inputs)
    if invalid:
        print(f"未找到路径：{', '.join(str(p) for p in invalid)}", file=sys.stderr)
        sys.exit(1)
    if not pngs:
        print("输入中未找到 PNG 文件", file=sys.stderr)
        sys.exit(1)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    for png in pngs:
        base_dir = Path(args.out_dir) if args.out_dir else png.parent
        for system in systems:
            sizes = custom_sizes or SYSTEM_SIZES[system]
            out_dir = base_dir / timestamp / png.stem / system
            out_dir.mkdir(parents=True, exist_ok=True)
            for out_path in png_to_icons(png, sizes, out_dir, bg, radius):
                print(f"{png.name} [{system}] -> {out_path}")


if __name__ == "__main__":
    main()
