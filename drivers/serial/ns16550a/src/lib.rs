#![no_std]

use zinnia::{
    alloc::string::String,
    device::dt::{Node, driver::Driver},
    log,
    posix::errno::EResult,
};

zinnia::module!("NS16550a serial driver", "Marvin Friedrich", main);

static DRIVER: Driver = Driver {
    name: "ns16550a",
    compatible: &[b"ns16550a"],
    probe,
};

fn probe(node: &Node) -> EResult<()> {
    log!("Hello from {}", String::from_utf8_lossy(node.name()));

    Ok(())
}

pub fn main(_cmdline: &str) {
    match DRIVER.register() {
        Ok(_) => (),
        Err(e) => zinnia::error!("Unable to load driver: {:?}", e),
    }
}
