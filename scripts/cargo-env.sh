#!/usr/bin/env bash

_repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH" ;;
esac
export PATH

if [ -z "${CARGO_HOME:-}" ]; then
    if mkdir -p "$HOME/.cargo" 2>/dev/null \
        && touch "$HOME/.cargo/.write-probe" 2>/dev/null; then
        rm -f "$HOME/.cargo/.write-probe"
        CARGO_HOME="$HOME/.cargo"
    else
        CARGO_HOME="$_repo_root/.cargo-home"
    fi
fi

if [ ! -d "$CARGO_HOME/registry" ] && [ -d "$HOME/.cargo/registry" ]; then
    echo "初始化 CARGO_HOME=$CARGO_HOME（从 ~/.cargo/registry 拷一份，本机约 450M）" >&2
    mkdir -p "$CARGO_HOME" || return 1
    cp -r "$HOME/.cargo/registry" "$CARGO_HOME/registry" || return 1
fi
export CARGO_HOME

if [ -z "${LIBCLANG_PATH:-}" ]; then
    for _dir in /usr/lib/llvm/22/lib64 /usr/lib/llvm/22/lib; do
        if [ -e "$_dir/libclang.so" ]; then
            LIBCLANG_PATH="$_dir"
            break
        fi
    done
    if [ -z "${LIBCLANG_PATH:-}" ]; then
        _libclang="$(find /usr/lib /usr/local/lib -name 'libclang.so*' 2>/dev/null | head -n1)"
        if [ -n "$_libclang" ]; then
            LIBCLANG_PATH="$(dirname "$_libclang")"
        fi
    fi
    unset _dir _libclang
fi
export LIBCLANG_PATH
