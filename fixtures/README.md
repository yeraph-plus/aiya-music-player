# fixtures/

测试素材约定：

- `legacy/`：旧版数据库副本（`user_version=4`），用于迁移/查询兼容验收；大文件不入 git。
- `media/`：小段真实音频（每种格式 5–10s），用于解码/seek/PCM 哈希；注意版权，优先自制正弦/静音。
- `generated/`：`seed` 脚本生成的 5 万首合成库与封面，不入 git。
- `golden/`：协议 JSON、PCM 哈希、查询结果快照，入 git。

禁止提交完整商业曲库或大体积二进制。