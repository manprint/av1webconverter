#!/usr/bin/env bash

set -Eeuo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)

# nullglob impedisce che "*.mp4" diventi un nome letterale quando non ci
# sono file; nocaseglob include anche estensioni come .MP4 e .Mp4.
shopt -s nullglob nocaseglob
files=( *.mp4 )
shopt -u nullglob nocaseglob

if (( ${#files[@]} == 0 )); then
    printf 'Nessun file MP4 da convertire in: %s\n' "$PWD"
    exit 0
fi

failures=0

for file in "${files[@]}"; do
    printf '\nElaborazione: %s\n' "$file"
    if "$script_dir/start_av1_docker.sh" "$file"; then
        printf 'OK: %s\n' "$file"
    else
        status=$?
        printf 'ERRORE (%d): %s\n' "$status" "$file" >&2
        (( failures += 1 ))
    fi
done

if (( failures > 0 )); then
    printf '\nConversioni fallite: %d su %d.\n' "$failures" "${#files[@]}" >&2
    exit 1
fi

printf '\nTutte le %d conversioni sono terminate correttamente.\n' "${#files[@]}"
