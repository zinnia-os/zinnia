<img src="logo.svg" width="10%"/>

# Zinnia

![GitHub License](https://img.shields.io/github/license/zinnia-os/zinnia?style=flat&color=red)
![GitHub Repo stars](https://img.shields.io/github/stars/zinnia-os/zinnia?style=flat)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/zinnia-os/zinnia/ci.yml)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/zinnia-os/zinnia?style=flat)

Zinnia is a modular 64-bit BSD-like kernel written in Rust.
It aims for full compatibility with POSIX 2024.

> [!NOTE]
> This repository contains only the kernel and drivers.
> If you want to get a bootable image, you might want to check out
> **https://github.com/zinnia-os/bootstrap** instead.

# Building

## Cloning the repository

```sh
git clone https://github.com/zinnia-os/zinnia
git submodule update --init --recursive
```

## Compiling the kernel

To compile the kernel you will need:
- cargo
- rustc
- clang (Used for bindgen)
- lld

Make sure you have a full nightly toolchain installed,
including the `rust-src` component.

The following commmand will build the kernel and all drivers for x86_64:
```sh
cargo +nightly build --release --target toolchain/x86_64-kernel.json
```

## Debugging

Follow the debugging setup from **https://github.com/zinnia-os/bootstrap**
