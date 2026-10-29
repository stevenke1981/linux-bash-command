# Source this file in Bash after adding the executable directory to PATH.
cd() {
    local executable resolved
    executable=$(type -P cd) || { printf '%s\n' 'cd: executable not found in PATH' >&2; return 1; }
    resolved=$(OLDPWD="${OLDPWD-}" "$executable" "$@") || return $?
    builtin cd -- "$resolved"
}
