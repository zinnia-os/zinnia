<img src="logo.svg" width="10%"/>

# Zinnia

![GitHub License](https://img.shields.io/github/license/zinnia-os/zinnia?style=flat&color=green)
![GitHub top language](https://img.shields.io/github/languages/top/zinnia-os/zinnia)
![GitHub Repo stars](https://img.shields.io/github/stars/zinnia-os/zinnia?style=flat)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/zinnia-os/zinnia/ci.yml)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/zinnia-os/zinnia?style=flat)
![Matrix](https://img.shields.io/matrix/zinnia%3Amrvn.sh?server_fqdn=matrix.mrvn.sh&fetchMode=summary&link=https%3A%2F%2Fmatrix.to%2F%23%2F%23zinnia%3Amrvn.sh)

Zinnia is a lightweight Unix-like monolithic kernel targeting 64-bit devices.

## Building

To build the kernel you need:
- `meson`
- a `kconfig` frontend (like `kconfiglib` via `pip`)
- a GNU-like C compiler with support for C23 (e.g. `gcc` or `clang`)

To configure, run:
```sh
ARCH=x86_64 alldefconfig
meson setup $build_dir
```

And to build:
```sh
meson compile -C $build_dir
```
