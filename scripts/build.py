#!/usr/bin/env python3
"""全量打包脚本：依次执行启动器（egui）与打包工具（pkgtool）两个 pybuild 打包，
产物复制到 dist/<版本号>/ 目录。

用法：
    python scripts/build.py     # = pnpm dist

产物：
    dist/<版本号>/launcherclient.exe
    dist/<版本号>/pkgtool.exe
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

import pybuild

DIST_DIR = pybuild.PROJECT_ROOT / "dist"


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
