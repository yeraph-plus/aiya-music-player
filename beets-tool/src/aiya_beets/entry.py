"""zipapp 的入口垫片：把 `cli.main()` 的返回值**变成退出码**。

`zipapp.create_archive(main="aiya_beets.cli:main")` 生成的 `__main__.py` 直接
调用 `main()` 并把返回值丢掉 ⇒ 退出码恒为 0——错误帧打出去了、进程却报成功。
所以这里只做一件事：`sys.exit(main())`。
"""

from __future__ import annotations

import sys


def main() -> None:
    from .cli import main as cli_main

    sys.exit(cli_main())
