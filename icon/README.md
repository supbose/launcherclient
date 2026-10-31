# png2ico.py — PNG 转多系统图标工具

按各系统图标惯例尺寸将 PNG 分别生成图标文件，按 `<时间戳>/<文件名>/<系统>/<尺寸>.ico` 分目录存放。

## 依赖

- Python 3.10+
- Pillow（`pip install pillow`）

## 使用

```powershell
python png2ico.py                                  # 转换脚本内 DEFAULT_PNGS，生成全部系统图标
python png2ico.py xxx.png yyy.png                  # 命令行指定 PNG（可多个，优先于 DEFAULT_PNGS）
python png2ico.py D:\assets                        # 输入可为 PNG 文件或包含 PNG 的目录（取其下所有 *.png）
python png2ico.py xxx.png --systems windows linux  # 选择目标系统（windows/linux/macos）
python png2ico.py xxx.png --sizes 16 32x32 256     # 自定义尺寸，应用到每个选中系统
python png2ico.py xxx.png --out-dir out            # 指定输出根目录（默认与输入 PNG 同目录）
python png2ico.py xxx.png --bg white               # 填充透明区域，生成不透明图标
python png2ico.py xxx.png --radius 20              # 裁圆角（半径占图标尺寸 20%）
```

## 各系统尺寸惯例

| 系统 | 尺寸 |
| --- | --- |
| `windows` | 16 / 24 / 32 / 48 / 64 / 128 / 256 |
| `linux` | 16 / 24 / 32 / 48 / 64 / 96 / 128 / 256 / 512 |
| `macos` | 16 / 32 / 128 / 256 / 512 / 1024 |

ICO 格式上限为 256x256，超过 256 的尺寸自动输出为 `.png`。

## 脚本内配置

脚本顶部常量可直接修改，命令行参数优先：

| 常量 | 默认值 | 说明 |
| --- | --- | --- |
| `SYSTEM_SIZES` | 见上表 | 各系统的尺寸列表 |
| `DEFAULT_PNGS` | 目录下两张 PNG | 缺省时要转换的 PNG 文件或目录（可多个，相对路径基于脚本所在目录；目录取其下所有 `*.png`） |
| `DEFAULT_BG` | `None` | 背景色；`None` 保持透明，可设为 `"white"`、`"#F0F0F0"` 等 |
| `DEFAULT_RADIUS` | `None` | 圆角半径（占尺寸百分比，0-50）；`None` 或 0 不裁圆角 |

## 输出结构

```
<输出根目录>/
└── 20261031_171530/          # 一次运行共用同一时间戳
    └── rocket_launch/        # 每个源 PNG 一个目录（文件名为源文件名）
        ├── windows/
        │   ├── 16x16.ico
        │   └── ...
        ├── linux/
        │   ├── 512x512.png   # 超过 256 的尺寸为 PNG
        │   └── ...
        └── macos/
            ├── 1024x1024.png
            └── ...
```

## 行为说明

- 输出目录为 `<输出根目录>/<YYYYMMDD_HHMMSS>/<文件名>/<系统>/`，一次运行共用同一时间戳，图标文件名为 `宽x高.ico`
- 缩放使用 LANCZOS，统一转为 RGBA 后输出
- 指定背景色时，透明区域按 alpha 合成到纯色背景，生成不透明（RGB）图标
- 圆角按尺寸百分比裁切（4 倍超采样掩膜，小尺寸边缘平滑），与背景色可叠加使用（先铺底再裁角）
- 源图小于请求尺寸时打印放大警告
- 路径不存在时报错退出（exit 1）；目录存在但无 PNG 时提示后退出；背景色非法时报错退出
