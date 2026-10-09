#!/bin/bash
# record.sh DISPLAY PORT OUTDIR: record enclosure_mcp.py driving the app on X display DISPLAY
# (1920x1080, the app started with --control PORT) to OUTDIR/enclosure-demo.mp4 (H.264,
# captions burnt in) and OUTDIR/enclosure-demo.gif. Needs ffmpeg with x11grab and libass.
set -e
DISP=${1:-:0}; PORT=${2:-7878}; OUT=${3:-.}
HERE=$(cd "$(dirname "$0")" && pwd)
CLI=${SOLVECRAFT_CLI:-solvecraft-cli}
mkdir -p "$OUT"
# x11grab stamps frames with the wall clock: its "start:" is when the first frame was taken.
ffmpeg -loglevel info -nostats -y -f x11grab -draw_mouse 0 -video_size 1920x1080 -framerate 30 -i "$DISP" \
  -c:v libx264 -preset veryfast -crf 16 -pix_fmt yuv420p "$OUT/raw.mp4" 2> "$OUT/ffmpeg.log" &
FF=$!
sleep 2
python3 "$HERE/enclosure_mcp.py" --port "$PORT" --cli "$CLI" --log "$OUT/steps.log"
sleep 2
kill -INT $FF; wait $FF || true
START=$(grep -o "start: [0-9.]*" "$OUT/ffmpeg.log" | head -1 | cut -d" " -f2)
python3 "$HERE/captions.py" "$OUT/steps.log" "$START" "$OUT/captions.ass"
ffmpeg -loglevel error -y -i "$OUT/raw.mp4" -vf "ass=$OUT/captions.ass" \
  -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -movflags +faststart "$OUT/enclosure-demo.mp4"
ffmpeg -loglevel error -y -i "$OUT/enclosure-demo.mp4" \
  -vf "fps=8,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4" \
  "$OUT/enclosure-demo.gif"
ls -la "$OUT"/enclosure-demo.*
