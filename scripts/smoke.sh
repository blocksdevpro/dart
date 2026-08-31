#!/bin/sh
set -eu

repository_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
smoke_dir=$(mktemp -d)

cleanup() {
    if [ -n "${smoke_dir:-}" ] && [ -d "$smoke_dir" ]; then
        rm -rf -- "$smoke_dir"
    fi
}
trap cleanup EXIT HUP INT TERM

cd "$repository_dir"
cargo build --quiet
dart_binary="$repository_dir/target/debug/dart"

runtime_id="1.21.8/0.17.2/1.1.2"
runtime_dir="$smoke_dir/runtimes/fabric/1.21.8/0.17.2/1.1.2"
mkdir -p "$runtime_dir"
printf 'PK\003\004smoke fixture' > "$runtime_dir/fabric-server-launch.jar"

"$dart_binary" --home "$smoke_dir" create survival "Survival Server" \
    --runtime "$runtime_id" --accept-eula >/dev/null
"$dart_binary" --home "$smoke_dir" create survival "Survival Server" \
    --runtime "$runtime_id" --accept-eula >/dev/null

config_path="$smoke_dir/instances/survival/dart.toml"
test -f "$config_path"
grep -q '^name = "Survival Server"$' "$config_path"
grep -q '^format_version = 2$' "$config_path"
grep -q '^minecraft = "1.21.8"$' "$config_path"
grep -q '^loader = "0.17.2"$' "$config_path"
grep -q '^installer = "1.1.2"$' "$config_path"
cmp "$runtime_dir/fabric-server-launch.jar" \
    "$smoke_dir/instances/survival/fabric-server-launch.jar"
grep -q '^eula=true$' "$smoke_dir/instances/survival/eula.txt"

runtime_output=$("$dart_binary" --home "$smoke_dir" runtimes list)
printf '%s\n' "$runtime_output" | grep -q "^$runtime_id[[:space:]]"

list_output=$("$dart_binary" --home "$smoke_dir" list)
printf '%s\n' "$list_output" | grep -q '^survival[[:space:]]Survival Server[[:space:]]'

if "$dart_binary" --home "$smoke_dir" create ../escape Escape \
    --runtime "$runtime_id" >/dev/null 2>&1; then
    echo "unsafe instance ID was accepted" >&2
    exit 1
fi

printf '%s\n' "Dart smoke test passed"
