# 智伴 Mindmate 服务器模式（B/S 部署）
# 多阶段构建：Rust 编译 → 精简运行镜像
# 部署：docker compose up -d  （数据挂载到 ./data 卷）

# ─────────── 构建阶段 ───────────
FROM rust:1.83-slim AS builder

# Tauri 桌面依赖（服务器模式无需 WebView，但 crate 依赖需要系统库）
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY core ./core
COPY src-tauri ./src-tauri
# 服务器模式不需要前端产物嵌入（浏览器直接访问 API + 静态资源由镜像内的 dist 提供）
COPY apps/web/dist ./apps/web/dist

WORKDIR /app/src-tauri
RUN cargo build --release --bin mindmate

# ─────────── 运行阶段 ───────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libgtk-3-0 libwebkit2gtk-4.1-0 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 mindmate

WORKDIR /app
COPY --from=builder /app/src-tauri/target/release/mindmate /usr/local/bin/mindmate
COPY apps/web/dist /app/web

RUN mkdir -p /data && chown -R mindmate:mindmate /data /app
USER mindmate

ENV MINDMATE_MODE=server \
    MINDMATE_PORT=17801 \
    MINDMATE_DATA_DIR=/data \
    MINDMATE_WEB_DIR=/app/web

EXPOSE 17801
VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
  CMD ["/usr/local/bin/mindmate", "--healthcheck"]

CMD ["/usr/local/bin/mindmate", "--mode", "server", "--headless", "--data-dir", "/data"]
