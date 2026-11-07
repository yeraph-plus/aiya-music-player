#!/bin/sh
# 频道管家（docs/68 §8 v2）：对账 /hls/conf/*/station.liq + ON 位 ↔ liquidsoap 进程。
# server（aiya-server 容器）负责渲染配置与开关位；本脚本只做"让进程态贴合期望态"：
#   conf/{ch}/station.liq   渲染产物（md5 变化 = 重启该频道）
#   conf/{ch}/ON            开关位（server 开播 touch / 停播 rm）
#   conf/{ch}/.pid/.md5     本管家的运行记账（.pid 同时是 server 侧 running 判据）
# 挂载进官方 savonet 镜像以 command 覆盖运行——不构建自家 liq 镜像。
# 依赖：md5sum/cut/dirname（镜像自带 coreutils）；无 inotify，轮询足够。

CONF=${LIQ_CONF_DIR:-/hls/conf}
POLL=${LIQ_POLL_SECS:-2}

while :; do
  for m in "$CONF"/*/station.liq; do
    [ -f "$m" ] || continue
    d=$(dirname "$m")
    pidfile="$d/.pid"
    md5file="$d/.md5"
    want_on=$([ -f "$d/ON" ] && echo 1 || echo 0)
    cur=$(md5sum "$m" | cut -d' ' -f1)
    pid=$(cat "$pidfile" 2>/dev/null)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then alive=1; else alive=0; fi
    last=$(cat "$md5file" 2>/dev/null)

    if [ "$want_on" = 1 ]; then
      # 在跑且配置未变 → 不动；否则（未起/崩溃/配置更新）重启
      if [ "$alive" = 1 ] && [ "$cur" = "$last" ]; then
        continue
      fi
      [ "$alive" = 1 ] && kill "$pid" 2>/dev/null && sleep 1
      liquidsoap "$m" >/dev/null 2>&1 &
      echo $! > "$pidfile"
      echo "$cur" > "$md5file"
    elif [ "$alive" = 1 ]; then
      kill "$pid" 2>/dev/null
      rm -f "$pidfile" "$md5file"
    else
      rm -f "$pidfile" "$md5file"
    fi
  done
  sleep "$POLL"
done
