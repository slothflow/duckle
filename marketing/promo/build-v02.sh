#!/usr/bin/env bash
# Build the Duckle v0.2 capability promo (silent, 1080p, ~30s).
# Inputs: logo PNG + 6 real v0.2 screenshots.
# Output: marketing/promo/out/duckle-promo-v02.mp4
#
# Same look as build.sh, but screenshots are FIT (not cropped) onto the
# 1920x1080 frame so the UI sidebars/panels stay visible, with the caption
# in a clean band below the shot instead of overlapping the run output.
#
# Run from anywhere; the script cd's into its own dir so ffmpeg filter
# strings can reference fonts by bare filename.

set -euo pipefail

cd "$(dirname "$0")"

ROOT="$(cd ../.. && pwd)"
SHOTS="$ROOT/docs/assets/real-life-screenshot"
EXTRA="C:/Users/Sourav Roy/Documents/New folder (2)"
LOGO="$ROOT/apps/desktop/icons/icon-source.png"
OUTDIR="out"
SCENES="scenes-v02"

mkdir -p "$OUTDIR" "$SCENES"

[ -f font-reg.ttf ] || cp /c/Windows/Fonts/segoeui.ttf font-reg.ttf
[ -f font-bold.ttf ] || cp /c/Windows/Fonts/segoeuib.ttf font-bold.ttf

W=1920
H=1080
FPS=30

TEXT="0xecf0f7"
MUTED="0xaab3c5"
ACCENT="0xffd400"

FR="font-reg.ttf"
FB="font-bold.ttf"

FF="ffmpeg -y -hide_banner -loglevel error"

# Fit a screenshot into the top 880px, pad to 1920x1080 (brand bg), leaving
# a 200px caption band at the bottom; draw a title + subtitle in it.
# Args: infile dur(int) outfile title subtitle
shot_scene () {
  local IN="$1" DUR="$2" OUT="$3" TITLE="$4" SUB="$5"
  local FO=$((DUR - 1))
  $FF -loop 1 -t "$DUR" -framerate ${FPS} -i "$IN" \
      -filter_complex "
        [0:v]scale=${W}:880:force_original_aspect_ratio=decrease,
        pad=${W}:${H}:(ow-iw)/2:0:color=0x07090f,
        drawtext=fontfile=${FB}:text='${TITLE}':fontcolor=${TEXT}:fontsize=56:x=80:y=926,
        drawtext=fontfile=${FR}:text='${SUB}':fontcolor=${MUTED}:fontsize=32:x=80:y=1000,
        drawtext=fontfile=${FB}:text='Duckle':fontcolor=${ACCENT}:fontsize=24:x=w-tw-40:y=934,
        fade=t=in:st=0:d=0.4,fade=t=out:st=${FO}:d=0.5
      " -c:v libx264 -pix_fmt yuv420p -preset medium -crf 18 -t "$DUR" "$OUT"
}

# ---- Scene 1: Logo + brand (4s)
$FF -f lavfi -i "color=c=#07090f:s=${W}x${H}:r=${FPS}:d=4" \
    -loop 1 -t 4 -i "$LOGO" \
    -filter_complex "
      [1:v]scale=420:420[lg];
      [0:v][lg]overlay=(W-w)/2:(H-h)/2-120:enable='between(t,0.2,4)',
      drawtext=fontfile=${FB}:text='Duckle v0.2':fontcolor=${TEXT}:fontsize=110:x=(w-tw)/2:y=h/2+180:alpha='if(lt(t,0.7),0,if(lt(t,1.5),(t-0.7)/0.8,1))',
      drawtext=fontfile=${FR}:text='Local-first ETL. New in this release.':fontcolor=${MUTED}:fontsize=40:x=(w-tw)/2:y=h/2+320:alpha='if(lt(t,1.3),0,if(lt(t,2.1),(t-1.3)/0.8,1))',
      fade=t=out:st=3.5:d=0.5
    " -c:v libx264 -pix_fmt yuv420p -preset medium -crf 18 "$SCENES/01_logo.mp4"

# ---- Capability scenes
shot_scene "$SHOTS/mega-pipeline-join.png" 4 "$SCENES/02_join.mp4" \
  "Join across sources, visually" \
  "CSV, Parquet, DuckDB and SQLite into one Map node. No SQL."

shot_scene "$EXTRA/monolithic_pipeline_2.png" 4 "$SCENES/03_mapper.mp4" \
  "The visual Map editor" \
  "Main plus lookups, per-output expressions, an inline filter."

shot_scene "$EXTRA/monolithic_pipeline_3.png" 4 "$SCENES/04_parallel.mp4" \
  "Fan out in parallel" \
  "Aggregates, windows and top-N branches run side by side."

shot_scene "$SHOTS/mega-pipeline-parallelize.png" 4 "$SCENES/05_output.mp4" \
  "16 nodes, one run" \
  "Independent branches finish in milliseconds. Concurrency auto-detects."

shot_scene "$SHOTS/cdc-ducklake.png" 4 "$SCENES/06_cdc.mp4" \
  "Change data capture" \
  "DuckLake change-feed, upsert and delete propagation. 100k rows."

shot_scene "$SHOTS/incremental-load.png" 4 "$SCENES/07_incremental.mp4" \
  "Incremental loads at scale" \
  "Watermark over 5,000,000 rows. State advances only on success."

# ---- End card (4s)
$FF -f lavfi -i "color=c=#07090f:s=${W}x${H}:r=${FPS}:d=4" \
    -loop 1 -t 4 -i "$LOGO" \
    -filter_complex "
      [1:v]scale=300:300[lg];
      [0:v][lg]overlay=(W-w)/2:(H-h)/2-220,
      drawtext=fontfile=${FB}:text='Duckle':fontcolor=${TEXT}:fontsize=92:x=(w-tw)/2:y=h/2+120,
      drawtext=fontfile=${FR}:text='Free  /  Open source  /  Local-first':fontcolor=${MUTED}:fontsize=36:x=(w-tw)/2:y=h/2+240,
      drawtext=fontfile=${FB}:text='github.com/SouravRoy-ETL/duckle':fontcolor=${ACCENT}:fontsize=44:x=(w-tw)/2:y=h/2+320,
      fade=t=in:st=0:d=0.5,fade=t=out:st=3.5:d=0.5
    " -c:v libx264 -pix_fmt yuv420p -preset medium -crf 18 "$SCENES/08_end.mp4"

# ---- Concat into final
cat > "$SCENES/concat.txt" <<EOF
file '01_logo.mp4'
file '02_join.mp4'
file '03_mapper.mp4'
file '04_parallel.mp4'
file '05_output.mp4'
file '06_cdc.mp4'
file '07_incremental.mp4'
file '08_end.mp4'
EOF

$FF -f concat -safe 0 -i "$SCENES/concat.txt" -c copy "$OUTDIR/duckle-promo-v02.mp4"

echo "Done: $OUTDIR/duckle-promo-v02.mp4"
ls -lh "$OUTDIR/duckle-promo-v02.mp4"
ffprobe -v error -show_entries format=duration -show_entries stream=width,height,codec_name -of default=noprint_wrappers=1 "$OUTDIR/duckle-promo-v02.mp4"
