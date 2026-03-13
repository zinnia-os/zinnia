# Zinnia

![GitHub License](https://img.shields.io/github/license/zinnia-os/zinnia?style=flat&color=blue)
![GitHub Repo stars](https://img.shields.io/github/stars/zinnia-os/zinnia?style=flat)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/zinnia-os/zinnia?style=flat)

Zinnia is a lightweight Unix-like kernel targeting 64-bit devices.

## Building Zinnia

To build the kernel you need `meson` and a C23-compatible GNU-like compiler.
Currently supported are GCC and Clang toolchains.

To configure, run:
```sh
meson setup $build_dir
```

And to build:
```sh
meson compile -C $build_dir
```

To cross-compile, you should follow the Meson cross-compilation guide.
