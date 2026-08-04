fn main() {
    let mut b = cc::Build::new();
    b.files([
        "uacpi/source/default_handlers.c",
        "uacpi/source/event.c",
        "uacpi/source/interpreter.c",
        "uacpi/source/io.c",
        "uacpi/source/mutex.c",
        "uacpi/source/namespace.c",
        "uacpi/source/notify.c",
        "uacpi/source/opcodes.c",
        "uacpi/source/opregion.c",
        "uacpi/source/osi.c",
        "uacpi/source/registers.c",
        "uacpi/source/resources.c",
        "uacpi/source/shareable.c",
        "uacpi/source/sleep.c",
        "uacpi/source/stdlib.c",
        "uacpi/source/tables.c",
        "uacpi/source/types.c",
        "uacpi/source/uacpi.c",
        "uacpi/source/utilities.c",
    ])
    .includes(["uacpi/include/"])
    .define("UACPI_SIZED_FREES", None)
    .pic(true)
    .flag("-ffreestanding")
    .flag("-nostdlib");

    match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => {
            b.flag("-mgeneral-regs-only");
            b.flag("-mno-red-zone");
        }
        "riscv64" => {}
        _ => (),
    }

    b.compile("uacpi");

    let bindings = bindgen::builder()
        .use_core()
        .wrap_unsafe_ops(true)
        .derive_default(true)
        .derive_debug(true)
        .prepend_enum_name(false)
        .header("src/wrapper.h")
        .clang_arg("-Iuacpi/include/")
        .generate()
        .expect("Unable to generate bindings!");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Unable to write bindings!");
}
