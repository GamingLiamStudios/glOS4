use x86_64::{
    registers::control::Cr2,
    structures::idt::{
        InterruptDescriptorTable,
        InterruptStackFrame,
        PageFaultErrorCode,
    },
};

use crate::{
    framebuffer::FRAMEBUFFER,
    println,
};

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn initalize_idt() {
    let mut table = InterruptDescriptorTable::new();
    x86_64::set_general_handler!(&mut table, generic_handler);

    table.page_fault.set_handler_fn(page_fault_handler);

    unsafe {
        IDT = table;

        IDT.load();
        x86_64::instructions::interrupts::enable();
    }
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    unsafe {
        FRAMEBUFFER.buffer = None;
    }

    let rbp: u64;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
    }

    let rsp = stack_frame.stack_pointer.as_u64();

    // Example stack bounds (adjust to your kernel)
    let stack_bottom = rsp & !0xffff; // 64 KiB stack
    let stack_top = stack_bottom + 0x10000;

    walk_stack(rbp, stack_bottom, stack_top, |frame| println!("{frame:X?}"));

    let cr2 = Cr2::read_raw();

    panic!("Interrupt! {stack_frame:?} {cr2:X} {error_code:?}");
}

#[allow(clippy::needless_pass_by_value)]
fn generic_handler(
    stack_frame: InterruptStackFrame,
    index: u8,
    error_code: Option<u64>,
) {
    unsafe {
        FRAMEBUFFER.buffer = None;
    }
    panic!("Interrupt! {stack_frame:?} {index} {error_code:?}");
    loop {}
}

#[derive(Debug, Copy, Clone)]
pub struct StackFrame {
    pub rbp: u64,
    pub rip: u64,
}

pub fn walk_stack(
    mut rbp: u64,
    stack_bottom: u64,
    stack_top: u64,
    mut callback: impl FnMut(StackFrame),
) {
    let mut last_rbp = 0;

    for _ in 0..64 {
        // Basic sanity checks
        if rbp < stack_bottom || rbp + 16 > stack_top {
            break;
        }
        if rbp & 0xf != 0 {
            break;
        }
        if rbp <= last_rbp {
            break;
        }

        unsafe {
            let prev_rbp = *(rbp as *const u64);
            let rip = *((rbp + 8) as *const u64);

            callback(StackFrame { rbp, rip });

            last_rbp = rbp;
            rbp = prev_rbp;
        }
    }
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
