"""beets 库访问的公共部分：字段口径、库打开、文件扫描分组、配置库根。

这里的每个函数都在**复刻 beets CLI 的判定逻辑**，注释里标了对应位置——
升 beets 版本时必须照着比对（见 docs/31 的固定调用命令清单）。
"""

from __future__ import annotations

import os

# 递归扫描时收集的音频扩展名（与宿主扫描口径一致；v1 不含音轨提取类）
AUDIO_EXTENSIONS = {
    ".mp3", ".flac", ".m4a", ".aac", ".ogg", ".opus", ".wma", ".wav", ".aiff", ".alac",
}

# 当前标签 vs 候选：diff 展示字段（docs/08 §4.1）
DIFF_FIELDS = ["title", "artist", "album", "albumartist", "year", "track", "disc", "genre"]


def open_library(directory: str | None = None):
    """打开 beets 库（apply / dump / 维护共用）。

    beets 2.14 实测坑：Library(path) 的 directory 默认取**系统 Music 目录**
    （platformdirs），不再读 config 的 directory；CLI 是显式传 config 值的
    （beets/ui/__init__.py:857），sidecar 照做，否则 move/copy 会写进用户的
    真实 Music 文件夹。

    `directory` = 显式指定的目标根（批 D 的 move_items 用宿主传的扫描根）；
    None 时沿用 config 的 directory（`~/Music` 兜底）。
    """
    from beets import config as beets_config
    from beets.library import Library

    return Library(
        beets_config["library"].as_filename(),
        directory or beets_config["directory"].as_filename(),
    )


def item_abs_path(lib, item) -> str:
    """item 的**绝对**路径。幂等：已经是绝对就原样返回（只做 normpath 归一）。

    beets 把 `items.path` 存成**相对 `library.directory`**（文件在该目录内时），
    API 通常已经拼回绝对——但那是实现细节，不是契约（docs/49 §1.2 的实测就
    踩在这个细节上）。所有出口（查询 / 导入预览）都过这一道，理由不是"现在会坏"，
    而是"哪天它变了，症状是全部曲目集体变未入库，且零报错"。
    """
    raw = item.path
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", "replace")
    if os.path.isabs(raw):
        return os.path.normpath(raw)
    base = lib.directory
    if isinstance(base, bytes):
        base = base.decode("utf-8", "replace")
    return os.path.normpath(os.path.join(base, raw))


def scan_audio_files(paths: list[str]) -> list[str]:
    """展开目录为音频文件列表（递归）；直接给文件则原样收下。"""
    out: list[str] = []
    for p in paths:
        if os.path.isfile(p):
            out.append(p)
        elif os.path.isdir(p):
            for root, _dirs, files in os.walk(p):
                for name in sorted(files):
                    if os.path.splitext(name)[1].lower() in AUDIO_EXTENSIONS:
                        out.append(os.path.join(root, name))
    return out


def group_albums(files: list[str]) -> list[list[str]]:
    """v1 分组：同目录 = 同专辑（含子目录里落单的散曲各自成组）。

    不读标签做簇分组（那是 beets importer 的活，v1 不复刻）；
    同目录混多专辑时用户可以在 UI 里逐曲处理（singleton 后续补）。
    """
    by_dir: dict[str, list[str]] = {}
    for f in files:
        by_dir.setdefault(os.path.dirname(f), []).append(f)
    return [files_in_dir for _d, files_in_dir in sorted(by_dir.items())]


def configured_directory() -> str | None:
    """从 config.yaml **原文**读 `directory`，没显式配置就返回 None。

    为什么不能问 beets：`config['directory'].get()` 恒返回内置默认值
    （2.14 实测：config.yaml 不存在的库也返回 `~/Music`，来自
    `config_default.yaml`），于是"用户配过"和"用的是默认"完全无法区分。
    拿这个默认值去 move 等于把用户的曲库搬进他真实的音乐文件夹。

    只认行首无缩进的 `directory:`（缩进的属于别的 section）。
    返回值做了 `expanduser`：手写 `directory: ~/Music` 是合法 YAML，而
    `Library(path, "~/Music")` 不会展开 `~`（beets 的 `as_filename()` 才会），
    不展开就会拼出 `<cwd>/~/Music` 这种路径去搬文件。
    """
    from beets import config as beets_config

    cfg = os.path.join(str(beets_config.config_dir()), "config.yaml")
    try:
        with open(cfg, encoding="utf-8") as f:
            for line in f:
                if line[:1] in (" ", "\t", "#"):
                    continue
                if line.startswith("directory:"):
                    val = line[len("directory:"):].strip().strip("'\"")
                    return os.path.expanduser(val) if val else None
    except OSError:
        return None
    return None


def dest_conflict(item, src: str) -> str | None:
    """计算 item 的 beets 目标路径并检查冲突。

    beets 的 move_file 遇到目标已存在会**静默改名**（unique_path → name.1.ext），
    不符合"目标冲突先报错"；所以这里在 move 之前自查，冲突就跳过文件操作。
    返回冲突描述（None = 无冲突；"same" = 目标就是源文件本身，move 是 no-op）。
    """
    dest = item.destination()  # bytes，按 lib.path_formats + db.directory
    if not os.path.exists(dest):
        return None
    try:
        if os.path.samefile(dest, src):
            return "same"  # 库目录就是来源目录：无需移动
    except OSError:
        pass
    return f"目标已存在：{dest.decode('utf-8', 'replace')}"
