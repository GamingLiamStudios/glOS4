#![no_std]
#![no_main]

mod limine;

#[unsafe(link_section = ".limine_requests")]
#[used]
static BASE_REVISION: limine::BaseRevision = limine::BaseRevision::new(4);

#[unsafe(link_section = ".limine_requests")]
#[used]
static FRAMEBUFFER_INFO: limine::Request<limine::FramebufferInfo> =
    limine::Request::from(limine::FramebufferInfo {});

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    assert!(
        BASE_REVISION.is_supported(),
        "Base Revision is not Supported!"
    );

    let framebuffer = unsafe { FRAMEBUFFER_INFO.response() };
    if let Some(limine::FullResponse { revision: _, data }) = framebuffer {
        let buffers = data.as_slice();
        if !buffers.is_empty() {
            let buffer = unsafe { buffers[0].as_ref() };
            let dest = buffer.address;

            // For now, assume RGBX32
            for i in 0..100 {
                unsafe {
                    let Ok(index) = usize::try_from(i * (buffer.pitch / 4) + i) else {
                        unreachable!("Limine only supports 64-bit")
                    };
                    dest.byte_add(index).write_bytes(0xff, 4);
                }
            }
        }
    }
    loop {}
}

#[panic_handler]
const fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
