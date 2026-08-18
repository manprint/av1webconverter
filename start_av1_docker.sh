#!/usr/bin/env bash

set -Eeuo pipefail

# Tutti i valori possono essere sovrascritti tramite variabili d'ambiente.
scale_height="${SCALE_HEIGHT:-480}"
crf="${CRF:-47}"
preset="${PRESET:-1}"
audio_bitrate="${AUDIO_BITRATE:-48k}"
local_cpus="${LOCAL_CPUS:-4}"
remote_cpus="${REMOTE_CPUS:-6}"
svtav1_params="${SVTAV1_PARAMS:-tune=5}"
progress_interval="${PROGRESS_INTERVAL:-5}"

remote_user="${REMOTE_USER:-ubuntu}"
remote_host="${REMOTE_HOST:-212.227.88.91}"

# Dopo il push su Docker Hub, impostare FFMPEG_IMAGE con il riferimento completo
# (es. utente/ffmpeg-svtav1:9.0.1-svt4.2.0-vmaf3.2.0) su entrambi gli host.
ffmpeg_image="${FFMPEG_IMAGE:-fabiop85/ffmpeg-netflix:latest}"

usage() {
    printf 'Uso: %s FILE_VIDEO\n' "$(basename -- "$0")" >&2
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'Errore: comando richiesto non trovato: %s\n' "$1" >&2
        exit 127
    fi
}

show_progress() {
    local label=$1
    local total_seconds=$2
    local total_frames=$3

    awk -v label="$label" -v total="$total_seconds" \
        -v expected_frames="$total_frames" '
        function clock(value, hours, minutes, seconds) {
            if (value < 0)
                value = 0
            hours = int(value / 3600)
            minutes = int(value / 60) % 60
            seconds = int(value) % 60
            return sprintf("%02d:%02d:%02d", hours, minutes, seconds)
        }
        function human_size(bytes) {
            bytes += 0
            if (bytes >= 1000000000)
                return sprintf("%.2f GB", bytes / 1000000000)
            if (bytes >= 1000000)
                return sprintf("%.2f MB", bytes / 1000000)
            if (bytes >= 1000)
                return sprintf("%.1f kB", bytes / 1000)
            return sprintf("%.0f B", bytes)
        }

        /^frame=/ {
            frame = substr($0, index($0, "=") + 1)
            next
        }
        /^fps=/ || /^stream_[^=]+=/ || /^out_time_ms=/ ||
        /^out_time=/ || /^dup_frames=/ || /^drop_frames=/ {
            next
        }
        /^total_size=/ {
            value = substr($0, index($0, "=") + 1)
            if (value ~ /^[0-9]+$/)
                written_bytes = value
            next
        }
        /^bitrate=/ {
            bitrate = substr($0, index($0, "=") + 1)
            next
        }
        /^out_time_us=/ {
            value = substr($0, index($0, "=") + 1)
            if (value ~ /^[0-9]+$/ && value / 1000000 > elapsed)
                elapsed = value / 1000000
            next
        }
        /^speed=/ {
            value = substr($0, index($0, "=") + 1)
            if (value ~ /^[0-9]+([.][0-9]+)?x$/)
                speed = value
            next
        }
        /^progress=/ {
            state = substr($0, index($0, "=") + 1)
            displayed_elapsed = elapsed
            if (expected_frames > 0 && frame + 0 > 0) {
                frame_elapsed = (frame + 0) * total / expected_frames
                if (frame_elapsed > displayed_elapsed)
                    displayed_elapsed = frame_elapsed
            }

            percent = total > 0 ? displayed_elapsed * 100 / total : 0
            if (state == "end")
                percent = 100
            else if (percent > 99.9)
                percent = 99.9
            if (percent < 0)
                percent = 0

            speed_number = speed
            sub(/x$/, "", speed_number)
            eta = "--:--:--"
            if (speed_number + 0 > 0)
                eta = clock((total - displayed_elapsed) / speed_number)

            if (speed == "")
                speed = "N/D"
            if (bitrate == "")
                bitrate = "N/D"
            if (frame == "")
                frame = "0"
            written = written_bytes == "" ? "N/D" : human_size(written_bytes)

            printf "[%s] %5.1f%% | %s / %s | frame %s | file %s | %s | %s | ETA %s\n", \
                label, percent, clock(displayed_elapsed), clock(total), frame, \
                written, bitrate, speed, eta
            fflush()
            next
        }

        {
            print
            fflush()
        }
    '
}

if (( $# != 1 )); then
    usage
    exit 2
fi

input_path=$1
if [[ ! -f "$input_path" || ! -r "$input_path" ]]; then
    printf 'Errore: file non esistente o non leggibile: %s\n' "$input_path" >&2
    exit 2
fi

require_command docker
require_command ssh
require_command awk
require_command ffmpeg
require_command ffprobe
require_command mktemp
require_command stat

if ! awk -v value="$progress_interval" \
        'BEGIN { exit !(value ~ /^[0-9]+([.][0-9]+)?$/ && value > 0) }'; then
    printf 'Errore: PROGRESS_INTERVAL deve essere un numero maggiore di zero.\n' >&2
    exit 2
fi

duration=$(
    ffprobe -v error \
        -show_entries format=duration \
        -of default=noprint_wrappers=1:nokey=1 \
        -i "$input_path"
)
if ! awk -v value="$duration" 'BEGIN { exit !(value > 0) }'; then
    printf 'Errore: durata video non valida o non rilevabile.\n' >&2
    exit 1
fi

video_frame_rate=$(
    ffprobe -v error -select_streams v:0 \
        -show_entries stream=avg_frame_rate \
        -of default=noprint_wrappers=1:nokey=1 \
        -i "$input_path"
)
estimated_frames=$(
    awk -v duration="$duration" -v rate="$video_frame_rate" '
        BEGIN {
            split(rate, parts, "/")
            if (parts[1] > 0 && parts[2] > 0)
                printf "%.0f", duration * parts[1] / parts[2]
            else
                print 0
        }
    '
)

input_dir=$(cd -- "$(dirname -- "$input_path")" && pwd -P)
input_name=$(basename -- "$input_path")
stem=${input_name%.*}

output_dir="${OUTPUT_DIR:-$PWD/converted}"
mkdir -p -- "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd -P)
output_path="$output_dir/${stem}_av1.mp4"

# Per default una conversione già presente non viene sovrascritta.
if [[ -e "$output_path" && "${OVERWRITE:-0}" != 1 ]]; then
    printf 'Saltato (output già presente): %s\n' "$output_path"
    exit 0
fi

tmp_mkv=
tmp_mp4=

cleanup() {
    local status=$?
    trap - EXIT
    [[ -z "$tmp_mkv" ]] || rm -f -- "$tmp_mkv"
    [[ -z "$tmp_mp4" ]] || rm -f -- "$tmp_mp4"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

tmp_mkv=$(mktemp --tmpdir="$output_dir" '.av1-remote.XXXXXX.mkv')
tmp_mp4=$(mktemp --tmpdir="$output_dir" '.av1-final.XXXXXX.mp4')

# %q conserva esattamente i confini degli argomenti attraverso la shell remota.
remote_ffmpeg=(
    docker run -i --rm --cpus="$remote_cpus"
    "$ffmpeg_image"
    -nostdin -hide_banner -loglevel error -nostats
    -stats_period "$progress_interval" -progress pipe:2
    -i pipe:0
    -map 0:v:0 -map '0:a?'
    -c:v libsvtav1
    -vf "scale=-2:trunc(min(${scale_height}\\,ih)/2)*2:reset_sar=1"
    -b:v 0 -crf "$crf" -preset "$preset"
    -svtav1-params "$svtav1_params"
    -g 240 -pix_fmt yuv420p10le
    -c:a libopus -b:a "$audio_bitrate"
    -f matroska pipe:1
)
printf -v remote_command '%q ' "${remote_ffmpeg[@]}"

ssh_options=(
    -o BatchMode=yes
    -o ConnectTimeout=15
    -o ServerAliveInterval=30
    -o ServerAliveCountMax=3
)

printf 'Codifica remota: %s\n' "$input_path"

if ! docker run --rm --cpus="$local_cpus" \
        -v "$input_dir:/input:ro" \
        "$ffmpeg_image" \
        -nostdin -hide_banner -loglevel error \
        -i "/input/$input_name" \
        -map 0:v:0 -map '0:a?' \
        -c copy -f matroska pipe:1 |
    ssh "${ssh_options[@]}" "$remote_user@$remote_host" "$remote_command" \
        >"$tmp_mkv" \
        2> >(show_progress 'Codifica' "$duration" "$estimated_frames" >&2); then
    printf 'Errore: remux locale, collegamento SSH o codifica remota falliti.\n' >&2
    exit 1
fi

if [[ ! -s "$tmp_mkv" ]]; then
    printf 'Errore: la codifica remota ha prodotto un file vuoto.\n' >&2
    exit 1
fi

# Il remux in un secondo file temporaneo evita output finali parziali.
if ! ffmpeg -nostdin -hide_banner -loglevel error -y \
        -i "$tmp_mkv" \
        -map 0 -c copy -tag:v av01 -movflags +faststart \
        "$tmp_mp4"; then
    printf 'Errore: creazione del file MP4 finale fallita.\n' >&2
    exit 1
fi

actual_size_bytes=$(stat -c %s "$tmp_mp4")
if [[ "${OVERWRITE:-0}" == 1 ]]; then
    mv -f -- "$tmp_mp4" "$output_path"
else
    mv -n -- "$tmp_mp4" "$output_path"
    if [[ -e "$tmp_mp4" ]]; then
        printf 'Errore: output creato da un altro processo: %s\n' "$output_path" >&2
        exit 1
    fi
fi
tmp_mp4=

printf 'Completato: %s\n' "$output_path"
awk -v size="$actual_size_bytes" \
    'BEGIN { printf "Dimensione finale: %.2f MB.\n", size / 1000000 }'
