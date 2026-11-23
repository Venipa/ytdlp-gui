#!/usr/bin/env bash
# Regenerates crates/engine/tests/fixtures (committed, <= 300 KB). Needs ffmpeg on PATH. Usage: make-fixtures.sh [outdir]
set -euo pipefail
out="${1:-$(dirname "$0")/../crates/engine/tests/fixtures}"
mkdir -p "$out/hls" "$out/merge/v" "$out/merge/a" && cd "$out"
ffmpeg -y -v error -f lavfi -i testsrc=size=160x90:rate=10:duration=6 -f lavfi -i sine=frequency=440:duration=6 -c:v libx264 -preset veryfast -crf 35 -g 10 -pix_fmt yuv420p -c:a aac -b:a 24k -movflags +faststart sample.mp4
ffmpeg -y -v error -i sample.mp4 -c copy -f hls -hls_time 2 -hls_playlist_type vod -hls_segment_filename 'hls/seg%d.ts' hls/index.m3u8
ffmpeg -y -v error -f lavfi -i sine=frequency=440:duration=6 -c:a libmp3lame -b:a 32k sample.mp3
# master playlist with a separate audio rendition: yt-dlp lists video-only and audio-only formats, so bv*+ba runs the Merger
ffmpeg -y -v error -i sample.mp4 -an -c copy -f hls -hls_time 2 -hls_playlist_type vod -hls_segment_filename 'merge/v/seg%d.ts' merge/v/index.m3u8
ffmpeg -y -v error -i sample.mp4 -vn -c copy -f hls -hls_time 2 -hls_playlist_type vod -hls_segment_filename 'merge/a/seg%d.ts' merge/a/index.m3u8
printf '#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",NAME="a",DEFAULT=YES,AUTOSELECT=YES,URI="a/index.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=60000,RESOLUTION=160x90,CODECS="avc1.42c00a,mp4a.40.2",AUDIO="aud"
v/index.m3u8
' > merge/master.m3u8
du -sk . | cut -f1 | xargs -I{} echo "fixtures: {} KB"
