"""apply 断点（v2）：``{dataHome}/checkpoints/apply.jsonl``。

为什么是 **decision 级**粒度：decision 是 UI 一次回执的单位（每个
``applied`` 事件对应一个 decision），断点到这个粒度就够"重跑跳过已完成的"；
文件级细粒度的事件流（progress）已经有了，不需要再存一份。

**它补的真正缺口是 move/copy**：纯写标签的重跑本来就路径幂等
（PathQuery upsert 收敛，A3.5 的"重跑补齐"）；但 move/copy 中断后，
已移走的文件不在 decisions 给的源路径上，``Item.from_path`` 必然报错——
重跑反而失败。断点让续跑跳过已完成的 decision，只补剩余的。

键 = sha256(sorted(源路径) + candidate 规范 JSON)：UI 续跑重发同一批
decisions 时键稳定；用户改了决策（内容变了）键自然失效、照常重做——
正确比省事重要。**只在 decision 零失败时记账**：有 conflict 的 decision
重跑还要补，不能标记完成。

容错：追加式 jsonl，崩溃时最后一行可能撕裂——读取跳过解析失败的行。
修剪：load 时顺手清掉 30 天前的行（临时文件 + 原子替换）。dataHome
不可写 → 全部 fail-open（丢断点不丢功能）。
"""

from __future__ import annotations

import hashlib
import json
import os
import time

from . import cfg

_KEEP_DAYS = 30
_pruned = False  # 每进程只修剪一次


def _path():
    return cfg.data_home() / "checkpoints" / "apply.jsonl"


def decision_key(paths: list[str], candidate: dict) -> str:
    """一个 decision 的内容指纹（稳定、跨进程一致）。"""
    body = {"paths": sorted(str(p) for p in paths or []), "candidate": candidate or {}}
    return hashlib.sha256(
        json.dumps(body, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()


def load() -> set[str]:
    """已完成 decision 的键集合。文件问题一律当空集。"""
    global _pruned
    try:
        with open(_path(), encoding="utf-8") as f:
            lines = f.readlines()
    except OSError:
        return set()
    keys: set[str] = set()
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue  # 崩溃撕裂的尾行
        if isinstance(rec, dict) and isinstance(rec.get("key"), str):
            keys.add(rec["key"])
    if not _pruned:
        _pruned = True
        _prune(lines)
    return keys


def _prune(lines: list[str]) -> None:
    cutoff = time.time() - _KEEP_DAYS * 86400
    kept = []
    for line in lines:
        try:
            rec = json.loads(line)
        except (json.JSONDecodeError, ValueError):
            continue
        if isinstance(rec, dict) and isinstance(rec.get("ts"), (int, float)) and rec["ts"] >= cutoff:
            kept.append(line.rstrip("\n"))
    if len(kept) == len(lines):
        return
    path = _path()
    try:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        tmp = str(path) + ".tmp"
        with open(tmp, "w", encoding="utf-8", newline="\n") as f:
            for line in kept:
                f.write(line + "\n")
        os.replace(tmp, path)
    except OSError:
        pass


def record(key: str, album_id) -> None:
    """标记一个 decision 完成（零失败才调）。失败不致命——丢断点 = 重跑多写一遍。"""
    try:
        path = _path()
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "a", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(
                {"key": key, "albumId": album_id, "ts": int(time.time())},
                ensure_ascii=False,
            ) + "\n")
    except OSError:
        pass
