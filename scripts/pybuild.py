#!/usr/bin/env python3
"""pybuild 打包脚本：把 package.json 的版本号同步到全部版本声明处，随后执行 cargo release 构建。

用法：
    python scripts/pybuild.py egui      # 启动器   src-egui/version.rc  -> target/release/launcherclient.exe
    python scripts/pybuild.py pkgtool   # 打包工具 src-pkgtool/icon.rc  -> src-pkgtool/target/release/pkgtool.exe
    python scripts/pybuild.py --check-version   # 只读：校验各处版本号是否一致（CI 门禁用）
    python scripts/pybuild.py --sync-version    # 只同步版本号，不构建（改完 package.json 后手动铺开）

版本号只有一个来源：package.json 的 version。其余五个文件（根 Cargo.toml 的
[workspace.package]、src-pkgtool/Cargo.toml、两个 rc 的 FILEVERSION/PRODUCTVERSION
与 FileVersion/ProductVersion 字符串）都是它的派生物，由本脚本写入并在构建后回读校验。
rc 与 Cargo 此前只有「被构建的那个目标」会被同步，所以长期漂移（icon.rc 停在旧版本）；
现在每次构建统一铺开，--check-version 再兜一层。

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

可选代码签名（默认不启用；没证书时构建行为与从前完全一致）：
    --codesign-pfx <路径>      Authenticode 签名证书（.pfx），给了才会执行签名
    --codesign-tsa <地址>      RFC3161 时间戳服务；不填则签名过期即失效
    --codesign-pass-env <名>   存放 pfx 口令的环境变量名（默认 QSLTD_SIGN_PFX_PASSWORD）
    --signtool <路径>          signtool.exe 路径；默认取 PATH 或 Windows SDK 下同名工具
    口令从环境变量取值，不进仓库、不进 shell 历史；但它最终仍会作为 /p 参数出现在
    那次 signtool 的进程命令行里（同机其他用户可见）。发布机更稳妥的做法是把证书
    导入本机证书库后改用 /sha1 选择，完全不传口令——本脚本暂未提供该模式。

流程：
1. 读取 package.json 的 version 字段；
2. 将版本号写入根 Cargo.toml、src-pkgtool/Cargo.toml 与两个 rc，并回读校验；
3. 执行 cargo build --release（附带构建参数环境变量）；
4. 校验 release 产物存在、内嵌版本号一致；
5. 仅在传入 --codesign-pfx 时签名并 verify /pa 校验。
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
PACKAGE_JSON = PROJECT_ROOT / "package.json"
# Rust 侧版本号的两个写入点：workspace 成员从根 [workspace.package] 继承，
# src-pkgtool 不在 workspace 内（见根 Cargo.toml 的 exclude 说明），保留自己的 version
WORKSPACE_MANIFEST = PROJECT_ROOT / "Cargo.toml"
WORKSPACE_LABEL = "Cargo.toml [workspace.package]"
PKGTOOL_MANIFEST = PROJECT_ROOT / "src-pkgtool" / "Cargo.toml"
PKGTOOL_LABEL = "src-pkgtool/Cargo.toml [package]"


class Target:
    def __init__(
        self,
        name: str,
        src_dir: Path,
        rc_name: str,
        bin_name: str,
        workspace_member: bool,
    ) -> None:
        self.name = name
        self.src_dir = src_dir
        self.rc_path = src_dir / rc_name
        self.bin_name = bin_name
        # workspace 成员的产物统一落在仓库根的 target/：Cargo 认 workspace 根目录，
        # 不再往各自 crate 下写。非成员（pkgtool）仍是 crate 内 target/。
        self.workspace_member = workspace_member

    @property
    def target_dir(self) -> Path:
        return PROJECT_ROOT if self.workspace_member else self.src_dir

    @property
    def artifact_dir(self) -> Path:
        return self.target_dir / "target" / "release"


TARGETS = {
    "egui": Target("egui", PROJECT_ROOT / "src-egui", "version.rc", "launcherclient", True),
    "pkgtool": Target("pkgtool", PROJECT_ROOT / "src-pkgtool", "icon.rc", "pkgtool", False),
}

# package.json 只允许 1.2.3 或 1.2.3.4 形式的数值版本号（可带 v 前缀）
PACKAGE_VERSION_RE = re.compile(r"^[vV]?(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?$")
# Windows 版本资源每段允许的最大值
RC_SEGMENT_MAX = 65535
# Cargo 清单里的 `version = "x.y.z"` 行（行首，避开依赖表里的 { version = ... } 内联写法）
CARGO_VERSION_RE = re.compile(r'^version\s*=\s*"([^"]*)"', re.MULTILINE)
# rc 里的字符串版本（VALUE "FileVersion", "x.y.z.w"）；rc 资源块是缩进的，行首要跳过
RC_STRING_VERSION_RE = re.compile(r'^\s*VALUE\s+"FileVersion",\s*"([^"]*)"', re.MULTILINE)



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


def version_segments(version: str) -> list[int]:
    """把 package.json 的版本解析成定长四段整数（缺第四段补 0）。"""
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
    return segments


def parse_version(version: str) -> tuple[str, str]:
    """返回 (逗号分隔的四段版本号, 点分隔的四段版本号)。"""
    segments = version_segments(version)
    return ",".join(map(str, segments)), ".".join(map(str, segments))


def _replace_single_line(lines: list[str], label: str, matcher: re.Pattern[str], content: str) -> None:
    hits = [index for index, line in enumerate(lines) if matcher.match(line.strip())]
    if len(hits) != 1:
        raise BuildError(f"{label} 配置项数量异常（期望 1 处，实际 {len(hits)} 处）")

    index = hits[0]
    original = lines[index]
    body = original.rstrip("\r\n")
    ending = original[len(body):]
    indent = body[: len(body) - len(body.lstrip())]
    lines[index] = indent + content + ending


def _read_lines(path: Path, label: str) -> list[str]:
    """按字节读后再解码，保住原换行符。

    本仓库的 rc 与 Cargo.toml 目前都是 LF，但 Windows 仓库里 CRLF 很常见；
    若用文本模式读（通用换行）再写回，整个文件的行尾会被统一改写，
    一次改版本号的提交能撑出满屏 diff。按字节读 + keepends 切分则原样进出。
    """
    try:
        text = path.read_bytes().decode("utf-8")
    except FileNotFoundError as exc:
        raise BuildError(f"未找到 {label}：{path}") from exc
    except UnicodeDecodeError as exc:
        raise BuildError(f"{label} 不是合法的 UTF-8 编码：{exc}") from exc
    except OSError as exc:
        raise BuildError(f"读取 {label} 失败：{exc}") from exc
    return text.splitlines(keepends=True)


def _write_lines(path: Path, label: str, lines: list[str]) -> None:
    """先写临时文件再原子替换，避免中断时留下半截配置"""
    temp_path = path.with_name(path.name + ".tmp")
    try:
        with temp_path.open("w", encoding="utf-8", newline="") as fh:
            fh.write("".join(lines))
        os.replace(temp_path, path)
    except OSError as exc:
        temp_path.unlink(missing_ok=True)
        raise BuildError(f"写入 {label} 失败：{exc}") from exc


def sync_version_rc(target: Target, csv_version: str, dotted_version: str) -> None:
    label = f"rc 文件 {target.rc_path.name}"
    lines = _read_lines(target.rc_path, label)
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
    for item_label, matcher, content in replacements:
        _replace_single_line(lines, f"{label} 的 {item_label}", matcher, content)

    if not any(line.strip() == "#pragma code_page(65001)" for line in lines):
        raise BuildError("rc 文件缺少 #pragma code_page(65001)，中文资源会按系统 ANSI 代码页解析导致乱码")

    _write_lines(target.rc_path, label, lines)

    written_lines = _read_lines(target.rc_path, label)
    for item_label, _, content in replacements:
        if not any(line.strip() == content for line in written_lines):
            raise BuildError(f"rc 文件写入后校验失败：未找到正确的 {item_label} 配置")


# ---------------------------------------------------------------------------
# Cargo 清单的版本字段：与 rc 同批铺开，package.json 是唯一来源
# ---------------------------------------------------------------------------


def read_cargo_version(path: Path, label: str) -> str:
    lines = _read_lines(path, label)
    hits = [line for line in lines if line.strip().startswith("version") and "=" in line]
    quoted = [line for line in hits if '"' in line]
    if len(quoted) != 1:
        raise BuildError(
            f"{label} 中形如 version = \"x.y.z\" 的行数量异常（期望 1 处，实际 {len(quoted)} 处）"
        )
    match = CARGO_VERSION_RE.search(quoted[0])
    if match is None:
        raise BuildError(f"{label} 的 version 行无法解析：{quoted[0].strip()}")
    return match.group(1)


def sync_cargo_version(path: Path, label: str, version: str) -> None:
    lines = _read_lines(path, label)
    _replace_single_line(lines, f'{label} 的 version', CARGO_VERSION_RE, f'version = "{version}"')
    _write_lines(path, label, lines)
    if read_cargo_version(path, label) != version:
        raise BuildError(f"{label} 写入后校验失败：期望 {version}")


def cargo_version_of(segments: list[int]) -> str:
    """Cargo 清单只接受三段 semver，rc 用四段。

    package.json 允许写成 1.2.3.4；这种四段版本里第四段只能进 rc 资源，
    Cargo 侧退化为前三段——不能不吭声，调用方会把它作为提示打印出来。
    """
    return ".".join(str(segment) for segment in segments[:3])


def read_rc_version(path: Path, label: str) -> str:
    text = "".join(_read_lines(path, label))
    matches = RC_STRING_VERSION_RE.findall(text)
    if len(matches) != 1:
        raise BuildError(f"{label} 中 VALUE \"FileVersion\" 数量异常（期望 1 处，实际 {len(matches)} 处）")
    return matches[0]


def sync_all_version_sources(version: str, segments: list[int]) -> list[str]:
    """把 package.json 的版本铺开到全部派生位置，返回改动说明列表"""
    csv_version, dotted_version = parse_version(version)
    cargo_version = cargo_version_of(segments)
    touched: list[str] = []

    sync_cargo_version(WORKSPACE_MANIFEST, WORKSPACE_LABEL, cargo_version)
    touched.append(f"{WORKSPACE_LABEL} = {cargo_version}")
    sync_cargo_version(PKGTOOL_MANIFEST, PKGTOOL_LABEL, cargo_version)
    touched.append(f"{PKGTOOL_LABEL} = {cargo_version}")

    # 两个 rc 一起写：此前只同步「本次构建的目标」，另一个会长期停在旧版本
    for target in TARGETS.values():
        sync_version_rc(target, csv_version, dotted_version)
        touched.append(f"{target.rc_path.name} = {dotted_version}")

    return touched


def collect_version_declarations(version: str, segments: list[int]) -> list[tuple[str, str, str]]:
    """返回 (位置, 期望值, 实际值)；实际值读不到时记「读取失败: 原因」"""
    cargo_expected = cargo_version_of(segments)
    _, rc_expected = parse_version(version)
    wanted = [
        (str(PACKAGE_JSON.relative_to(PROJECT_ROOT)), version, version),
        (WORKSPACE_LABEL, cargo_expected, None),
        (PKGTOOL_LABEL, cargo_expected, None),
    ]
    for target in TARGETS.values():
        wanted.append((str(target.rc_path.relative_to(PROJECT_ROOT)), rc_expected, None))

    collected: list[tuple[str, str, str]] = []
    for label, expected, provided in wanted:
        if provided is not None:
            collected.append((label, expected, provided))
            continue
        try:
            if label.endswith(".rc"):
                actual = read_rc_version(PROJECT_ROOT / label, label)
            elif label == WORKSPACE_LABEL:
                actual = read_cargo_version(WORKSPACE_MANIFEST, label)
            else:
                actual = read_cargo_version(PKGTOOL_MANIFEST, label)
        except BuildError as exc:
            actual = f"读取失败：{exc}"
        collected.append((label, expected, actual))
    return collected



def run_cargo_build(target: Target, build_env: dict[str, str]) -> Path:
    env = {**os.environ, **build_env}
    try:
        subprocess.run(["cargo", "build", "--release"], cwd=str(target.src_dir), check=True, env=env)
    except FileNotFoundError as exc:
        raise BuildError("未找到 cargo 命令，请确认 Rust 工具链已安装并加入 PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise BuildError(f"cargo build --release 执行失败，退出码 {exc.returncode}") from exc

    exe_name = f"{target.bin_name}.exe" if os.name == "nt" else target.bin_name
    return target.artifact_dir / exe_name


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


# ---------------------------------------------------------------------------
# 可选 Authenticode 签名：默认整条链路不启用，行为与加这一步之前完全一致
# ---------------------------------------------------------------------------

DEFAULT_SIGN_PASSWORD_ENV = "QSLTD_SIGN_PFX_PASSWORD"
# Windows SDK 里 signtool 的常见位置；版本号不确定，glob 后取最高版本
SIGNTOOL_SDK_GLOBS = (
    r"C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe",
    r"C:\Program Files\Windows Kits\10\bin\*\x64\signtool.exe",
)


def resolve_signtool(explicit: str | None) -> Path:
    if explicit:
        path = Path(explicit)
        if not path.is_file():
            raise BuildError(f"--signtool 指向的文件不存在：{path}")
        return path

    found = shutil.which("signtool")
    if found:
        return Path(found)

    candidates: list[Path] = []
    for pattern in SIGNTOOL_SDK_GLOBS:
        candidates.extend(Path(match) for match in glob.glob(pattern))
    if candidates:
        return max(candidates)
    raise BuildError(
        "未找到 signtool.exe：请安装 Windows SDK，或用 --signtool <完整路径> 指定"
    )


def codesign_options(args: argparse.Namespace) -> dict[str, object] | None:
    """把签名参数收敛成一个可空配置；没传 --codesign-pfx 就是「不签名」"""
    if not args.codesign_pfx:
        if args.codesign_tsa:
            print("      提示：只给了 --codesign-tsa 而没有 --codesign-pfx，本次不签名")
        return None
    if os.name != "nt":
        raise BuildError("Authenticode 签名依赖 signtool，只能在 Windows 上执行")

    pfx_path = Path(args.codesign_pfx)
    if not pfx_path.is_file():
        raise BuildError(f"未找到签名证书：{pfx_path}")

    return {
        "pfx": pfx_path,
        "tsa": (args.codesign_tsa or "").strip(),
        "password_env": (args.codesign_pass_env or DEFAULT_SIGN_PASSWORD_ENV).strip()
        or DEFAULT_SIGN_PASSWORD_ENV,
        "signtool": resolve_signtool(args.signtool),
    }


def sign_artifact(exe_path: Path, options: dict[str, object]) -> None:
    signtool = Path(options["signtool"])
    password_env = str(options["password_env"])
    password = os.environ.get(password_env, "")

    command = [str(signtool), "sign", "/fd", "SHA256", "/f", str(options["pfx"])]
    if password:
        # 注意：signtool 的 /p 会把口令放进进程命令行，同机的其他用户/监控能看到。
        # 更适合 CI 的做法是把证书导到机器证书库后用 /sha1 选择，不带口令；
        # 这里从环境变量取值，至少保证口令不进 shell 历史与仓库。
        command += ["/p", password]
    else:
        print(f"      提示：环境变量 {password_env} 未设置，不传 /p（pfx 有口令时 signtool 会交互提示或失败）")

    timestamp = str(options["tsa"])
    if timestamp:
        command += ["/tr", timestamp, "/td", "SHA256"]
    else:
        print("      警告：未提供 --codesign-tsa，证书过期后签名即失效（历史包全部报红）")

    command.append(str(exe_path))

    try:
        result = subprocess.run(command, capture_output=True, text=True, check=False)
    except FileNotFoundError as exc:
        raise BuildError(f"signtool 无法执行：{exc}") from exc

    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip() or "无输出"
        # signtool 的错误输出不回显口令，可安全打印
        raise BuildError(f"signtool sign 失败（退出码 {result.returncode}）：{detail}")

    verify = [str(signtool), "verify", "/pa", str(exe_path)]
    verify_result = subprocess.run(verify, capture_output=True, text=True, check=False)
    if verify_result.returncode != 0:
        detail = (verify_result.stderr or verify_result.stdout or "").strip() or "无输出"
        raise BuildError(f"签名已写入但校验失败（signtool verify /pa）：{detail}")

    suffix = "（含时间戳）" if timestamp else "（无时间戳）"
    print(f"      已完成 Authenticode 签名并校验通过{suffix}")


def build_target(
    target: Target,
    csv_version: str,
    dotted_version: str,
    build_env: dict[str, str] | None = None,
    version: str | None = None,
    segments: list[int] | None = None,
    sign: dict[str, object] | None = None,
) -> Path:
    """执行单目标打包：铺开版本号 → cargo release → 校验产物 →（可选）签名，返回 exe 路径。"""
    total = 4 if sign else 3
    print(f"[1/{total}] 同步版本号到 Cargo 清单与两个 rc")
    if version is not None and segments is not None:
        for note in sync_all_version_sources(version, segments):
            print(f"      {note}")
    else:
        # 调用方没给 package.json 原文时（例如外部只关心 rc），退化为只同步本目标 rc
        sync_version_rc(target, csv_version, dotted_version)
        print(f"      {target.rc_path.name} = {dotted_version}")
    print("      版本号配置已写入并通过回读校验")

    print(f"[2/{total}] 执行 cargo build --release：{target.src_dir}")
    exe_path = run_cargo_build(target, build_env or {})
    print("      cargo 构建成功")

    print(f"[3/{total}] 校验 release 产物")
    verify_artifact(exe_path, dotted_version)

    if sign:
        print(f"[4/{total}] Authenticode 签名：{Path(sign['pfx']).name}")
        sign_artifact(exe_path, sign)

    print(f"打包完成（{target.name}）：版本 {dotted_version}")
    return exe_path


def check_versions(version: str, segments: list[int]) -> int:
    """只读校验：五处版本声明是否都等于 package.json 推导出的期望值。"""
    rows = collect_version_declarations(version, segments)
    drift = [row for row in rows if row[1] != row[2]]
    width = max(len(row[0]) for row in rows)
    print("版本号一致性检查（期望值来自 package.json）：")
    for label, expected, actual in rows:
        mark = "OK  " if expected == actual else "漂移"
        print(f"      [{mark}] {label.ljust(width)}  期望 {expected:<10} 实际 {actual}")

    if not drift:
        if segments[3] != 0:
            print(f"      提示：四段版本 {version} 的第四段仅进入 rc 资源，Cargo 侧记为 {cargo_version_of(segments)}")
        print("版本号全部一致")
        return 0

    print(f"版本号不一致：{len(drift)} 处与 package.json 不符。", file=sys.stderr)
    print("      修复：python scripts/pybuild.py --sync-version（或正常打包一次，构建会自动铺开）", file=sys.stderr)
    return 1



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
        description="打包目标 crate：铺开 package.json 版本号到 Cargo 清单与 rc → cargo build --release → 校验产物（可选签名）。\n"
        "可选构建参数在编译期烘焙进 exe 内置默认值（替代 qsm 默认产品线），未传项保持默认。",
        epilog=(
            "构建参数说明（全部可选，经 QSLTD_* 环境变量传给 src-core/build.rs 生成覆盖常量）：\n"
            "  --name           烘焙后作为启动器窗口标题栏左侧标题（默认「启动器」）\n"
            "  --app-name       烘焙后决定 exe 无 update.json 时的默认应用名及配置/下载/安装目录\n"
            "  --manifest-url   可重复传，顺序即故障转移顺序；仅传一个则无备用地址\n"
            "  --service        可重复传，仅影响生成的 update.json 模板；运行时未配置 services 则不卸载\n"
            "  其余参数分别对应内置默认 官网首页 / 界面模板 / 重试次数 / 保留版本目录数\n\n"
            "辅助参数（不带 target，只做版本号或签名的旁路操作）：\n"
            "  --version        仅打印 package.json 的构建版本号后退出\n"
            "  --check-version  只读校验五处版本声明是否一致，不一致退出码 1（CI 门禁用）\n"
            "  --sync-version   只把版本号铺开到 Cargo 清单与 rc，不执行构建\n"
            "  --show-paths     打印各目标构建产物的落盘路径（workspace 成员在根 target/）\n\n"
            "可选签名（不传 --codesign-pfx 即完全不签名）：\n"
            "  --codesign-pfx <路径> / --codesign-tsa <时间戳地址> /\n"
            "  --codesign-pass-env <环境变量名，默认 QSLTD_SIGN_PFX_PASSWORD> / --signtool <路径>\n\n"
            "示例：\n"
            "  python scripts/pybuild.py --check-version               # 提交前自查版本一致性\n"
            "  python scripts/pybuild.py --version                     # 查看当前构建版本号\n"
            "  python scripts/pybuild.py egui                          # 默认 qsm 产品线\n"
            "  python scripts/pybuild.py egui --app-name myapp --manifest-url https://a.myapp.ltd/version.json --manifest-url https://b.myapp.ltd/version.json --homepage https://www.myapp.ltd --template B --retry-times 5 --max-versions 3 --service MySvc\n"
            "  python scripts/pybuild.py pkgtool                       # 打包工具（构建参数一般不需要）\n"
            "  python scripts/pybuild.py egui --codesign-pfx release.pfx --codesign-tsa http://timestamp.digicert.com\n"
            "  pnpm qsm:release --app-name myapp                       # 经 package.json 入口透传\n\n"
            "验证烘焙结果：把产物放入空目录运行一次 --check-only，自动生成的 update.json 即为烘焙后的默认值。"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    # target 变成可选：--check-version / --sync-version / --version 这类旁路操作不需要目标
    parser.add_argument("target", nargs="?", choices=sorted(TARGETS), help="打包目标：egui=启动器 launcherclient.exe，pkgtool=打包工具 pkgtool.exe")
    try:
        version_text = read_package_version()
    except BuildError:
        version_text = "未知（package.json 不可读）"
    parser.add_argument("--version", action="version", version=f"launcherclient {version_text}", help="显示 package.json 中的构建版本号（即本次打包将烘焙进 rc 的版本）后退出")
    parser.add_argument("--check-version", action="store_true", help="只读校验各处版本号是否与 package.json 一致，不构建")
    parser.add_argument("--sync-version", action="store_true", help="只把版本号写入 Cargo 清单与 rc，不构建")
    parser.add_argument("--show-paths", action="store_true", help="打印各目标产物落盘路径后退出")
    parser.add_argument("--name", metavar="标题", help="默认启动器标题（窗口标题栏左侧显示文字，内置默认：启动器），非空、不含换行，最长 64 字符")
    parser.add_argument("--app-name", metavar="名称", help="默认应用名（内置默认：qsm），仅允许字母数字开头、由字母数字 . _ - 组成")
    parser.add_argument("--manifest-url", metavar="地址", action="append", default=[], help="默认 manifest 地址（内置默认：qsm 双 CDN 容灾），需以 http(s):// 开头，可重复传")
    parser.add_argument("--homepage", metavar="地址", help="默认官网首页地址（内置默认：https://www.qcsm.ltd），需以 http(s):// 开头")
    parser.add_argument("--template", metavar="A-H", help="默认界面模板（内置默认：F），可选 A~H")
    parser.add_argument("--retry-times", metavar="0-10", type=int, help="默认自动重试次数，含首次（内置默认：3，0 视为 1）")
    parser.add_argument("--max-versions", metavar="N", type=int, help="默认保留版本目录数（内置默认：2），至少为 1")
    parser.add_argument("--service", metavar="服务名", action="append", default=[], help="默认更新前卸载的 Windows 服务名（内置默认：QsmServer），可重复传")
    parser.add_argument("--codesign-pfx", metavar="路径", help="Authenticode 证书（.pfx）路径；不传则完全不签名")
    parser.add_argument("--codesign-tsa", metavar="地址", help="RFC3161 时间戳服务地址，建议始终提供")
    parser.add_argument("--codesign-pass-env", metavar="变量名", help=f"存放 pfx 口令的环境变量名（默认 {DEFAULT_SIGN_PASSWORD_ENV}）")
    parser.add_argument("--signtool", metavar="路径", help="signtool.exe 路径；默认取 PATH 或 Windows SDK")
    return parser


def main() -> int:
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, OSError):
        pass

    args = make_parser().parse_args()

    if args.show_paths:
        for target in TARGETS.values():
            where = "workspace 成员 → 根 target/" if target.workspace_member else "独立 crate → crate 内 target/"
            print(f"{target.name}: {target.artifact_dir}（{where}）")
        return 0

    try:
        version = read_package_version()
        segments = version_segments(version)
        csv_version, dotted_version = parse_version(version)

        if args.check_version:
            return check_versions(version, segments)

        if args.sync_version:
            print(f"读取 package.json 版本号：{version}")
            for note in sync_all_version_sources(version, segments):
                print(f"      {note}")
            print("版本号已铺开（未执行构建）")
            return 0

        if args.target is None:
            print("缺少打包目标：egui / pkgtool（或使用 --check-version / --sync-version / --show-paths）", file=sys.stderr)
            return 2

        target = TARGETS[args.target]
        build_env = collect_build_env(args)
        if build_env:
            print("构建参数覆盖（未列项使用内置默认）：")
            for key, value in build_env.items():
                print(f"      {key} = {value}")
        else:
            print("未传入构建参数，使用内置默认产品线配置（qsm）")

        sign = codesign_options(args)
        if sign:
            print(f"签名目标：{Path(sign['pfx'])}（时间戳 {sign['tsa'] or '未设置'}）")

        print(f"读取 package.json 版本号：{PACKAGE_JSON}")
        print(f"      版本号：{version} -> Cargo {cargo_version_of(segments)} / rc {csv_version} / 字符串 {dotted_version}")

        build_target(
            target,
            csv_version,
            dotted_version,
            build_env,
            version=version,
            segments=segments,
            sign=sign,
        )
        return 0
    except BuildError as exc:
        print(f"打包失败：{exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("打包已取消", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
