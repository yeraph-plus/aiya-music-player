# fixtures/

测试素材约定（无旧库/旧协议）：

- `media/`：小段真实音频（每种格式 5–10s），用于解码/seek/PCM 哈希；注意版权，优先自制正弦/静音。
- `generated/`：`seed` 脚本生成的 5 万首合成库与封面，不入 git。
- `beets/`：最小的 beets `library.db`/配置/示例音频，用于 sidecar 预览/应用测试；不含真实曲库。
- `golden/`：协议 JSON、PCM 哈希、查询结果快照，入 git。

禁止提交完整商业曲库或大体积二进制；旧 demo 的 `library.sqlite` 不需要。