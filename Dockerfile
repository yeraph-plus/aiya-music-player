# Web 版镜像（docs/65 W5）：node 构建前端 → cargo 构建 server → slim 运行时。
# beets / liquidsoap 层按批次约定暂缺（W5 范围：播放器本体的容器可用性）。
# 网络环境适配：npm/crates/apt 都走国内镜像（docker hub 直连不可达的同一台机器上可构建）。

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

# ---- 运行时 ----
FROM debian:bookworm-slim AS runtime
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' \
      /etc/apt/sources.list.d/debian.sources /etc/apt/sources.list 2>/dev/null || true \
 && apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY --from=rust /app/target/release/aiya-server /app/aiya-server
COPY --from=ui /app/ui/dist /app/ui
ENV AIYA_BIND=0.0.0.0 \
    AIYA_PORT=8787 \
    AIYA_DATA_ROOT=/data \
    AIYA_MUSIC_DIR=/music \
    AIYA_STATIC_DIR=/app/ui
VOLUME ["/data", "/music"]
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
  CMD curl -fsS "http://127.0.0.1:${AIYA_PORT}/" >/dev/null || exit 1
ENTRYPOINT ["/app/aiya-server"]
