use alloc::boxed::Box;
use core::fmt::Write;

use embedded_graphics::{
    Pixel,
    mono_font::{
        MonoTextStyle,
        ascii::FONT_10X20,
    },
    pixelcolor::{
        Rgb888,
        raw::ToBytes,
    },
    prelude::{
        DrawTarget,
        Drawable,
        OriginDimensions,
        Point,
        Size,
        WebColors,
    },
    text::Text,
};

use super::limine;
use crate::limine::FramebufferDescriptor;

#[unsafe(link_section = ".limine_requests")]
#[used]
static FRAMEBUFFER_INFO: limine::Request<limine::FramebufferInfo> =
    limine::Request::from(limine::FramebufferInfo {});

fn framebuffer_info() -> &'static FramebufferDescriptor {
    let Some(framebuffer_info) = (unsafe { FRAMEBUFFER_INFO.response() }) else {
        panic!("No framebuffers exist!");
    };

    let Some(framebuffer) = framebuffer_info.as_slice().first() else {
        panic!("No framebuffers exist!");
    };

    unsafe { framebuffer.as_ref() }
}

pub struct UefiFramebuffer {
    current_line: usize,
    pub buffer:   Option<Box<[u8]>>,
}
pub static mut FRAMEBUFFER: UefiFramebuffer = UefiFramebuffer {
    current_line: 0,
    buffer:       None,
};

impl UefiFramebuffer {
    pub fn init(&mut self) {
        let framebuffer = framebuffer_info();

        let Ok(length) = usize::try_from(framebuffer.pitch * framebuffer.height) else {
            unreachable!()
        };

        let buffer = Box::new_uninit_slice(length);
        let mut buffer = unsafe { buffer.assume_init() };
        buffer.fill(0x00);

        self.buffer = Some(buffer);
    }

    pub fn flush(&mut self) {
        let Some(buffer) = self.buffer.as_mut() else {
            return;
        };

        let framebuffer = framebuffer_info();
        unsafe {
            core::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                framebuffer.address.as_ptr(),
                buffer.len(),
            );
        }
    }
}

impl DrawTarget for UefiFramebuffer {
    type Color = Rgb888;
    type Error = ();

    fn draw_iter<I>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        let framebuffer = framebuffer_info();
        if self.buffer.is_none() {
            //self.init();
        }

        for Pixel(point, color) in pixels {
            let Ok(pixel_x) = u64::try_from(point.x) else {
                panic!("Unable to draw pixel! OOB");
            };
            let Ok(pixel_y) = u64::try_from(point.y) else {
                panic!("Unable to draw pixel! OOB");
            };

            let offset = usize::try_from(
                framebuffer.pitch * pixel_y + pixel_x * u64::from(framebuffer.bits_per_pixel / 8),
            )
            .expect("Pixel Offset too large!");

            if let Some(buffer) = self.buffer.as_mut() {
                buffer[offset..offset + color.to_ne_bytes().len()]
                    .copy_from_slice(&color.to_ne_bytes());
            } else {
                unsafe {
                    let addr = framebuffer.address.add(offset);
                    core::ptr::copy_nonoverlapping(
                        (&raw const color).cast(),
                        addr.as_ptr(),
                        size_of_val(&color),
                    );
                }
            }
        }

        Ok(())
    }
}

impl OriginDimensions for UefiFramebuffer {
    fn size(&self) -> Size {
        let framebuffer = framebuffer_info();

        let width = u32::try_from(framebuffer.width).unwrap_or(u32::MAX);
        let height = u32::try_from(framebuffer.height).unwrap_or(u32::MAX);
        Size::new(width, height)
    }
}

// Simple wrapper to write into a byte buffer
struct BufferWriter<'a> {
    buffer:   &'a mut [u8],
    position: usize,
}

impl<'a> BufferWriter<'a> {
    const fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buffer[..self.position]).unwrap_or("")
    }
}

impl core::fmt::Write for BufferWriter<'_> {
    fn write_str(
        &mut self,
        s: &str,
    ) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buffer.len() - self.position;

        if bytes.len() > remaining {
            return Err(core::fmt::Error);
        }

        self.buffer[self.position..self.position + bytes.len()].copy_from_slice(bytes);
        self.position += bytes.len();
        Ok(())
    }
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => ($crate::framebuffer::println(format_args!($($arg)*)));
}

// TODO: Improve
pub fn println(args: core::fmt::Arguments) {
    let mut buffer = [0u8; 2048];
    let mut writer = BufferWriter::new(&mut buffer);
    _ = writer.write_fmt(args);

    let max_chars = ((unsafe { FRAMEBUFFER.size().width } / 10) - 3) as usize;

    for line in writer.as_str().lines() {
        for start in (0..line.len()).step_by(max_chars) {
            let end = core::cmp::min(start + max_chars, line.len());
            let sub_str = &line[start..end];

            unsafe {
                let mut text_y = 20 + FRAMEBUFFER.current_line * 23;
                if text_y + 20 >= FRAMEBUFFER.size().height as usize {
                    text_y -= 23;

                    if FRAMEBUFFER.buffer.is_none() {
                        FRAMEBUFFER.init();
                    }

                    if let Some(buffer) = FRAMEBUFFER.buffer.as_mut() {
                        let buffer_info = framebuffer_info();
                        let Ok(pitch) = usize::try_from(buffer_info.pitch) else {
                            unreachable!()
                        };
                        let clipped_start = pitch * 23;
                        buffer.copy_within(clipped_start..buffer.len(), 0);

                        let len = buffer.len();
                        buffer[len - clipped_start..].fill(0x00);
                        FRAMEBUFFER.current_line -= 1;

                        for _ in 0..10_000_000 {}
                    }
                }

                if let Ok(text_y) = i32::try_from(text_y) {
                    _ = Text::new(
                        sub_str,
                        Point::new(20, text_y),
                        MonoTextStyle::new(&FONT_10X20, Rgb888::CSS_WHITE),
                    )
                    .draw(&mut FRAMEBUFFER);

                    FRAMEBUFFER.current_line += 1;
                    FRAMEBUFFER.flush();
                }
            }
        }
    }
}
