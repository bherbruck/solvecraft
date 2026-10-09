#!/bin/bash
# record_ui.sh DISPLAY PORT OUTDIR [STORY]: record enclosure_ui_mcp.py --story STORY (v2 or v3,
# default v3) driving the app on X display DISPLAY (1920x1080, started with --control PORT) to
# OUTDIR/enclosure-demo-STORY.mp4 (H.264) and .gif, with captions and a drawn pointer (the app
# gets synthetic pointer events, so the screen's own pointer never moves). Needs ffmpeg with
# x11grab and libass.
set -e
DISP=${1:-:0}; PORT=${2:-7878}; OUT=$(cd "${3:-.}" && pwd); STORY=${4:-v3}
HERE=$(cd "$(dirname "$0")" && pwd)
CLI=${SOLVECRAFT_CLI:-solvecraft-cli}
# x11grab stamps frames with the wall clock: its "start:" is when the first frame was taken.
ffmpeg -loglevel info -nostats -y -f x11grab -draw_mouse 0 -video_size 1920x1080 -framerate 30 -i "$DISP" \
  -c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p "$OUT/raw.mp4" 2> "$OUT/ffmpeg.log" &
FF=$!
trap 'kill -INT $FF 2>/dev/null || true' EXIT
sleep 1
python3 "$HERE/enclosure_ui_mcp.py" --port "$PORT" --cli "$CLI" --story "$STORY" --log "$OUT/steps.log"
sleep 1
kill -INT $FF; wait $FF || true
START=$(grep -o "start: [0-9.]*" "$OUT/ffmpeg.log" | head -1 | cut -d" " -f2)
python3 "$HERE/overlay.py" "$OUT/steps.log" "$START" "$OUT"
read CX CY < "$OUT/cursor.start"
ffmpeg -loglevel error -y -i "$OUT/raw.mp4" -loop 1 -i "$OUT/cursor.png" -loop 1 -i "$OUT/ripple.png" -filter_complex \
  "[0:v]ass=$OUT/captions.ass[v0];[v0][2:v]overlay@rip=x=-100:y=-100:enable='$(cat "$OUT/ripple.enable")':shortest=1[v1];[v1][1:v]overlay@cur=x=$CX:y=$CY:shortest=1,sendcmd=f=$OUT/cursor.cmd[v]" \
  -map "[v]" -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -movflags +faststart "$OUT/enclosure-demo-$STORY.mp4"
ffmpeg -loglevel error -y -i "$OUT/enclosure-demo-$STORY.mp4" \
  -vf "fps=8,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=80:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4" \
  "$OUT/enclosure-demo-$STORY.gif"
ls -la "$OUT"/enclosure-demo-$STORY.*
