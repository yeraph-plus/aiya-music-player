"""外部二进制：下载、校验、落到 `bin/`、以及把 `bin/` 塞进 PATH。

## 为什么要有这个模块（docs/53 §1.1 实测）

默认插件清单里曾有 `replaygain`，它**写死**找 `ffmpeg`（`beetsplug/replaygain.py:289`，
没有配置键）。而 PATH 上没有 ffmpeg 时，beets 不是"少了归一化"，是**任何子命令都起不来**：

    beets.exceptions.UserError: replaygain initialization failed:
    could not find ffmpeg at ffmpeg

同一批命令把 ffmpeg 放进 PATH 就正常，`rg_track_gain` 落到 -2.6 / -2.4 / -3.1
（3 个 wav 实测）。所以"外部二进制"从"提示用户自备"升级成"装的时候就带上"。

## 三个二进制各怎么拿（都是实测过 200 的地址）

    ffmpeg                       PyPI imageio-ffmpeg 0.7.0 轮子（轮子本身就是 zip）
    streaming_extractor_music    data.metabrainz.org 直链 zip（AcousticBrainz 停服的留存）
    fpcalc                       GitHub acoustid/chromaprint v1.6.2 zip

为什么不"挂 pip 装"而是把轮子当信封：我们要的是一个**固定路径的 exe**
（PATH 注入、状态显示、卸载清理都靠它），而 `pip install imageio-ffmpeg` 装进
venv 只会多一个 beets 不认识的包，里面的 exe 名字还带版本。轮子在这里只是
"一个能放二进制的地方"，和 zip 没有区别 —— 所以 `kind` 只有 `zip` 一种。

## 为什么不 `curl`

本机实测：`curl https://pypi.org/…` → 30s 超时（走代理挂住），同一时刻
`urllib` 与 `pip` 都正常。而且 Windows 上 `curl.exe` 要 1803 才有。
理由由解释器自己发请求：尊重 `HTTP(S)_PROXY`、能设超时与上限、不依赖外部程序。

## 校验三件套（缺一不可）

1. 载体：下载完立刻算 `size` + `sha256`（不等解压）；
2. 成员：载体内的 glob **必须恰好命中 1 个**（0 个 = 载体换了；2 个 = 里面塞了
   第二个 exe，随便挑一个是运气）；
3. 可用：落盘后跑一次 `probe`（`ffmpeg -version` / `fpcalc -version`）；
   essentia 那个 exe 无参调用退出码非 0 但打 `Usage:`，用 `probeAllowFail` 表达。
   32 位 exe 靠 WoW64，精简系统会缺 —— 下完能跑才算装上。
"""

from __future__ import annotations

import fnmatch
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

from . import plugins
from .plugins import BINARIES, BinarySpec

# 单个下载物的上限。最大的那个是 ffmpeg 的轮子 31.2MB，留 8 倍余量。
# 为什么要有：`.read()` 一直读到磁盘满；而地址写错（指到大文件）的代价由用户的
# 磁盘承担（与宿主侧 `download_to` 的 ASSET_MAX_BYTES 同一个理由）。
MAX_BYTES = 256 * 1024 * 1024

TIMEOUT = 120
USER_AGENT = "aiya-beets/1.0 (+https://github.com/yeraph-plus/aiya-music-player)"

# Windows 上这些 exe 都是控制台程序，而本进程可能没有控制台（宿主用
# CREATE_NO_WINDOW 拉起）。该 flag 不传给子进程，每个 spawn 都要单独补。
_NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}


class BinError(Exception):
    """下载/校验/落盘失败。消息直接给用户看，必须说清是哪一步。"""


# ---------------------------------------------------------------- 位置


def bin_dir(home: Path | None = None) -> Path | None:
    """`{data_root}/beets-tool/bin`。

    数据根从 `AIYA_BEETS_HOME` 推（它的上一级就是 `{data_root}`，两个家都在
    同一层：`beets/`、`beets-index/`、`beets-tool/`）。刻意**不**用
    `cfg.data_home()`：那条路在没有环境变量时会 `import beets` 去问默认配置目录，
    而本函数要在 `import beets` 之前就能回答。

    `AIYA_BEETS_BIN_DIR` 可以覆盖（离线演练、测试、以及"我有自己的 ffmpeg"）。
    """
    override = (os.environ.get("AIYA_BEETS_BIN_DIR") or "").strip()
    if override:
        return Path(override).expanduser()
    for key in ("AIYA_BEETS_HOME", "AIYA_BEETS_DIR"):
        raw = (os.environ.get(key) or "").strip()
        if raw:
            return Path(raw).expanduser().parent / "beets-tool" / "bin"
    return None


def inject_path() -> Path | None:
    """把 `bin/` 前插到 `PATH`，返回插进去的目录（没插返回 None）。

    **必须在 `import beets` 之前调用**：replaygain / convert 都直接用字符串
    `"ffmpeg"` 去 spawn，只有 PATH 一条路。放在 `cli.main` 里而不是宿主里，
    是为了让 `python -m aiya_beets …` 这条路也自动生效（只有一个入口）。

    前插而不是追加：系统里另有一个老版本 ffmpeg（比如带 libavfilter < 6.67 的
    4.x）时，`replaygain` 会因为它不达标而整库报错——我们下发的那个是验过的。
    """
    local = bin_dir()
    if local is None or not local.is_dir():
        return None
    current = os.environ.get("PATH", "")
    parts = current.split(os.pathsep) if current else []
    if any(os.path.normcase(p) == os.path.normcase(str(local)) for p in parts):
        return local
    os.environ["PATH"] = str(local) + os.pathsep + current if current else str(local)
    return local


def installed_path(spec: BinarySpec) -> Path | None:
    """这个二进制现在在不在 `bin/` 里。"""
    local = bin_dir()
    if local is None:
        return None
    candidate = local / spec.target
    return candidate if candidate.is_file() else None


# ---------------------------------------------------------------- 体检


def check(names: list[str] | None = None) -> list[dict]:
    """逐个报状态：在不在、在哪、多大、版本号（只在 bin/ 有才跑 probe）。"""
    wanted = names or [spec.name for spec in BINARIES]
    rows: list[dict] = []
    for name in wanted:
        spec = plugins.binary(name)
        if spec is None:
            raise KeyError(name)
        path = installed_path(spec)
        row = {
            "name": spec.name,
            "target": spec.target,
            "downloadBytes": spec.carrier_size,
            "bytes": spec.size,
            "license": spec.license,
            "present": path is not None,
            "path": str(path) if path else None,
            "version": None,
        }
        if path is not None:
            ok, version = probe(path, spec)
            row["version"] = version
            row["usable"] = ok
        else:
            # 不在 bin/ 也可能在 PATH 上（用户自己装的）。报"能找到"，
            # 但不写 version：那不是我们下发的东西，版本不保证。
            row["path"] = plugins.exe_path(spec.name)
            row["usable"] = row["path"] is not None
        rows.append(row)
    return rows


def probe(exe: Path, spec: BinarySpec) -> tuple[bool, str | None]:
    """跑一次 `probe` 参数，返回 (能不能跑, 版本行)。"""
    try:
        got = subprocess.run(
            [str(exe), *spec.probe],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
            **_NO_WINDOW,
        )
    except (OSError, subprocess.SubprocessError):
        return False, None
    text = (got.stdout or "") + (got.stderr or "")
    line = text.strip().splitlines()[0].strip() if text.strip() else None
    if got.returncode == 0:
        return True, line
    # 允许失败的只有"无参调用会报用法"这一类（essentia）。但**必须**看到
    # 它能打印点什么：0 字节输出 + 非 0 退出 = 缺 DLL 或被拦，不能算能用。
    # 版本行给 None 而不是把 "Error: wrong number of arguments" 当版本号显示 ——
    # 那是用法提示，不是版本。
    if spec.probe_allow_fail and line and ("Usage" in text or "usage" in text):
        return True, None
    return False, line


# ---------------------------------------------------------------- 下载与落盘


def _human(n: int) -> str:
    return f"{n / 1024 / 1024:.1f} MB"


def _download(url: str, dest: Path, expect_size: int) -> None:
    """下到 `dest`。先看 `content-length`，再用 `take` 语义兜一层。"""
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            declared = resp.headers.get("content-length")
            if declared and declared.isdigit() and int(declared) > MAX_BYTES:
                raise BinError(
                    f"下载物声明 {_human(int(declared))}，超过上限 —— 地址指错了？{url}"
                )
            written = 0
            with open(dest, "wb") as out:
                while True:
                    chunk = resp.read(1 << 16)
                    if not chunk:
                        break
                    written += len(chunk)
                    if written > MAX_BYTES:
                        raise BinError(f"下载物超过 {_human(MAX_BYTES)} 上限，已中止：{url}")
                    out.write(chunk)
    except urllib.error.HTTPError as e:
        raise BinError(f"下载失败 HTTP {e.code}：{url}") from e
    except urllib.error.URLError as e:
        raise BinError(f"下载失败（网络不可达）：{url} —— {e.reason}") from e
    except TimeoutError as e:
        raise BinError(f"下载超时（{TIMEOUT}s）：{url}") from e
    if expect_size and dest.stat().st_size != expect_size:
        raise BinError(
            f"下载物大小不对：期望 {expect_size} 字节，实际 {dest.stat().st_size}"
        )


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def extract_member(carrier: Path, pattern: str) -> bytes:
    """从 zip 里按 glob 抽一个成员。**必须恰好命中 1 个**。

    0 个 = 载体换了（轮子内部文件名带版本，升级就变）；
    2 个 = 里面还有第二个同名 exe，随便挑一个是在赌。
    两种都要当场报错，而不是"尽量找找"。
    """
    try:
        with zipfile.ZipFile(carrier) as zf:
            names = [n for n in zf.namelist() if fnmatch.fnmatch(n, pattern)]
            # 目录项不算（有些包的目录名恰好也匹配 glob）
            names = [n for n in names if not n.endswith("/")]
            if len(names) != 1:
                raise BinError(
                    f"载体里匹配 {pattern!r} 的成员有 {len(names)} 个"
                    f"（必须恰好 1 个）：{', '.join(names[:5]) or '无'}"
                )
            return zf.read(names[0])
    except zipfile.BadZipFile as e:
        raise BinError(f"载体不是有效 zip：{carrier}") from e


def install_one(
    name: str,
    carrier: Path | None = None,
    on_progress=None,
) -> dict:
    """把一个二进制落到 `bin/`。`carrier` 给了就不联网（本地包/镜像）。"""
    spec = plugins.binary(name)
    if spec is None:
        raise KeyError(name)
    local = bin_dir()
    if local is None:
        raise BinError(
            "不知道装到哪：AIYA_BEETS_HOME / AIYA_BEETS_DIR / AIYA_BEETS_BIN_DIR 都没设"
        )
    local.mkdir(parents=True, exist_ok=True)
    say = on_progress or (lambda _m: None)

    tmp = Path(tempfile.mkdtemp(prefix=f"aiya-bin-{name}-"))
    try:
        if carrier is not None:
            src = carrier
            if not src.is_file():
                raise BinError(f"本地包不存在：{src}")
            say(f"{name}：用本地包 {src.name}")
        else:
            src = tmp / "carrier.zip"
            say(f"{name}：下载 {_human(spec.carrier_size)}")
            _download(spec.url, src, spec.carrier_size)

        got = src.stat().st_size
        if spec.carrier_size and got != spec.carrier_size:
            raise BinError(f"{name}：载体大小 {got} ≠ 期望 {spec.carrier_size}")
        digest = _sha256(src)
        if spec.carrier_sha256 and digest != spec.carrier_sha256:
            raise BinError(
                f"{name}：载体 sha256 不对（{digest[:12]}… ≠ {spec.carrier_sha256[:12]}…）"
            )

        blob = extract_member(src, spec.member)
        if spec.size and len(blob) != spec.size:
            raise BinError(f"{name}：解出的大小 {len(blob)} ≠ 期望 {spec.size}")
        inner = hashlib.sha256(blob).hexdigest()
        if spec.sha256 and inner != spec.sha256:
            raise BinError(f"{name}：解出的 sha256 不对（{inner[:12]}… ≠ {spec.sha256[:12]}…）")

        # 先写 .part 再 replace：中途失败不会留下一个"看着在、其实半截"的 exe
        staged = local / (spec.target + ".part")
        staged.write_bytes(blob)
        final = local / spec.target
        os.replace(staged, final)
        try:
            final.chmod(0o755)
        except OSError:
            pass

        ok, version = probe(final, spec)
        if not ok:
            final.unlink(missing_ok=True)
            raise BinError(f"{name}：落盘后跑不起来（{version or '无输出'}）")
        say(f"{name}：就绪（{version or spec.target}）")
        return {"name": name, "path": str(final), "version": version, "bytes": len(blob)}
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def ensure(names: list[str], on_progress=None, carriers: dict[str, Path] | None = None) -> list[dict]:
    """把这批二进制补齐（已有的跳过）。返回每个的实际结果。"""
    say = on_progress or (lambda _m: None)
    done: list[dict] = []
    for name in names:
        spec = plugins.binary(name)
        if spec is None:
            raise KeyError(name)
        present = installed_path(spec)
        if present is not None:
            ok, version = probe(present, spec)
            if ok:
                say(f"{name}：已就绪，跳过")
                done.append(
                    {"name": name, "path": str(present), "version": version,
                     "bytes": present.stat().st_size, "skipped": True}
                )
                continue
            # 在但跑不起来（缺 DLL / 被杀软拦 / 半截文件）→ 当没有，重下
            say(f"{name}：现有文件跑不起来，重新下载")
            present.unlink(missing_ok=True)
        done.append(install_one(name, (carriers or {}).get(name), say))
    return done


# ---------------------------------------------------------------- CLI


def main(argv: list[str] | None = None) -> int:
    import argparse

    from .runtime import force_utf8_stdio

    # stdout 被宿主拿管道接走时，Windows 会按 locale 编码打开它；消息里有中文
    # （"下载 29.8 MB"）。宿主侧虽然给了 PYTHONUTF8=1，但命令行直接跑时没人给。
    force_utf8_stdio()

    ap = argparse.ArgumentParser(
        prog="aiya-beets bins",
        description="下载/校验外部二进制（ffmpeg / Essentia 提取器 / fpcalc）到 bin/",
    )
    ap.add_argument("--only", default=None, help="只处理这些名字，逗号分隔")
    ap.add_argument("--check", action="store_true", help="只体检，不下载")
    ap.add_argument("--dry-run", action="store_true", help="只打印将下载什么")
    ap.add_argument(
        "--carrier",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="用本地包代替下载（可重复），仍是同一套校验",
    )
    args = ap.parse_args(argv)

    names = [x.strip() for x in (args.only or "").split(",") if x.strip()]
    if not names:
        names = [spec.name for spec in BINARIES]
    unknown = [n for n in names if plugins.binary(n) is None]
    if unknown:
        known = ", ".join(spec.name for spec in BINARIES)
        emit({"type": "error", "stage": "prepare",
              "message": f"不认识的外部工具 {', '.join(unknown)}（可用：{known}）"})
        return 2

    carriers: dict[str, Path] = {}
    for raw in args.carrier:
        name, _, path = raw.partition("=")
        if not path:
            emit({"type": "error", "stage": "prepare", "message": f"--carrier 要 NAME=PATH：{raw}"})
            return 2
        carriers[name.strip()] = Path(path.strip())

    if args.check:
        rows = check(names)
        emit({"type": "done", "stage": "done", "binaries": rows,
              "missing": [r["name"] for r in rows if not r["usable"]]})
        return 0

    if args.dry_run:
        plan = []
        for name in names:
            spec = plugins.binary(name)
            assert spec is not None
            plan.append({
                "name": spec.name,
                "target": spec.target,
                "url": spec.url,
                "downloadBytes": spec.carrier_size,
                "bytes": spec.size,
                "license": spec.license,
                "present": installed_path(spec) is not None,
            })
        emit({"type": "dryRun", "stage": "prepare", "binaries": plan})
        emit({"type": "done", "stage": "done", "dryRun": True, "binaries": plan})
        return 0

    try:
        rows = ensure(names, lambda m: emit({"type": "progress", "stage": "bin", "message": m}), carriers)
    except BinError as e:
        emit({"type": "error", "stage": "bin", "message": str(e)})
        return 6
    except KeyError as e:
        emit({"type": "error", "stage": "bin", "message": f"未知工具：{e}"})
        return 2
    except OSError as e:
        emit({"type": "error", "stage": "bin", "message": f"落盘失败：{e}"})
        return 6

    emit({"type": "done", "stage": "done", "binaries": rows})
    return 0


def emit(obj: dict) -> None:
    """与 install.py 同款行帧输出（宿主逐行读）。"""
    import json

    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()
