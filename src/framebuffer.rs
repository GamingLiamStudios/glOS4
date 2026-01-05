use alloc::boxed::Box;
use core::{
    fmt,
    mem::MaybeUninit,
    range::Range,
};

use bitvec::boxed::BitBox;
use num_traits::float::FloatCore;
use rgb::Rgba;
use swash::{
    FontRef,
    scale::{
        Render,
        ScaleContext,
        Source,
        image::Content,
    },
};

use super::limine;
use crate::{
    ansi::{
        self,
        ControlSequence,
        Direction,
        EraseRegion,
        SelectGraphicRendition,
    },
    limine::FramebufferDescriptor,
};

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

/// Describes memory layout of a single Pixel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelLayout {
    /// Red-Green-Blue
    Rgb {
        red_mask:   Range<u8>,
        green_mask: Range<u8>,
        blue_mask:  Range<u8>,
    },
    /// Y-Cb-Cr
    Yuv {
        luma_mask: Range<u8>,
        blue_mask: Range<u8>,
        red_mask:  Range<u8>,
    },
}

impl PixelLayout {
    const fn from_descriptor(desc: &FramebufferDescriptor) -> Self {
        Self::Rgb {
            red_mask:   Range {
                start: desc.red_mask_shift,
                end:   desc.red_mask_shift + desc.red_mask_size,
            },
            green_mask: Range {
                start: desc.green_mask_shift,
                end:   desc.green_mask_shift + desc.green_mask_size,
            },
            blue_mask:  Range {
                start: desc.blue_mask_shift,
                end:   desc.blue_mask_shift + desc.blue_mask_size,
            },
        }
    }
}

#[derive(Debug)]
pub struct PixelBuffer<T: AsRef<[u8]>> {
    data: T,

    pitch:  usize,
    width:  usize,
    height: usize,

    /// Layout of [`data`](Self::data)
    pixel_layout:   PixelLayout,
    bits_per_pixel: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point2 {
    x: usize,
    y: usize,
}

fn u64_to_usize(value: u64) -> usize {
    let Ok(value) = usize::try_from(value) else {
        unreachable!()
    };
    value
}

pub struct UefiFramebuffer {
    active_mode: usize,
}
pub static FRAMEBUFFER: spin::Mutex<UefiFramebuffer> =
    spin::Mutex::new(UefiFramebuffer { active_mode: 0 });

impl UefiFramebuffer {
    #[allow(clippy::unused_self)]
    pub fn available_modes(&self) -> impl Iterator<Item = &'static FramebufferDescriptor> {
        let Some(framebuffer_info) = (unsafe { FRAMEBUFFER_INFO.response() }) else {
            panic!("No framebuffers exist!");
        };

        framebuffer_info
            .as_slice()
            .iter()
            .map(|ptr| unsafe { ptr.as_ref() })
    }

    pub fn active_mode(&self) -> &'static FramebufferDescriptor {
        unsafe {
            let Some(framebuffer_info) = FRAMEBUFFER_INFO.response() else {
                panic!("No framebuffers exist!");
            };

            framebuffer_info.as_slice()[self.active_mode].as_ref()
        }
    }

    pub const fn set_active_mode(
        &mut self,
        mode: usize,
    ) {
        self.active_mode = mode;
    }

    /// Safety:
    /// - `buffer` does not point to target framebuffer
    #[allow(clippy::needless_pass_by_ref_mut)] // Is absolutely a mutable operation but rust is dumb
    pub fn blit_buffer<T: AsRef<[u8]>>(
        &mut self,
        buffer: &PixelBuffer<T>,
        offset: &Point2,
    ) {
        let active_mode = self.active_mode();

        if offset.x + buffer.width > u64_to_usize(active_mode.width) {
            return;
        }
        if offset.y + buffer.height > u64_to_usize(active_mode.height) {
            return;
        }

        let dst_pitch = u64_to_usize(active_mode.pitch);
        let _dst_layout = PixelLayout::from_descriptor(active_mode);

        // TODO: Properly implement pixel blit
        if active_mode.bits_per_pixel == buffer.bits_per_pixel {
            let Some(bytes_per_pixel) = active_mode.bits_per_pixel.div_exact(8) else {
                todo!("UefiFramebuffer::blit_buffer doesn't support non-byte-aligned PixelLayouts");
            };

            for src_y in 0..buffer.height {
                let src_offs = src_y * dst_pitch;
                let dst_offs =
                    (offset.y + src_y) * dst_pitch + offset.x * usize::from(bytes_per_pixel);
                let length = buffer.width * usize::from(bytes_per_pixel);

                unsafe {
                    let src_ptr = buffer.data.as_ref().as_ptr().add(src_offs);
                    let dst_ptr = active_mode.address.add(dst_offs).as_ptr();

                    core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, length);
                }
            }
        }
    }
}

#[macro_export]
macro_rules! print {
    ($($args:tt)*) => {$crate::framebuffer::print(format_args!($($args)*))};
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };
    ($($args:tt)*) => {
        $crate::print!("{}\n", format_args!($($args)*));
    };
}

#[allow(clippy::cast_precision_loss)]
const fn lossy_usize_to_f32(value: usize) -> f32 {
    value as f32
}

/// Returns `f32::floor` as an integer
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn downcast_f32_to_usize(value: f32) -> usize {
    value.floor() as usize
}

// ASCII escape codes only support 24-bit color
type Color = rgb::Rgb<u8>;

#[derive(Debug, Clone, Copy)]
struct CharCell {
    char: char,

    foreground: Color,
    background: Color,
}

impl Default for CharCell {
    fn default() -> Self {
        Self {
            char:       ' ',
            foreground: Color::new(u8::MAX, u8::MAX, u8::MAX),
            background: Color::new(0, 0, 0),
        }
    }
}

struct KernelLog {
    cells: Box<[CharCell]>,
    dirty: BitBox,

    size:   (usize, usize),
    cursor: (usize, usize),

    framebuffer:   PixelBuffer<Box<[u8]>>,
    ppem:          f32,
    should_render: bool,
}

/// Derived from [Alpha Blending with No Division Operations](https://arxiv.org/pdf/2202.02864)
#[allow(clippy::cast_possible_truncation)]
#[inline]
pub const fn mul_channel_alpha(
    value: u8,
    alpha: u8,
) -> u8 {
    let alpha = alpha as u32;
    let value = value as u32;

    // Return value is guarenteed to be within bounds of u8
    let result = value * alpha + 0x80;
    ((result + (result >> 8)) >> 8) as u8
}

pub const fn premul_rgba(value: Rgba<u8>) -> Rgba<u8> {
    let alpha = value.a;
    Rgba {
        r: mul_channel_alpha(value.r, alpha),
        g: mul_channel_alpha(value.g, alpha),
        b: mul_channel_alpha(value.b, alpha),
        a: alpha,
    }
}

/// Blends `above` over `below`. Expects premultiplied colors
#[inline]
pub const fn blend_rgba(
    above: Rgba<u8>,
    below: Rgba<u8>,
) -> Rgba<u8> {
    let inv_alpha = u8::MAX - above.a;

    Rgba {
        r: above
            .r
            .saturating_add(mul_channel_alpha(below.r, inv_alpha)),
        g: above
            .g
            .saturating_add(mul_channel_alpha(below.g, inv_alpha)),
        b: above
            .b
            .saturating_add(mul_channel_alpha(below.b, inv_alpha)),
        a: above
            .a
            .saturating_add(mul_channel_alpha(below.a, inv_alpha)),
    }
}

impl KernelLog {
    pub fn new<T: AsRef<[u8]>>(
        target: &PixelBuffer<T>,
        ppem: f32,
    ) -> Self {
        // Compute glyph width/height
        let font = FontRef::from_index(JETBRAINS_MONO, 0)
            .expect("Console font doesn't have anything at index 0");
        let space_gid = font.charmap().map(' ');
        let m_gid = font.charmap().map('M');

        let metrics = font.glyph_metrics(&[]).scale(ppem);
        assert!(
            (metrics.advance_width(m_gid) - metrics.advance_width(space_gid)).abs() <= 0.01,
            "Provided terminal font is not Monospace!"
        );

        let num_cells_width = downcast_f32_to_usize(
            lossy_usize_to_f32(target.width) / metrics.advance_width(space_gid),
        );
        let num_cells_height = downcast_f32_to_usize(
            lossy_usize_to_f32(target.height) / metrics.advance_height(space_gid),
        );

        let dirty = BitBox::from_boxed_slice(unsafe {
            Box::new_zeroed_slice(num_cells_width * num_cells_height).assume_init()
        });

        Self {
            cells: unsafe {
                let mut cells = Box::new_zeroed_slice(num_cells_width * num_cells_height);
                cells.fill(MaybeUninit::new(CharCell::default()));
                cells.assume_init()
            },
            dirty,
            cursor: (0, 0),
            size: (num_cells_width, num_cells_height),
            ppem,
            framebuffer: PixelBuffer {
                data: unsafe {
                    Box::new_zeroed_slice(target.width * target.height * 4).assume_init()
                },

                width:  target.width,
                height: target.height,
                pitch:  target.width * 4,

                pixel_layout:   PixelLayout::Rgb {
                    red_mask:   (0..8).into(),
                    green_mask: (8..16).into(),
                    blue_mask:  (16..24).into(),
                },
                bits_per_pixel: 32,
            },
            should_render: false,
        }
    }

    fn wipe_region(
        &mut self,
        start: (usize, usize),
        end: (usize, usize),
    ) {
        // TODO: Add bounds check
        let (width, _height) = self.size;

        let mut cursor = start;
        while cursor != end {
            let (x, y) = cursor;
            self.cells[y * width + x] = CharCell::default();
            self.dirty.set(y * width + x, true);

            if x == width {
                cursor = (0, y + 1);
            } else {
                cursor = (x + 1, y);
            }
        }
    }

    pub fn scroll_up(
        &mut self,
        amount: isize,
    ) {
        // TODO: Add bounds check
        let (width, height) = self.size;

        if amount > 0 {
            let amount = amount.cast_unsigned();
            self.cells
                .copy_within((width * amount)..(width * (height - amount)), 0);
            self.cells[(width * (height - 1))..].fill(CharCell::default());
        } else {
            let amount = amount.abs().cast_unsigned();
            self.cells
                .copy_within(0..(width * (height - amount)), width * amount);
            self.cells[0..(width * amount)].fill(CharCell::default());
        }

        self.dirty.fill(true);
    }

    fn set_cursor_mode(
        &mut self,
        mode: &SelectGraphicRendition,
    ) {
        let (cursor_x, cursor_y) = self.cursor;
        let (width, _height) = self.size;
        let Some(cell) = self.cells.get_mut(cursor_y * width + cursor_x) else {
            return;
        };

        match mode {
            SelectGraphicRendition::Background(color) => {
                cell.background = color.as_rgb(Color::new(0, 0, 0));
            },
            SelectGraphicRendition::Foreground(color) => {
                cell.foreground = color.as_rgb(Color::new(255, 255, 255));
            },
            SelectGraphicRendition::Reset => {
                *cell = CharCell {
                    char: cell.char,
                    ..CharCell::default()
                }
            },
            _ => {}, // Not Yet Implemented
        }

        self.dirty.set(cursor_y * width + cursor_x, true);
    }

    fn flush_framebuffer(&mut self) {
        let (width, height) = self.size;
        let framebuffer: &mut [Rgba<u8>] = bytemuck::cast_slice_mut(&mut self.framebuffer.data);

        let font =
            FontRef::from_index(JETBRAINS_MONO, 0).expect("No font at index 0 in JETBRAINS_MONO");
        let mut context = ScaleContext::new();
        let mut scaler = context.builder(font).hint(true).size(self.ppem).build();

        let mut render = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(swash::scale::StrikeWith::BestFit),
            Source::Outline,
        ]);
        render.format(swash::zeno::Format::Alpha);

        for cell_index in self.dirty.iter_ones() {
            let Some(cell) = self.cells.get_mut(cell_index) else {
                continue;
            };

            let cell_y = cell_index.div_floor(width);
            let cell_x = cell_index.rem_euclid(width);

            let cell_width = self.framebuffer.width / width;
            let cell_height = self.framebuffer.height / height;

            // Fill cell with background color
            let background_color = premul_rgba(cell.background.with_alpha(u8::MAX));

            for pixel_y in 0..cell_height {
                for pixel_x in 0..cell_width {
                    let x = (cell_width * cell_x) + pixel_x;
                    let y = (cell_height * cell_y) + pixel_y;

                    let pixel = &mut framebuffer[y * self.framebuffer.width + x];
                    *pixel = background_color;
                }
            }

            if cell.char == ' ' {
                continue;
            }

            let glyph_id = font.charmap().map(cell.char);

            // TODO: Glyph Cache
            let image = render
                .render(&mut scaler, glyph_id)
                .expect("Failed to render glyph");

            // TODO: Ensure this is properly handled
            #[allow(clippy::cast_possible_truncation)]
            let origin = font.metrics(&[]).scale(self.ppem).ascent as isize;

            let Ok(glyph_left) = isize::try_from(image.placement.left) else {
                unreachable!()
            };

            let Ok(glyph_top) = isize::try_from(image.placement.top) else {
                unreachable!()
            };

            let glyph_x = (cell_width * cell_x).saturating_add_signed(glyph_left);
            let glyph_y = (cell_height * cell_y).saturating_sub_signed(glyph_top - origin);

            match image.content {
                Content::SubpixelMask => unimplemented!("Swash returned unexpected SubpixelMask"),
                Content::Color => {
                    let glyph_width = image.placement.width as usize;
                    let row_size = glyph_width * 4;
                    for (pixel_y, row) in image.data.chunks_exact(row_size).enumerate() {
                        for (pixel_x, pixel) in row.chunks_exact(4).enumerate() {
                            let x = glyph_x + pixel_x;
                            let y = glyph_y + pixel_y;

                            let raw_color = *bytemuck::from_bytes(pixel);
                            let premul_fg = premul_rgba(raw_color);

                            let pixel = &mut framebuffer[y * self.framebuffer.width + x];
                            *pixel = blend_rgba(premul_fg, background_color);
                        }
                    }
                },
                Content::Mask => {
                    let glyph_width = image.placement.width as usize;
                    let glyph_height = image.placement.height as usize;

                    let mut i = 0;
                    let foreground_color = cell.foreground;
                    for pixel_y in 0..glyph_height {
                        for pixel_x in 0..glyph_width {
                            let x = glyph_x + pixel_x;
                            let y = glyph_y + pixel_y;

                            let alpha = image.data[i];
                            let raw_color = foreground_color.with_alpha(alpha);
                            let premul_fg = premul_rgba(raw_color);

                            let pixel = &mut framebuffer[y * self.framebuffer.width + x];
                            *pixel = blend_rgba(premul_fg, background_color);
                            i += 1;
                        }
                    }
                },
            }
        }

        self.should_render = false;
        self.dirty.fill(false);

        let Some(mut framebuffer) = FRAMEBUFFER.try_lock() else {
            return;
        };
        framebuffer.blit_buffer(&self.framebuffer, &Point2 { x: 0, y: 0 });
    }
}

const JETBRAINS_MONO: &[u8] =
    include_bytes!("../resources/JetBrains_Mono/JetBrainsMono-VariableFont_wght.ttf");

static KERNEL_LOG: spin::Mutex<Option<KernelLog>> = spin::Mutex::new(None);

const TAB_WIDTH: usize = 4;

#[allow(clippy::too_many_lines)] // FIXME
pub fn print(args: fmt::Arguments<'_>) {
    let mut kernel_log = KERNEL_LOG.lock();
    let kernel_log = kernel_log.get_or_insert_with(|| {
        let framebuffer_info = framebuffer_info();

        KernelLog::new(
            &PixelBuffer {
                data:           &[],
                pitch:          u64_to_usize(framebuffer_info.pitch),
                width:          u64_to_usize(framebuffer_info.width),
                height:         u64_to_usize(framebuffer_info.height),
                pixel_layout:   PixelLayout::from_descriptor(framebuffer_info),
                bits_per_pixel: framebuffer_info.bits_per_pixel,
            },
            16.0,
        )
    });

    let formatted = alloc::fmt::format(args);
    let (width, height) = kernel_log.size;

    let mut chars = formatted.char_indices();
    while let Some((index, char)) = chars.next() {
        match char {
            '\x07' => unimplemented!("Bell"), // Bell
            '\x0C' => {},                     // Form Feed
            '\x08' => {
                let (x, y) = kernel_log.cursor;
                kernel_log.cursor = (core::cmp::min(0, x - 1), y);
            }, // Backspace
            '\t' => {
                let (x, y) = kernel_log.cursor;
                kernel_log.cursor = (core::cmp::min(x.next_multiple_of(TAB_WIDTH), width - 1), y);
            }, // Tab
            '\n' => {
                let (_x, y) = kernel_log.cursor;
                if y + 1 >= height {
                    // Scroll console
                    kernel_log.scroll_up(1);
                }
                kernel_log.cursor = (0, core::cmp::min(y + 1, height - 1));
            }, // Line Feed
            '\r' => {
                let (_, y) = kernel_log.cursor;
                kernel_log.cursor = (0, y);
            }, // Carriage Return
            '\x1B' => {
                use core::cmp::min;

                let seq_start = &formatted[index..];
                let Ok((remain, seq)) = ansi::parse_control_sequence(seq_start) else {
                    continue;
                };
                _ = chars.advance_by(seq_start.len() - remain.len() - 1);

                match seq {
                    ControlSequence::DeviceStatus => unimplemented!("stdin doesn't exist"),
                    ControlSequence::CursorMoveDir { dir, amt } => {
                        let (x, y) = kernel_log.cursor;
                        match dir {
                            Direction::Up => kernel_log.cursor = (x, y.saturating_sub(amt)),
                            Direction::Down => kernel_log.cursor = (x, min(y + amt, height)),

                            Direction::Forward => kernel_log.cursor = (min(x + amt, width), y),
                            Direction::Back => kernel_log.cursor = (x.saturating_sub(amt), y),
                        }
                    },
                    ControlSequence::CursorMoveLine { dir, amt } => {
                        let (_x, y) = kernel_log.cursor;
                        match dir {
                            Direction::Forward | Direction::Back => unreachable!(),
                            Direction::Down => kernel_log.cursor = (0, min(y + amt, height)),
                            Direction::Up => kernel_log.cursor = (0, y.saturating_sub(amt)),
                        }
                    },
                    ControlSequence::CursorSet { x, y } => {
                        let (_old_x, old_y) = kernel_log.cursor;
                        kernel_log.cursor = (x, y.unwrap_or(old_y));
                    },
                    ControlSequence::EraseDisplay(region) => {
                        let start;
                        let end;

                        match region {
                            EraseRegion::CursorToEnd => {
                                start = kernel_log.cursor;
                                end = kernel_log.size;
                            },
                            EraseRegion::StartToCursor => {
                                start = (0, 0);
                                end = kernel_log.cursor;
                            },
                            EraseRegion::StartToEnd => {
                                start = (0, 0);
                                end = kernel_log.size;
                            },
                            EraseRegion::PrevToEnd => {
                                start = (0, 0);
                                end = kernel_log.size;

                                // TODO: Wipe Scrollback
                            },
                        }

                        kernel_log.wipe_region(start, end);
                    },
                    ControlSequence::EraseLine(region) => {
                        let start;
                        let end;

                        let (x, y) = kernel_log.cursor;
                        match region {
                            EraseRegion::CursorToEnd => {
                                start = kernel_log.cursor;
                                end = (width, y);
                            },
                            EraseRegion::StartToCursor => {
                                start = (0, y);
                                end = (x, y);
                            },
                            EraseRegion::StartToEnd => {
                                start = (0, y);
                                end = (width, y);
                            },
                            EraseRegion::PrevToEnd => unreachable!(),
                        }

                        kernel_log.wipe_region(start, end);
                    },
                    ControlSequence::Scroll { dir, amt } => {
                        let amount =
                            isize::try_from(amt).expect("Scroll amount is larger than isize::MAX");
                        let sign = match dir {
                            Direction::Up => 1,
                            Direction::Down => -1,
                            _ => unreachable!(),
                        };
                        kernel_log.scroll_up(amount * sign);
                    },
                    ControlSequence::Sgi(mode) => kernel_log.set_cursor_mode(&mode),
                }
            }, // ESC - Begin Sequence

            char => {
                let (cursor_x, cursor_y) = kernel_log.cursor;
                let cell_index = cursor_y * width + cursor_x;

                let Some(cell) = kernel_log.cells.get_mut(cell_index) else {
                    continue;
                };
                cell.char = char;

                let new_cell = CharCell { char: ' ', ..*cell };
                if let Some(next_cell) = kernel_log.cells.get_mut(cell_index + 1) {
                    *next_cell = new_cell;
                }

                kernel_log.dirty.set(cell_index, true);
                kernel_log.cursor = (core::cmp::min(width, cursor_x + 1), cursor_y);
            },
        }
    }

    kernel_log.should_render = true;
    kernel_log.flush_framebuffer();
}
