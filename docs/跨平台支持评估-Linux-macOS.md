# 跨平台支持评估（Linux / macOS）与启动性能实测

> 状态：**暂缓（2026-10-31）**。本文档不包含任何实现改动，仅保存评估结论、实测数据与恢复工作时的实施清单。
> 决策：**暂时放弃 Linux / macOS 支持，继续只交付 Windows 10/11 x64**。
> 恢复条件：明确目标平台与架构、macOS 签名/公证证书、分平台更新包与安装位置方案。

---

## 0. 结论速览

1. **Windows 10/11 x64 是当前唯一可用平台**。
2. **Linux 在当前配置下连编译都过不去**：`eframe` 关闭了 `x11`/`wayland`，winit 直接 `compile_error!`。不是"能跑但没测"。
3. **macOS 具备编译与启动条件**（winit 的 macOS 后端不需要额外 feature，wgpu 有 Metal），但**不具备可交付性**：
   - 自动定位主程序只认 `.exe`，更新装完拉不起目标；
   - 没有 `.app` 打包，也没有签名/公证，Gatekeeper 会拦；
   - 单实例锁、磁盘预检、服务/提权等功能降级或失效。
4. **性能上最值得注意的一点**：无更新"静默退出"并不免费。Win10 实测 **767 ms / 峰值 187 MB**，其中约 **600 ms** 花在隐藏窗口的 eframe/wgpu/字体初始化上；纯检测 `--check-only` 只要 **169 ms**。恢复跨平台工作或优化启动时，优先考虑"把 UI 初始化推迟到确定要显示窗口之后"。
5. 工作量量级：Linux 基本可用 2–3 天；macOS 可交付 3–5 天（不含证书流程）；再算上打包、CI、分平台发布链路，整体 1–2 周量级。

---

## 1. 平台边界与代码证据

### 1.1 支持矩阵

| 平台 | 构建 | 运行 | 结论 |
| --- | --- | --- | --- |
| Windows 10/11 x64 | ✅ | ✅ | 唯一交付目标 |
| Linux | ❌ 当前 feature 下编译失败 | — | 恢复工作需先补窗口后端 |
| macOS | ✅ 具备条件 | ⚠️ 可开窗，更新后无法自动拉起 | 不是可交付产品 |
| Windows 10 ARM64 | — | ❌ | Win10 ARM 只仿真 x86，64 位 exe 不能运行 |
| Windows 11 ARM64 | — | ⚠️ 未验证 | x64 仿真可跑，有仿真开销 |
| Windows 8.1 / 7 | — | ❌ | wgpu 无 DX12 后端，且没有 GL 回退 |
| Server 2016+ 桌面体验 | — | ⚠️ 可用 | Server Core 无桌面，不适用 |

### 1.2 Linux 编译失败的直接原因

`src-egui/Cargo.toml:22`：

```toml
eframe = { version = "0.37", default-features = false, features = ["wgpu"] }
```

eframe 0.37 的默认 feature 包含 `x11`、`wayland`、`winit/default`（见本机 cargo registry 中的 `eframe-0.37.0/Cargo.toml`），本项目为了减体积全部关闭。winit 0.30.13 在 Linux 上必须在 X11/Wayland 中至少启用一个，否则触发：

```rust
// winit-0.30.13/src/platform_impl/mod.rs:78
compile_error!("The platform you're compiling for is not supported by winit");
```

2026-10-31 用 `cargo tree -e features -i winit --target x86_64-unknown-linux-gnu` 验证，Linux 目标下 winit 只有 `rwh_06`，没有 `x11` / `wayland`。

另外 `src-core/Cargo.toml` 的 `reqwest` 使用 `default-tls`，Linux 构建需要 OpenSSL 开发包，运行需要系统 OpenSSL；打包时需一并处理（或评估切换 rustls）。

> README §2（约 68 行）写的"Linux/macOS 可编译但跳过资源嵌入"是旧说法，与当前 feature 裁剪不符。恢复跨平台工作时需一并修正。

### 1.3 macOS 的功能缺口

| 缺口 | 位置 | 影响 |
| --- | --- | --- |
| 主程序识别只认 `.exe` | `src-core/src/install.rs:119-124` | 更新安装完，`--launch` 留空时找不到目标，报"未找到主程序" |
| 无 `.app` 打包/签名/公证 | 发布链路（当前无） | Gatekeeper 拦截；裸 Mach-O 不能被 Finder 正常当作应用 |
| Unix 执行权限依赖 zip 条目 | `src-core/src/install.rs:815/860` 会恢复 `unix_mode`，但 Windows 打的 zip 通常不带 | 解压出的主程序可能不可执行，需发布包写入 `0755` |
| 单实例锁退化 | `src-core/src/instance.rs:89-110`，非 Windows 退化为普通可写打开 | 两个更新器可同时写同一目录 |
| 磁盘空间预检跳过 | `src-core/src/disk.rs:74/109`，非 Windows 返回 `None` | 空间不足只能在写盘时失败 |
| `services` 仍调用 `sc.exe` | `src-core/src/services.rs:136-143` | 不配置服务不会触发；配置了在 Unix 上必然失败 |
| 提权是空实现 | `src-core/src/elevation.rs:79/219` | 安装到系统目录时没有 sudo/授权路径 |

已具备的跨平台基础：

- 目录解析有 macOS / Linux 分支（`src-core/src/appdir.rs:154-170`）：macOS 用 `~/Library/Application Support`，Linux 用 `$XDG_CONFIG_HOME` 或 `~/.config`。
- 打开浏览器有平台分支（`src-core/src/shell.rs:107/115/119`）：`start` / `open` / `xdg-open`。
- 字体内嵌，且有 Windows/macOS/Linux 系统字体回退（`src-ui/src/fonts.rs`）。
- 解压时保留 Unix 权限位（`src-core/src/install.rs:815/860`）。
- `build.rs` 仅在 Windows 嵌入 rc 资源（`src-egui/build.rs:14`）。
- `.cargo/config.toml` 的 `+crt-static` 只作用于 Windows 目标，不影响其他平台。

### 1.4 Windows 专属能力（不是"缺失"，是平台语义不同）

- Windows 服务管理（`sc.exe`）与 UAC 提权（`ShellExecuteW("runas")`）。
- `%APPDATA%` / `%LOCALAPPDATA%` / `%PROGRAMDATA%` 目录降级规则。
- `share_mode(0)` 单实例锁。
- `GetDiskFreeSpaceExW` 磁盘预检。
- rc 图标/版本资源。

移植时应按平台禁用或替换，不应强行"等价实现"。例如 Linux 的服务管理通常不属于启动器职责；macOS 的授权应通过安装位置设计规避 sudo。

---

## 2. 启动性能实测（2026-10-31，Windows 10）

### 2.1 测试环境

| 项 | 值 |
| --- | --- |
| 系统 | Windows 10 Enterprise LTSC 2021，10.0.19044 |
| CPU | Intel Xeon E5-2666 v3 @ 2.90 GHz |
| GPU | NVIDIA GeForce GTX 750 Ti（DX12 FL11_0）；另有 OrayIddDriver 远程显示驱动 |
| 产物 | `dist/1.0.9/launcherclient.exe`，15,693,312 字节 = **14.97 MiB** |
| 构建 | eframe/egui 0.37，wgpu 30.0.1，release，内嵌三套 HarmonyOS Sans SC（约 5.1 MB），静态 CRT |
| 网络 | 本机 mock manifest（127.0.0.1:8799），设置 `NO_PROXY` |

### 2.2 结果

| 场景 | 轮次 | 中位/结果 |
| --- | --- | --- |
| 无更新静默全程（隐藏建窗 → 检查 → 拉起空目标 → 退出） | 3 | **767 ms**（1074 / 763 / 767；首轮冷启 +307 ms） |
| 错误窗首次可见（manifest 连接失败） | 3 | **710 ms**（710 / 713 / 692） |
| 纯检测 `--check-only`（不建窗） | 5 | **169 ms**（165 / 171 / 169 / 170 / 168，退出码 0） |

内存（工作集）：

| 场景 | 峰值 | 显示后约 0.8 s |
| --- | --- | --- |
| 无更新静默 | **187.6 / 188.1 / 186.5 MB** | 进程已退出 |
| 错误窗 | **188.2 / 187.0 / 187.3 MB** | **136.5 / 136.8 / 136.4 MB** |

### 2.3 解读

- **静默路径 ≈ `--check-only` + 约 600 ms UI 初始化**。代码上 `main.rs:177` 无条件调用 `run_ui`，即使随后判定无更新、窗口保持隐藏，也会完整创建 winit 窗口、wgpu 适配器/设备/管线、解析 5.1 MB 字体并渲染至少一帧。
- **峰值 187 MB 主要来自 wgpu 初始化**，与更新包大小无关；显示后约 0.8 s 回落到约 137 MB（更长静置未测，不能称为最终稳态）。
- 首轮比后续多约 300 ms，更像 Defender 扫描/驱动着色器首次编译，而非业务代码差异。
- 绘制是"按需重绘"：动画/下载阶段 33 ms，静止态 150 ms（`src-egui/src/app.rs:328`）；界面仅 440×420，GPU 基本闲置。
- 隐藏窗口的 300 ms 懒显示阈值见 `src-egui/src/app.rs:30`；首检快于 300 ms 时不显示窗口，但 UI 初始化成本已经付出。

### 2.4 与 Win11 存档数据的对照

存档来自 [Tauri-vs-egui性能对比.md](Tauri-vs-egui性能对比.md)（Win11 x64，eframe 0.35）：

| 指标 | Win11 存档 | Win10 本次 |
| --- | --- | --- |
| 首次可见 | 776 ms（wgpu） | 710 ms |
| 无更新静默全程 | 未测 | 767 ms |
| 纯检测 | 未测 | 169 ms |
| 峰值内存 | 177 MB | 187 MB |
| 显示后 0.8 s 内存 | 81.5 MB（单次采样） | 136.5 MB |
| 更新期 CPU | 10.6%–11.2%（2.5 s 采样） | 未重测 |
| 进程数 | 1 | 1 |

**不可严格对比**：机器不同、eframe 0.35 vs 0.37、当前版内嵌字体并静态链接 CRT、启动逻辑改为"隐藏建窗 + 300 ms 懒显示"。710 vs 776 ms 只能说明同一量级，不能得出"Win10 更快"。

与 Tauri 版的历史对照（同仓库同功能实现）：Tauri 峰值内存 354–362 MB、更新期 CPU 43.3%、首次可见 835 ms，且需要 WebView2；当前 egui 单进程、187 MB 峰值、约 11% CPU、无外部运行时。

### 2.5 测量局限

- 单机、单 GPU、单分辨率（1920×1080，100% DPI）；未测 125%/150%/200% DPI 与多屏混合 DPI。
- 未重测更新期 CPU/GPU；未测 VM/WARP、ARM64、macOS、Linux。
- 机器装有远程显示驱动，可能影响呈现时间。
- 每组 3–5 轮，样本量小；首轮冷启单独列出，未混入中位数。
- 测试用的 mock 服务、临时目录、空目标 exe 与探针文件已在测试后清理。

---

## 3. 恢复工作时的实施清单

### P0：先把 Linux/macOS 做到"能构建、能更新、能拉起"

1. **Linux 窗口后端**
   - `src-egui/Cargo.toml`：`features = ["wgpu", "x11", "wayland"]`（两者都要，覆盖不同桌面会话）。
   - 补系统依赖：X11/Wayland 开发包、`libxkbcommon`、Vulkan 或 Mesa、`pkg-config`；评估 `reqwest` 是否切换 rustls 以去掉 OpenSSL 构建依赖。
   - 验收：Ubuntu 22.04/24.04 `cargo build --release` 成功，窗口可显示。
   - 注意：这些是 target 级依赖，不增加 Windows 产物。

2. **主程序定位跨平台化**
   - Linux：接受可执行位（`mode & 0o111`），而不是只认 `.exe`。
   - macOS：识别 `.app/Contents/MacOS/*`；`--launch` 指向 `.app` 目录时转成内部可执行文件，或用 `open -a`。
   - 验收：空 `--launch` 时，测试包更新后能自动拉起目标。

3. **Unix 权限与打包**
   - 发布 zip 必须为可执行文件写入 `0755`，否则 `install.rs` 恢复的 `unix_mode` 为空。
   - 验收：解压后的主程序直接可执行。

4. **修正 README §2 的"Linux/macOS 可编译"表述**，避免再次误导。

### P1：可交付性

5. **macOS `.app` + 签名/公证**
   - 生成 `.app` bundle（Info.plist、图标、Bundle 结构）；codesign + notarization。
   - 更新替换 `.app` 后，新包也必须已签名，否则下次启动被 Gatekeeper 拦。
   - 决策：用户级安装（`~/Applications`）还是系统级（`/Applications`，需授权）。
6. **单实例锁**：非 Windows 用 `flock`（或 `fs2`）实现独占；保持"崩溃由 OS 释放"的语义。
7. **磁盘预检**：Unix 用 `statvfs` 实现 `available_bytes`。
8. **分平台更新包/清单**：当前一个 manifest 对应一个 zip。可二选一：
   - 每平台单独构建，把不同的 `--manifest-url` 烘进去；
   - 扩展 manifest 支持按平台/架构下发。
9. **Linux 数据目录**：从 `~/.config/<app>` 迁到 `~/.local/share/<app>`（XDG_DATA_HOME），日志可放 `~/.local/state`。

### P2：打磨与 CI

10. `services` 明确 gate 到 Windows，Unix 上打印"不支持"而不是尝试 `sc.exe`。
11. 打包格式：Linux AppImage/deb；macOS dmg。
12. CI 矩阵：Linux 构建/单元测试、macOS 构建；至少一次真实桌面冒烟（Xvfb 只能覆盖"能起窗"，不能替代真实 GPU/桌面）。
13. 完整测试矩阵：Win10 LTSC、Win11、无独显 VM（WARP）、150% DPI、Linux X11/Wayland、macOS Intel/Apple Silicon（若可得）。

### 工作量量级

| 阶段 | Linux | macOS |
| --- | --- | --- |
| L1 能编译、能开窗 | 0.5–1 天 | 0.5 天 |
| L2 功能等价（更新/拉起/权限/日志） | 2–3 天 | 3–5 天 |
| L3 打包、签名、CI、分平台发布 | 2–4 天 | 1–2 周（含证书/公证流程） |

以上为量级估计，不含真实设备采购与证书审批时间。

---

## 4. 待定设计决策（恢复前必须先定）

1. **目标平台与架构**：只 Linux？只 macOS？是否要求 ARM64？
2. **安装模型**：用户级安装还是系统级安装？后者在 macOS 需要授权、在 Linux 需要 root，会改变更新器定位。
3. **macOS 签名**：是否有 Apple Developer 证书；没有证书只能本地/自用，不能公开分发。
4. **更新包组织**：每平台独立构建 + 独立 manifest，还是扩展 manifest 的平台字段。
5. **TLS 后端**：继续 `default-tls`（Linux 依赖 OpenSSL），还是切换 rustls（更可移植，但证书库/代理行为需重测）。
6. **Unix 服务语义**：启动器是否负责 systemd/launchd？建议不负责，明确边界。
7. **Windows 专属能力**：UAC、服务、`%PROGRAMDATA%` 降级在 Unix 上直接禁用，还是提供等价交互。

---

## 5. 复现方法

### 5.1 纯检测

```bash
export NO_PROXY="127.0.0.1,localhost"
launcherclient.exe --app-name perfprobe --current-version 1.0.9 \
  --manifest http://127.0.0.1:8799/version.json --check-only
```

### 5.2 无更新静默路径

1. 起一个本地静态服务，提供 `version.json`（版本与 `--current-version` 相同）。
2. 准备一个立即退出的空目标 exe（`fn main() {}` 编译产物）。
3. 启动：

```bash
launcherclient.exe --app-name perfprobe --current-version 1.0.9 \
  --manifest http://127.0.0.1:8799/version.json --launch D:/tmp/perfprobe/dummy.exe
```

4. 从 `CreateProcess` 到进程退出计时，轮询 `GetProcessMemoryInfo` 取峰值工作集。

### 5.3 错误窗首次可见

把 `--manifest` 指向不可达地址（如 `http://127.0.0.1:9/version.json`），用 `EnumWindows` + `GetWindowThreadProcessId` + `IsWindowVisible` 轮询首个可见窗口，记录时间与内存，随后结束进程。

### 5.4 测量脚本（存档）

测试时使用的 Python 脚本（ctypes + Win32 API，无第三方依赖）核心逻辑：

```python
import ctypes, ctypes.wintypes as w, os, subprocess, time

EXE = r"dist\1.0.9\launcherclient.exe"
kernel32 = ctypes.WinDLL("kernel32")
user32 = ctypes.WinDLL("user32")
psapi = ctypes.WinDLL("psapi")

class PMC(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("PageFaultCount", w.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t)]

def peak_mem(pid):
    h = kernel32.OpenProcess(0x0410, False, pid)
    if not h: return None
    try:
        p = PMC(); p.cb = ctypes.sizeof(PMC)
        if psapi.GetProcessMemoryInfo(h, ctypes.byref(p), p.cb):
            return p.PeakWorkingSetSize, p.WorkingSetSize
    finally:
        kernel32.CloseHandle(h)

def measure_silent(rounds=3):
    env = dict(os.environ, NO_PROXY="127.0.0.1,localhost")
    for _ in range(rounds):
        t0 = time.perf_counter()
        p = subprocess.Popen([EXE, "--app-name", "perfprobe", "--current-version", "1.0.9",
                              "--manifest", "http://127.0.0.1:8799/version.json",
                              "--launch", "dummy.exe"], env=env)
        peak = 0
        while p.poll() is None:
            m = peak_mem(p.pid)
            if m: peak = max(peak, m[0])
            time.sleep(0.01)
        print((time.perf_counter() - t0) * 1000, peak / 1048576, "MB")

WNDENUMPROC = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

def first_visible_title(pid):
    found = []
    @WNDENUMPROC
    def cb(hwnd, _):
        wp = w.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wp))
        if wp.value == pid and user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            if n:
                buf = ctypes.create_unicode_buffer(n + 1)
                user32.GetWindowTextW(hwnd, buf, n + 1)
                found.append(buf.value); return False
        return True
    user32.EnumWindows(cb, 0)
    return found[0] if found else None
```

完整脚本还包含：窗口可见轮询、显示后 0.8 s 稳态采样、3–5 轮中位数统计、进程清理。恢复工作时可按上述逻辑重建。

---

## 6. 参考

- [README §2 环境要求](../README.md)
- [Tauri 与 egui 性能对比](Tauri-vs-egui性能对比.md)
- [启动器外壳选型决策记录](选型决策记录.md)
- 关键代码：
  - `src-egui/Cargo.toml`（eframe feature 裁剪）
  - `src-egui/src/main.rs`（启动决策与无条件 `run_ui`）
  - `src-egui/src/app.rs`（300 ms 懒显示、33/150 ms 重绘）
  - `src-core/src/install.rs`（`.exe` 定位、Unix 权限位、交换安装）
  - `src-core/src/instance.rs` / `disk.rs` / `services.rs` / `elevation.rs`（Windows 专属能力）
  - `src-core/src/appdir.rs`（平台目录解析）
  - `src-ui/src/fonts.rs`（内嵌字体与系统回退）
