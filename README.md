# Zinnia

![GitHub License](https://img.shields.io/github/license/marv7000/zinnia?style=flat&color=blue)
![GitHub Repo stars](https://img.shields.io/github/stars/marv7000/zinnia?style=flat)
![GitHub Issues or Pull Requests](https://img.shields.io/github/issues/marv7000/zinnia?style=flat)

Zinnia is a lightweight Unix-like microkernel targeting 64-bit devices.

## Building Zinnia

To build the kernel and the servers you need `meson` and a C23-compatible GNU-like compiler.
Currently supported are GCC and Clang toolchains. You can find the code in `src/`

To configure, run:
```sh
meson setup -Dbuild_kernel=true -Dbuild_servers=true $build_dir
```

And to build:
```sh
meson compile -C $build_dir
```

To cross-compile, you should follow the Meson cross-compilation guide.

## Building the userspace

### Prerequisites
- `xbstrap` (via pip or from [source](https://github.com/managarm/xbstrap))
- `xbps`

### Building with a Docker container

Create a new directory, e.g. `build-x86_64` and change your working directory to it. Don't leave this directory.

Create a file named `bootstrap-site.yml` with the following contents (where `$ARCH` is the target architecture):

```yaml
define_options:
  arch: $ARCH

labels:
  ban:
  - broken
  - no-$ARCH

pkg_management:
  format: xbps

container:
  runtime: docker
  image: zinnia-buildenv
  src_mount: /var/bootstrap-zinnia/src
  build_mount: /var/bootstrap-zinnia/build
  allow_containerless: true
```


```sh
docker build -t zinnia-buildenv --build-arg=USER=$(id -u) ../support
```

```sh
# Initialize the build directory
xbstrap init ..

# Build all packages
# Note that this will literally build *every* package and might take a while
xbstrap build --all

# Create an empty image
xbstrap run empty-image

# Create an initramfs
xbstrap run make-initramfs

# Copy everything into the image
xbstrap run make-image
```

## Testing in QEMU

```sh
xbstrap run qemu
```
