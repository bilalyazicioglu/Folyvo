#!/usr/bin/env bash
# Rebuild both README animations from the fixed-camera recording.
# An optional input recording replaces the archived original.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
for tool in ffmpeg gifsicle; do
  command -v "$tool" >/dev/null || { echo "Required: $tool" >&2; exit 1; }
done
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ "$#" -gt 0 ]; then
  input="$1"
else
  # The original recording predates the animated camera. Keep its native 15 fps;
  # duplicating frames at 60 fps adds decoding work without adding motion detail.
  git -C "$root" show f8a8c37acad5c25a1410405e63cdb3bb2055311a:docs/assets/folyvo-demo.gif >"$work/original.gif"
  input="$work/original.gif"
fi

# Skip the empty desktop at the start and the end card; leave time to read the
# restore confirmation and see the new save. Do not animate the viewport.
timing='trim=start=2:duration=12,setpts=PTS-STARTPTS,tpad=stop_mode=clone:stop_duration=2,fps=15'
ffmpeg -hide_banner -loglevel error -y -i "$input" -an \
  -filter_complex "[0:v]$timing,scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle" \
  -loop 0 "$work/demo.gif"
gifsicle -O3 "$work/demo.gif" -o "$work/folyvo-demo.gif"

ffmpeg -hide_banner -loglevel error -y -i "$input" -an \
  -vf "$timing,scale=960:-1:flags=lanczos" \
  -c:v libsvtav1 -preset 6 -crf 28 -pix_fmt yuv420p -loop 0 "$work/folyvo-demo.avif"

cp "$work/folyvo-demo.gif" "$root/docs/assets/folyvo-demo.gif"
cp "$work/folyvo-demo.avif" "$root/docs/assets/folyvo-demo.avif"
ls -lh "$root/docs/assets/folyvo-demo.gif" "$root/docs/assets/folyvo-demo.avif"
