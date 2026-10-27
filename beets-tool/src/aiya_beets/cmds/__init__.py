"""命令实现。server 只做分派，业务都在这里。

共同约定：函数直接往 stdout 写事件流，不返回响应（server 的 handle 返回 None）。
"""
