#!/usr/bin/env python3
"""全量打包脚本：依次执行启动器（egui）与打包工具（pkgtool）两个 pybuild 打包，
产物复制到 dist/<版本号>/ 目录。

用法：
    python scripts/build.py     # = pnpm dist

产物：
    dist/<版本号>/launcherclient.exe   # 字体已编进 exe，单文件即可运行
    dist/<版本号>/HarmonyOS-Sans-LICENSE.txt   # 字体许可（分发时须保留）
    dist/<版本号>/pkgtool.exe
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

import pybuild

DIST_DIR = pybuild.PROJECT_ROOT / "dist"
FONT_LICENSE = pybuild.PROJECT_ROOT / "src-egui" / "assets" / "font" / "LICENSE.txt"


def main() -> int:
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, OSError):
        pass

    try:
        version = pybuild.read_package_version()
        segments = pybuild.version_segments(version)
        csv_version, dotted_version = pybuild.parse_version(version)
        out_dir: Path = DIST_DIR / version
        print(f"版本号：{version} -> rc {csv_version} / 字符串 {dotted_version}")
        print(f"产物目录：{out_dir}")

        for name in ("egui", "pkgtool"):
            print(f"\n===== 打包 {name} =====")
            # 传入 version/segments：每次打包都把版本铺开到 Cargo 清单与两个 rc，
            # 让 dist 里的 exe 与 package.json 始终同源（不传则只同步目标 rc）
            exe_path = pybuild.build_target(
                pybuild.TARGETS[name], csv_version, dotted_version,
                version=version, segments=segments,
            )
            out_dir.mkdir(parents=True, exist_ok=True)
            dest = out_dir / exe_path.name
            shutil.copy2(exe_path, dest)
            print(f"      产物已复制到：{dest}")

            # 启动器内嵌了华为字体（include_bytes!），只需随包分发许可文本；
            # pkgtool 不需要
            if name == "egui":
                if FONT_LICENSE.is_file():
                    license_dest = out_dir / "HarmonyOS-Sans-LICENSE.txt"
                    shutil.copy2(FONT_LICENSE, license_dest)
                    print(f"      字体许可已复制到：{license_dest}")
                else:
                    print(
                        f"      警告：未找到 {FONT_LICENSE}，请补齐字体许可文本",
                        file=sys.stderr,
                    )

        print(f"\n全部打包完成（版本 {version}）")
        return 0
    except pybuild.BuildError as exc:
        print(f"打包失败：{exc}", file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"复制产物失败：{exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("打包已取消", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
