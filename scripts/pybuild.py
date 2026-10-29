#!/usr/bin/env python3
"""pybuild 打包脚本：把 package.json 的版本号同步到目标 crate 的版本资源 rc 文件，随后执行 cargo release 构建。

用法：
    python scripts/pybuild.py egui      # 启动器   src-egui/version.rc  -> launcherclient.exe
    python scripts/pybuild.py pkgtool   # 打包工具 src-pkgtool/icon.rc  -> pkgtool.exe

可选构建参数（编译期烘焙进 exe 内置默认值，替代 qsm 默认产品线；未传项保持默认）：
    --name <标题>              默认启动器标题（窗口标题栏左侧显示文字）
    --app-name <名称>          默认应用名（决定配置/下载/安装目录）
    --manifest-url <地址>      默认 manifest 地址，可重复传实现按序故障转移
    --homepage <地址>          默认官网首页
    --template <A-H>           默认界面模板
    --retry-times <0-10>       默认自动重试次数（含首次）
    --max-versions <N>         默认保留版本目录数
    --service <服务名>         默认更新前卸载的 Windows 服务，可重复传

参数经 QSLTD_* 环境变量传给 src-core/build.rs 生成覆盖常量（见 defaults.rs）。

流程：
1. 读取 package.json 的 version 字段；
2. 将版本号写入目标 rc（FILEVERSION / PRODUCTVERSION / FileVersion / ProductVersion）；
3. 校验 rc 写入结果后执行 cargo build --release（附带构建参数环境变量）；
4. 校验 cargo 默认 release 产物并核对 exe 内嵌版本号。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
PACKAGE_JSON = PROJECT_ROOT / "package.json"


class Target:
    def __init__(self, name: str, src_dir: Path, rc_name: str, bin_name: str) -> None:
        self.name = name
        self.src_dir = src_dir
        self.rc_path = src_dir / rc_name
        self.bin_name = bin_name


TARGETS = {
    "egui": Target("egui", PROJECT_ROOT / "src-egui", "version.rc", "launcherclient"),
    "pkgtool": Target("pkgtool", PROJECT_ROOT / "src-pkgtool", "icon.rc", "pkgtool"),
}

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
        raise BuildError(f"rc 文件中 {label} 配置项数量异常（期望 1 处，实际 {len(hits)} 处）")

    index = hits[0]
    original = lines[index]
    body = original.rstrip("\r\n")
    ending = original[len(body):]
    indent = body[: len(body) - len(body.lstrip())]
    lines[index] = indent + content + ending


def sync_version_rc(target: Target, csv_version: str, dotted_version: str) -> None:
    try:
        text = target.rc_path.read_bytes().decode("utf-8")
    except FileNotFoundError as exc:
        raise BuildError(f"未找到 rc 文件：{target.rc_path}") from exc
    except UnicodeDecodeError as exc:
        raise BuildError(f"rc 文件不是合法的 UTF-8 编码：{exc}") from exc
    except OSError as exc:
        raise BuildError(f"读取 rc 文件失败：{exc}") from exc

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
        raise BuildError("rc 文件缺少 #pragma code_page(65001)，中文资源会按系统 ANSI 代码页解析导致乱码")

    temp_path = target.rc_path.with_suffix(".rc.tmp")
    try:
        with temp_path.open("w", encoding="utf-8", newline="") as fh:
            fh.write("".join(lines))
        os.replace(temp_path, target.rc_path)
    except OSError as exc:
        raise BuildError(f"写入 rc 文件失败：{exc}") from exc

    try:
        written_lines = target.rc_path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise BuildError(f"回读 rc 文件校验失败：{exc}") from exc

    for label, _, content in replacements:
        if not any(line.strip() == content for line in written_lines):
            raise BuildError(f"rc 文件写入后校验失败：未找到正确的 {label} 配置")


def run_cargo_build(target: Target, build_env: dict[str, str]) -> Path:
    env = {**os.environ, **build_env}
    try:
        subprocess.run(["cargo", "build", "--release"], cwd=str(target.src_dir), check=True, env=env)
    except FileNotFoundError as exc:
        raise BuildError("未找到 cargo 命令，请确认 Rust 工具链已安装并加入 PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise BuildError(f"cargo build --release 执行失败，退出码 {exc.returncode}") from exc

    exe_name = f"{target.bin_name}.exe" if os.name == "nt" else target.bin_name
    return target.src_dir / "target" / "release" / exe_name


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


def build_target(target: Target, csv_version: str, dotted_version: str, build_env: dict[str, str] | None = None) -> Path:
    """执行单目标打包：同步 rc 版本 → cargo release（附带构建参数环境变量）→ 校验产物，返回 exe 路径。"""
    print(f"[1/3] 同步版本号到 rc 文件：{target.rc_path}")
    sync_version_rc(target, csv_version, dotted_version)
    print("      版本号配置已写入并通过回读校验")

    print(f"[2/3] 执行 cargo build --release：{target.src_dir}")
    exe_path = run_cargo_build(target, build_env or {})
    print("      cargo 构建成功")

    print("[3/3] 校验 cargo 默认 release 产物")
    verify_artifact(exe_path, dotted_version)
    print(f"打包完成（{target.name}）：版本 {dotted_version}")
    return exe_path


# ---------------------------------------------------------------------------
# 构建参数：命令行接收 → 校验 → 转 QSLTD_* 环境变量（src-core/build.rs 消费）
# ---------------------------------------------------------------------------

APP_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
VALID_TEMPLATES = "ABCDEFGH"
# 启动器标题最长字符数：标题栏单行可容纳上限，超长会挤压右侧按钮区
LAUNCHER_NAME_MAX = 64


def collect_build_env(args: argparse.Namespace) -> dict[str, str]:
    """校验并收集构建参数，返回要注入 cargo 子进程的环境变量（未传项不出现，走内置默认）。"""
    env: dict[str, str] = {}

    if args.app_name is not None:
        if not APP_NAME_RE.match(args.app_name):
            raise BuildError(f"--app-name 不合法：{args.app_name!r}，仅允许字母数字开头、由字母数字 . _ - 组成")
        env["QSLTD_APP_NAME"] = args.app_name

    if args.name is not None:
        launcher_name = args.name.strip()
        if not launcher_name:
            raise BuildError("--name 不合法：标题不能为空")
        if any(ch in launcher_name for ch in "\r\n"):
            raise BuildError(f"--name 不合法：{args.name!r}，标题不能包含换行符")
        if len(launcher_name) > LAUNCHER_NAME_MAX:
            raise BuildError(
                f"--name 过长：{len(launcher_name)} 字符，最多 {LAUNCHER_NAME_MAX} 字符"
            )
        env["QSLTD_NAME"] = launcher_name

    if args.manifest_url:
        for url in args.manifest_url:
            if not url.startswith(("http://", "https://")):
                raise BuildError(f"--manifest-url 不合法：{url!r}，需以 http:// 或 https:// 开头")
        env["QSLTD_MANIFEST_URLS"] = "|".join(args.manifest_url)

    if args.homepage is not None:
        if not args.homepage.startswith(("http://", "https://")):
            raise BuildError(f"--homepage 不合法：{args.homepage!r}，需以 http:// 或 https:// 开头")
        env["QSLTD_HOMEPAGE"] = args.homepage

    if args.template is not None:
        template = args.template.upper()
        if template not in VALID_TEMPLATES:
            raise BuildError(f"--template 不合法：{args.template!r}，仅支持 A~H")
        env["QSLTD_LAUNCHER_TEMPLATE"] = template

    if args.retry_times is not None:
        if not 0 <= args.retry_times <= 10:
            raise BuildError(f"--retry-times 超出范围：{args.retry_times}，允许 0~10（0 视为 1）")
        env["QSLTD_RETRY_TIMES"] = str(args.retry_times)

    if args.max_versions is not None:
        if args.max_versions < 1:
            raise BuildError(f"--max-versions 不合法：{args.max_versions}，至少保留 1 个版本目录")
        env["QSLTD_MAX_VERSIONS"] = str(args.max_versions)

    if args.service:
        for svc in args.service:
            if not svc.strip() or "|" in svc:
                raise BuildError(f"--service 不合法：{svc!r}，服务名不能为空或含 |")
        env["QSLTD_SERVICES"] = "|".join(s.strip() for s in args.service)

    return env


def make_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="pybuild.py",
        description="打包目标 crate：同步 package.json 版本号到 rc → cargo build --release → 校验产物。\n"
        "可选构建参数在编译期烘焙进 exe 内置默认值（替代 qsm 默认产品线），未传项保持默认。",
        epilog=(
            "构建参数说明（全部可选，经 QSLTD_* 环境变量传给 src-core/build.rs 生成覆盖常量）：\n"
            "  --name           烘焙后作为启动器窗口标题栏左侧标题（默认「启动器」）\n"
            "  --app-name       烘焙后决定 exe 无 update.json 时的默认应用名及配置/下载/安装目录\n"
            "  --manifest-url   可重复传，顺序即故障转移顺序；仅传一个则无备用地址\n"
            "  --service        可重复传，仅影响生成的 update.json 模板；运行时未配置 services 则不卸载\n"
            "  其余参数分别对应内置默认 官网首页 / 界面模板 / 重试次数 / 保留版本目录数\n\n"
            "辅助参数：\n"
            "  --version        仅打印 package.json 的构建版本号（即打包将烘焙进 rc 的版本）后退出，不执行打包\n\n"
            "示例：\n"
            "  python scripts/pybuild.py --version                 # 查看当前构建版本号\n"
            "  python scripts/pybuild.py egui                      # 默认 qsm 产品线\n"
            "  python scripts/pybuild.py egui --app-name myapp --manifest-url https://a.myapp.ltd/version.json --manifest-url https://b.myapp.ltd/version.json --homepage https://www.myapp.ltd --template B --retry-times 5 --max-versions 3 --service MySvc\n"
            "  python scripts/pybuild.py pkgtool                   # 打包工具（构建参数一般不需要）\n"
            "  pnpm qsm:release --app-name myapp                   # 经 package.json 入口透传\n\n"
            "验证烘焙结果：把产物放入空目录运行一次 --check-only，自动生成的 update.json 即为烘焙后的默认值。"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("target", choices=sorted(TARGETS), help="打包目标：egui=启动器 launcherclient.exe，pkgtool=打包工具 pkgtool.exe")
    try:
        version_text = read_package_version()
    except BuildError:
        version_text = "未知（package.json 不可读）"
    parser.add_argument("--version", action="version", version=f"launcherclient {version_text}", help="显示 package.json 中的构建版本号（即本次打包将烘焙进 rc 的版本）后退出")
    parser.add_argument("--name", metavar="标题", help="默认启动器标题（窗口标题栏左侧显示文字，内置默认：启动器），非空、不含换行，最长 64 字符")
    parser.add_argument("--app-name", metavar="名称", help="默认应用名（内置默认：qsm），仅允许字母数字开头、由字母数字 . _ - 组成")
    parser.add_argument("--manifest-url", metavar="地址", action="append", default=[], help="默认 manifest 地址（内置默认：qsm 双 CDN 容灾），需以 http(s):// 开头，可重复传")
    parser.add_argument("--homepage", metavar="地址", help="默认官网首页地址（内置默认：https://www.qcsm.ltd），需以 http(s):// 开头")
    parser.add_argument("--template", metavar="A-H", help="默认界面模板（内置默认：F），可选 A~H")
    parser.add_argument("--retry-times", metavar="0-10", type=int, help="默认自动重试次数，含首次（内置默认：3，0 视为 1）")
    parser.add_argument("--max-versions", metavar="N", type=int, help="默认保留版本目录数（内置默认：2），至少为 1")
    parser.add_argument("--service", metavar="服务名", action="append", default=[], help="默认更新前卸载的 Windows 服务名（内置默认：QsmServer），可重复传")
    return parser


def main() -> int:
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, OSError):
        pass

    args = make_parser().parse_args()
    target = TARGETS[args.target]
    try:
        build_env = collect_build_env(args)
        if build_env:
            print("构建参数覆盖（未列项使用内置默认）：")
            for key, value in build_env.items():
                print(f"      {key} = {value}")
        else:
            print("未传入构建参数，使用内置默认产品线配置（qsm）")

        print(f"读取 package.json 版本号：{PACKAGE_JSON}")
        version = read_package_version()
        csv_version, dotted_version = parse_version(version)
        print(f"      版本号：{version} -> rc {csv_version} / 字符串 {dotted_version}")

        build_target(target, csv_version, dotted_version, build_env)
        return 0
    except BuildError as exc:
        print(f"打包失败：{exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("打包已取消", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
