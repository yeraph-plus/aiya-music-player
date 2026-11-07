# Web 版镜像（docs/65 W5 + docs/67 beets Linux 轨集成 + docs/68 v2）：
# node 构建前端 → cargo 构建 server → beets 套件包构建（PBS + 提取器 + 模型）
# → slim 运行时。liquidsoap **不在本镜像**（v2 双容器：官方 savonet 镜像
# 外置 + 配置卷 worker——体积账见 docs/68 §8）。网络环境适配：npm/crates/apt
# 走国内镜像。
#
# beets 套件（docs/67 转正）：构建阶段跑 `build_bundle.py --platform linux`
# 产出 `beets-portable-linux-x64.zip`；运行时层只带 zip 不展开——容器首启后
# 在 UI 的 Beets 页点一次安装（`AIYA_BEETS_BUNDLE` 指向包内路径，走与桌面
# 相同的 install_via_bundle 代码路径，套件展开进 /data 卷，重启不丢）。
# 提取器二进制来自 build-essentia-linux workflow 的 artifact（docs/67，
# sha256 见 docker/beets/SHA256SUMS），放在 docker/beets/ 随构建上下文进来。

# ---- 前端 ----
FROM node:24-slim AS ui
WORKDIR /app
RUN npm config set registry https://registry.npmmirror.com
COPY ui/package.json ui/package-lock.json ./ui/
RUN npm ci --prefix ui
COPY ui/ ui/
RUN npm --prefix ui run build

# ---- 服务端 ----
FROM rust:1-bookworm AS rust
WORKDIR /app
# crates 源走 rsproxy（sparse）；构建缓存挂 BuildKit cache（重建增量）
RUN mkdir -p /usr/local/cargo && printf '[source.crates-io]\nreplace-with = "rsproxy"\n\n[source.rsproxy]\nregistry = "sparse+https://rsproxy.cn/index/"\n\n[net]\ngit-fetch-with-cli = true\n' > /usr/local/cargo/config.toml
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src-tauri/Cargo.toml src-tauri/Cargo.toml
# workspace 成员清单解析要求 [lib]/[bin] 的路径文件存在（不编译该成员）
COPY src-tauri/src src-tauri/src
COPY beets-tool/src/aiya_beets/data/ beets-tool/src/aiya_beets/data/
COPY protocol/ protocol/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release -p aiya-server

# ---- beets 套件包（docs/67 转正）----
# 脚手架是 bookworm 的系统 python3（只跑打包脚本）；套件内的解释器由
# build_bundle.py 从预置的 python-build-standalone tar.gz 解开（3.14.10，
# 与 win 包同版本，sha 钉扎）。三个预置件全走构建上下文：docker build 离线
# （模型 zip / PBS / 提取器的来源与校验见 docker/beets/ 下文件与 docs/67）。
FROM debian:bookworm-slim AS beets-bundle
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' \
      /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list 2>/dev/null || true \
 && apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-pip \
 && rm -rf /var/lib/apt/lists/* \
 && pip config set global.index-url https://mirrors.aliyun.com/pypi/simple/
# uv：依赖解析与安装器（pypi 镜像；构建脚手架用，不进发布物）
RUN pip install --break-system-packages --no-cache-dir uv
ENV UV_DEFAULT_INDEX=https://mirrors.aliyun.com/pypi/simple/
COPY beets-tool /src/beets-tool
COPY docker/beets/ /build/
# 产物自检在 build_bundle.py 内部（包内解释器 version/pip + bin/ 条目在位 +
# manifest 校验），失败即构建失败
RUN cd /src/beets-tool \
 && python3 tools/build_bundle.py --platform linux \
      --extractor /build/streaming_extractor_music \
      --pbs-tarball /build/pbs.tar.gz \
      --models-zip /build/svm-models.zip \
      --out /out/beets-portable-linux-x64.zip

# ---- 运行时 ----
FROM debian:bookworm-slim AS runtime
# libqt5core5a / libqt5concurrent5 / libdrm2：提取器的动态依赖（docs/67 ldd
# 实测；Qt5 同为 5.15 ABI 兼容；libdrm 是"待宿主实测"项——实测必须显式装）。
# liquidsoap 不在本镜像（docs/68 v2 双容器：官方 savonet 镜像外置 +
# 配置卷 worker——apt 全家依赖 ~450MB 摘除，体积账见 docs/68 §8）。
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' \
      /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list 2>/dev/null || true \
 && apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
      libqt5core5a libqt5concurrent5 libdrm2 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=rust /app/target/release/aiya-server /app/aiya-server
COPY --from=ui /app/ui/dist /app/ui
COPY --from=beets-bundle /out/beets-portable-linux-x64.zip /opt/beets-portable-linux-x64.zip
ENV AIYA_BIND=0.0.0.0 \
    AIYA_PORT=8787 \
    AIYA_DATA_ROOT=/data \
    AIYA_STATIC_DIR=/app/ui \
    AIYA_BEETS_BUNDLE=/opt/beets-portable-linux-x64.zip
VOLUME ["/data", "/music"]
EXPOSE 8787
# start-period 放宽：首启种子扫描在 bind 之前同步跑（大曲库首启监听晚于容器起）
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s \
  CMD curl -fsS "http://127.0.0.1:${AIYA_PORT}/" >/dev/null || exit 1
ENTRYPOINT ["/app/aiya-server"]
