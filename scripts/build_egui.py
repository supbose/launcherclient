#!/usr/bin/env python3
"""egui 打包脚本：把 package.json 的版本号同步到 version.rc，随后执行 cargo release 构建。

流程：
1. 读取 package.json 的 version 字段；
2. 将版本号写入 src-egui/version.rc（FILEVERSION / PRODUCTVERSION / FileVersion / ProductVersion）；
3. 校验 version.rc 写入结果后执行 cargo build --release；
4. 校验 cargo 默认 release 产物并核对 exe 内嵌版本号。
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
PACKAGE_JSON = PROJECT_ROOT / "package.json"
EGUI_DIR = PROJECT_ROOT / "src-egui"
VERSION_RC = EGUI_DIR / "version.rc"
BIN_NAME = "launcherclient"

# package.json 只允许 1.2.3 或 1.2.3.4 形式的数值版本号（可带 v 前缀）
PACKAGE_VERSION_RE = re.compile(r"^[vV]?(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?$")
# Windows 版本资源每段允许的最大值
RC_SEGMENT_MAX = 65535


class BuildError(Exception):
    """打包流程中的可预期错误，统一转成中文提示输出。"""


def read_package_version() -> str:
    try:
        with PACKAGE_JSON.open("r", encoding="utf-8") as fh:
            package = json.load(fh)
    except FileNotFoundError as exc:
        raise BuildError(f"未找到 package.json：{PACKAGE_JSON}") from exc
    except json.JSONDecodeError as exc:
        raise BuildError(f"package.json 不是合法的 JSON：{exc}") from exc
    except OSError as exc:
        raise BuildError(f"读取 package.json 失败：{exc}") from exc

    version = package.get("version") if isinstance(package, dict) else None
    if not isinstance(version, str) or not version.strip():
        raise BuildError("package.json 中缺少有效的 version 字段")
    return version.strip()


def parse_version(version: str) -> tuple[str, str]:
    """返回 (逗号分隔的四段版本号, 点分隔的四段版本号)。"""
    match = PACKAGE_VERSION_RE.match(version)
    if not match:
        raise BuildError(
            f"版本号格式不合法：{version!r}，仅支持 1.2.3 或 1.2.3.4 形式的数值版本号"
        )

    segments = [int(group) if group is not None else 0 for group in match.groups()]
    for segment in segments:
        if segment > RC_SEGMENT_MAX:
            raise BuildError(
                f"版本号 {version!r} 的某一段超出 Windows 资源版本允许范围（0-{RC_SEGMENT_MAX}）"
            )

    return ",".join(map(str, segments)), ".".join(map(str, segments))


def _replace_single_line(lines: list[str], label: str, matcher: re.Pattern[str], content: str) -> None:
    hits = [index for index, line in enumerate(lines) if matcher.match(line.strip())]
    if len(hits) != 1:
        raise BuildError(f"version.rc 中 {label} 配置项数量异常（期望 1 处，实际 {len(hits)} 处）")

    index = hits[0]
    original = lines[index]
    body = original.rstrip("\r\n")
    ending = original[len(body):]
    indent = body[: len(body) - len(body.lstrip())]
    lines[index] = indent + content + ending


def sync_version_rc(csv_version: str, dotted_version: str) -> None:
    try:
        text = VERSION_RC.read_bytes().decode("utf-8")
    except FileNotFoundError as exc:
        raise BuildError(f"未找到 version.rc：{VERSION_RC}") from exc
    except UnicodeDecodeError as exc:
        raise BuildError(f"version.rc 不是合法的 UTF-8 编码：{exc}") from exc
    except OSError as exc:
        raise BuildError(f"读取 version.rc 失败：{exc}") from exc

    lines = text.splitlines(keepends=True)
    replacements = (
        ("FILEVERSION", re.compile(r"^FILEVERSION\b"), f"FILEVERSION {csv_version}"),
        ("PRODUCTVERSION", re.compile(r"^PRODUCTVERSION\b"), f"PRODUCTVERSION {csv_version}"),
        (
            'VALUE "FileVersion"',
            re.compile(r'^VALUE\s+"FileVersion"'),
            f'VALUE "FileVersion", "{dotted_version}"',
        ),
        (
            'VALUE "ProductVersion"',
            re.compile(r'^VALUE\s+"ProductVersion"'),
            f'VALUE "ProductVersion", "{dotted_version}"',
        ),
    )
    for label, matcher, content in replacements:
        _replace_single_line(lines, label, matcher, content)

    if not any(line.strip() == "#pragma code_page(65001)" for line in lines):
        raise BuildError("version.rc 缺少 #pragma code_page(65001)，中文资源会按系统 ANSI 代码页解析导致乱码")

    temp_path = VERSION_RC.with_suffix(".rc.tmp")
    try:
        with temp_path.open("w", encoding="utf-8", newline="") as fh:
            fh.write("".join(lines))
        os.replace(temp_path, VERSION_RC)
    except OSError as exc:
        raise BuildError(f"写入 version.rc 失败：{exc}") from exc

    try:
        written_lines = VERSION_RC.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise BuildError(f"回读 version.rc 校验失败：{exc}") from exc

    for label, _, content in replacements:
        if not any(line.strip() == content for line in written_lines):
            raise BuildError(f"version.rc 写入后校验失败：未找到正确的 {label} 配置")


def run_cargo_build() -> Path:
    try:
        subprocess.run(["cargo", "build", "--release"], cwd=str(EGUI_DIR), check=True)
    except FileNotFoundError as exc:
        raise BuildError("未找到 cargo 命令，请确认 Rust 工具链已安装并加入 PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise BuildError(f"cargo build --release 执行失败，退出码 {exc.returncode}") from exc

    exe_name = f"{BIN_NAME}.exe" if os.name == "nt" else BIN_NAME
    return EGUI_DIR / "target" / "release" / exe_name


def verify_artifact(exe_path: Path, dotted_version: str) -> None:
    if not exe_path.is_file():
        raise BuildError(f"构建完成但未找到产物：{exe_path}")

    size_mb = exe_path.stat().st_size / (1024 * 1024)
    print(f"      产物路径：{exe_path}（{size_mb:.2f} MB）")

    if os.name != "nt":
        return

    ps_command = f"(Get-Item -LiteralPath '{exe_path}').VersionInfo.FileVersion"
    try:
        result = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps_command],
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError:
        print("      警告：未找到 powershell，跳过产物内嵌版本号校验")
        return

    actual = result.stdout.strip()
    if result.returncode != 0 or not actual:
        detail = result.stderr.strip() or "无输出"
        print(f"      警告：读取产物版本信息失败，跳过校验（{detail}）")
        return
    if actual != dotted_version:
        raise BuildError(f"产物内嵌版本号与预期不一致：实际 {actual}，预期 {dotted_version}")
    print(f"      已确认产物内嵌版本号：{actual}")


def main() -> int:
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, OSError):
        pass

    try:
        print(f"[1/4] 读取 package.json 版本号：{PACKAGE_JSON}")
        version = read_package_version()
        csv_version, dotted_version = parse_version(version)
        print(f"      版本号：{version} -> rc {csv_version} / 字符串 {dotted_version}")

        print(f"[2/4] 同步版本号到 version.rc：{VERSION_RC}")
        sync_version_rc(csv_version, dotted_version)
        print("      版本号配置已写入并通过回读校验")

        print(f"[3/4] 执行 cargo build --release：{EGUI_DIR}")
        exe_path = run_cargo_build()
        print("      cargo 构建成功")

        print("[4/4] 校验 cargo 默认 release 产物")
        verify_artifact(exe_path, dotted_version)

        print(f"打包完成：版本 {dotted_version}")
        return 0
    except BuildError as exc:
        print(f"打包失败：{exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("打包已取消", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
