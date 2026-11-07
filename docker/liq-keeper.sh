#!/bin/sh
# 频道管家（docs/68 §8 v2）：对账 /hls/conf/*/station.liq + ON 位 ↔ liquidsoap 进程。
# server（aiya-server 容器）负责渲染配置与开关位；本脚本只做"让进程态贴合期望态"：
#   conf/{ch}/station.liq   渲染产物（md5 变化 = 重启该频道）
#   conf/{ch}/ON            开关位（server 开播 touch / 停播 rm）
#   conf/{ch}/.pid/.md5     本管家的运行记账（.pid 同时是 server 侧 running 判据）
#   conf/{ch}/.crash        连续崩溃计数（达到上限停手置 .failed，防秒崩风暴）
# 挂载进官方 savonet 镜像以 command 覆盖运行——不构建自家 liq 镜像。
# 依赖：md5sum/cut/dirname/cat/rm/sleep（镜像自带 coreutils）；无 inotify，轮询足够。

CONF=${LIQ_CONF_DIR:-/hls/conf}
POLL=${LIQ_POLL_SECS:-2}
CRASH_LIMIT=${LIQ_CRASH_LIMIT:-5}

# 容器停止：TERM 只达 PID1（本 shell）——转发给仍在跑的 liq 子进程，避免
# 段写盘中被 namespace 销毁的 SIGKILL 硬切（审查 P2-3）
trap 'test -n "$STOPPING" || { STOPPING=1; kill -TERM $PIDS 2>/dev/null; exit 0; }' TERM INT

PIDS=""

while :; do
  NEWPIDS=""
  for m in "$CONF"/*/station.liq; do
    [ -f "$m" ] || continue
    d=$(dirname "$m")
    pidfile="$d/.pid"
    md5file="$d/.md5"
    crashfile="$d/.crash"
    want_on=$([ -f "$d/ON" ] && echo 1 || echo 0)
    # md5sum 瞬时失败（文件在 glob 与读取之间被删）→ 空记账，跳过本轮
    cur=$(md5sum "$m" 2>/dev/null | cut -d' ' -f1)
    [ -n "$cur" ] || continue
    pid=$(cat "$pidfile" 2>/dev/null)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then alive=1; else alive=0; fi
    last=$(cat "$md5file" 2>/dev/null)

    if [ "$want_on" = 1 ]; then
      # 在跑且配置未变 → 不动；否则（未起/崩溃/配置更新）重启
      if [ "$alive" = 1 ] && [ "$cur" = "$last" ]; then
        NEWPIDS="$NEWPIDS $pid"
        continue
      fi
      if [ "$alive" = 1 ]; then
        kill "$pid" 2>/dev/null
        # 等退出（SIGTERM 收尾刷段可能 >1s），超时 SIGKILL 升级（审查 P1-1）
        n=0
        while kill -0 "$pid" 2>/dev/null && [ $n -lt 20 ]; do
          sleep 0.1
          n=$((n + 1))
        done
        kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
      fi
      # 秒崩风暴防护（审查 P1-2）：连续崩溃计数，达上限停手置 .failed
      crashes=$(cat "$crashfile" 2>/dev/null)
      crashes=${crashes:-0}
      if [ "$crashes" -ge "$CRASH_LIMIT" ]; then
        [ -f "$d/.failed" ] || : > "$d/.failed"
        continue
      fi
      liquidsoap "$m" >/dev/null 2>&1 &
      NEWPIDS="$NEWPIDS $!"
      echo $! > "$pidfile"
      echo "$cur" > "$md5file"
      # 正常存活由下轮清零；此处只累加（spawn 后立即可判定成败要等下轮）
      echo $((crashes + 1)) > "$crashfile"
    elif [ "$alive" = 1 ]; then
      kill "$pid" 2>/dev/null
      rm -f "$pidfile" "$md5file" "$crashfile"
    else
      rm -f "$pidfile" "$md5file" "$crashfile"
    fi
  done
  PIDS="$NEWPIDS"
  # 正常存活过一轮的频道清零崩溃计数
  for d in "$CONF"/*/; do
    [ -d "$d" ] || continue
    pid=$(cat "$d/.pid" 2>/dev/null)
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      rm -f "$d/.crash"
    fi
  done
  sleep "$POLL"
done
