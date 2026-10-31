"""zipapp 的入口垫片：把 `cli.main()` 的返回值**变成退出码**。

## 为什么需要这一层（2026-10-30 实测）

`zipapp.create_archive(main="aiya_beets.cli:main")` 生成的 `__main__.py` 是：

    import aiya_beets.cli
    aiya_beets.cli.main()

返回值被丢掉 ⇒ **pyz 永远退出 0**。实测：`beets-installer.pyz bins --only nope`
打出「不认识的外部工具 nope」的错误帧，退出码却是 0；同一个调用走源码树
（`python -m aiya_beets`）退出码是 2。

这正是 docs/51 §0 记过的那类事故的翻版——"错误发生了，但退出码说没事"。
所以这里只要做一件事：`sys.exit(main())`。
"""

from __future__ import annotations

import sys


def main() -> None:
    from .cli import main as cli_main

    sys.exit(cli_main())
