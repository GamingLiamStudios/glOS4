use x86_64::structures::idt::{
    InterruptDescriptorTable,
    InterruptStackFrame,
    PageFaultErrorCode,
};

use crate::println;

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn initalize_idt() {
    let mut table = InterruptDescriptorTable::new();
    x86_64::set_general_handler!(&mut table, generic_handler);

    unsafe {
        IDT = table;

        IDT.load();
        x86_64::instructions::interrupts::enable();
    }
}

#[allow(clippy::needless_pass_by_value)]
fn generic_handler(
    stack_frame: InterruptStackFrame,
    index: u8,
    error_code: Option<u64>,
) {
    panic!("Interrupt! {stack_frame:?} {index} {error_code:?}");
    loop {}
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!("at {}\n {}", location, info.message());
    } else {
        println!("{}", info.message());
    }

    loop {}
}
