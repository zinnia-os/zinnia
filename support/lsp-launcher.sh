#!/bin/sh
set -ue
# check if $1 is a valid build directory
[ -L "${1:-/dev/null}"/bootstrap.link ] || exit 1

# find compile_commands.json
sourcedir="$(pwd)"
until [ -z "$sourcedir" ] || [ -L "$sourcedir/compile_commands.json" ]; do
	sourcedir="${sourcedir%/*}"
done

# if not found, exit
[ -z "$sourcedir" ] && exit 2

# extract the package name from the compile commands link
x="$(readlink "$sourcedir/compile_commands.json")"
x="${x%/*}"
x="${x##*/}"

# run the lsp server
exec xbstrap -C "$1" \
    lsp "$x" --extra-tools host-llvm -- \
    env HOME=@BUILD_ROOT@/clangd_home \
        XDG_CACHE_HOME=@BUILD_ROOT@/clangd_home/cache \
    clangd -background-index -header-insertion=never \
    --path-mappings \
    @HOST_BUILD_ROOT@=@BUILD_ROOT@,@HOST_SOURCE_ROOT@=@SOURCE_ROOT@
