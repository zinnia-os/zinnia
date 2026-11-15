# Contributing Guide

Thanks for your interest in improving Zinnia!

## Preface

Contributing to the Zinnia project is fairly straightforward.
There are only a few expectations of you as a contributor:

### 1. Be respectful to everyone

It goes without saying that you should not be rude to other contributors.
Disagree with code, not with people.

### 2. Understand and own your changes

- All changes must be compatible with the terms of the BSD-3-Clause license.
- **You** are accountable for every change you submit.

Please do not submit any changes you don't fully understand, e.g. if you copied
them from somewhere, or used some kind of external assistance, human or not.

*From an old IBM manual:*

> A COMPUTER CAN NEVER BE HELD ACCOUNTABLE
>
> THEREFORE A COMPUTER MUST NEVER MAKE A MANAGEMENT DECISION

## Contributing

Most changes can go straight into a pull request without problems.
For larger changes, please open an issue and/or ask first.

### Building and Testing

Tests run on x86_64 in QEMU and need an OVMF image (set with `-Dovmf=`
if Meson can't find it). Enable `CONFIG_KTEST` in your config, then run:
```sh
meson test -C $build_dir
```

### Code style

- Format all C code with `clang-format` using the `.clang-format` in the
  repository root.
- Make sure that added code is REUSE compliant.
- Prepend commit messages with the subsystem they are touching, for example:
  `dev/foobar: Fix reference counting on close()`
