#!/usr/bin/env bash
set -euo pipefail
root=$1
binary_dir=$2
export PATH="$binary_dir:$PATH"
source "$root/shell/cd.bash"
cd "$root"
cd src
[[ "$PWD" == "$root/src" ]]
cd ..
[[ "$PWD" == "$root" ]]
if cd this-directory-does-not-exist 2>/dev/null; then
    exit 1
fi
[[ "$PWD" == "$root" ]]
printf 'Bash cd integration passed\n'
