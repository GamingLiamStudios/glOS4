use alloc::fmt::Debug;
use core::{
    ffi::c_void,
    ops::Deref,
    ptr::NonNull,
};

#[unsafe(link_section = ".limine_requests_start")]
#[used]
static REQUESTS_START: [u64; 4] = [
    0xf6b8_f4b3_9de7_d1ae,
    0xfab9_1a69_40fc_b9cf,
    0x785c_6ed0_15d3_e316,
    0x181e_920a_7852_b9d9,
];

#[unsafe(link_section = ".limine_requests_end")]
#[used]
static REQUESTS_END: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BaseRevision {
    magic: [u64; 2],
    ver:   u64,
}

impl BaseRevision {
    pub const fn new(ver: u64) -> Self {
        Self {
            magic: [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc],
            ver,
        }
    }

    pub fn is_supported(&self) -> bool {
        unsafe {
            let ver = core::ptr::read_volatile(&raw const self.ver);
            ver == 0
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FullResponse<T: Debug + Clone + Copy> {
    pub revision: u64,
    data:         T,
}

impl<T: Debug + Clone + Copy> Deref for FullResponse<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub trait ImplRequest {
    const ID: [u64; 2];
    const REVISION: u64;

    type Response: Debug + Clone + Copy;
}

#[repr(C)]
pub struct Request<T: ImplRequest> {
    id:       [u64; 4],
    revision: u64,

    pub response: *const FullResponse<T::Response>,
    data:         T,
}

impl<T: ImplRequest> Request<T> {
    pub const fn from(data: T) -> Self {
        Self {
            id: [
                0xc7b1_dd30_df4c_8b88,
                0x0a82_e883_a194_f07b,
                T::ID[0],
                T::ID[1],
            ],
            revision: T::REVISION,
            response: core::ptr::null(),
            data,
        }
    }

    pub unsafe fn response(&self) -> Option<&FullResponse<T::Response>> {
        unsafe {
            let self_ = core::ptr::read_volatile(self);
            self_.response.as_ref()
        }
    }
}

unsafe impl<T: ImplRequest> Sync for Request<T> {}

/*
pub struct BootloaderInfoResponse {
    name:    NonNull<CStr>,
    version: NonNull<CStr>,
}

#[repr(C)]
pub struct BootloaderInfo {}
impl ImplRequest for BootloaderInfo {
    type Response = BootloaderInfoResponse;

    const ID: [u64; 2] = [0xf550_38d8_e2a1_202f, 0x2794_26fc_f5f5_9740];
    const REVISION: u64 = 0;
}
*/

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FramebufferResponse {
    num_buffers: u64,
    buffers:     NonNull<NonNull<FramebufferDescriptor>>,
}

impl FramebufferResponse {
    pub fn as_slice(&self) -> &'static [NonNull<FramebufferDescriptor>] {
        let Ok(num_buffers) = usize::try_from(self.num_buffers) else {
            unreachable!("Limine only supports 64-bit systems")
        };
        unsafe { core::slice::from_raw_parts(self.buffers.as_ptr(), num_buffers) }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum FramebufferMemoryModel {
    Rgb = 1,
}

// TODO: Document
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FramebufferDescriptor {
    pub address: NonNull<u8>,

    pub width:          u64,
    pub height:         u64,
    pub pitch:          u64,
    pub bits_per_pixel: u16,

    pub memory_model: FramebufferMemoryModel,

    pub red_mask_size:  u8,
    pub red_mask_shift: u8,

    pub green_mask_size:  u8,
    pub green_mask_shift: u8,

    pub blue_mask_size:  u8,
    pub blue_mask_shift: u8,

    _unused: [u8; 7],

    pub edid_size: u64,
    pub edid:      Option<NonNull<c_void>>,
}

#[repr(C)]
pub struct FramebufferInfo {}
impl ImplRequest for FramebufferInfo {
    type Response = FramebufferResponse;

    const ID: [u64; 2] = [0x9d58_27dc_d881_dd75, 0xa314_8604_f6fa_b11b];
    const REVISION: u64 = 0; // TODO: Support rev1
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryMapResponse {
    entry_count: u64,
    entries:     NonNull<NonNull<MemoryMapEntry>>,
}
impl MemoryMapResponse {
    pub fn entries(&self) -> &'static [NonNull<MemoryMapEntry>] {
        let Ok(entry_count) = usize::try_from(self.entry_count) else {
            unreachable!("Limine only supports 64-bit systems")
        };
        unsafe { core::slice::from_raw_parts(self.entries.as_ptr(), entry_count) }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryMapEntry {
    pub base:   u64,
    pub length: u64,
    pub typ:    MemoryMapEntryType,
}

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapEntryType {
    Usable = 0,
    Reserved = 1,
    AcpiReclaimable = 2,
    AcpiNonVolatile = 3,
    BadMemory = 4,
    BootloaderReclaimable = 5,
    ExecutableAndModules = 6,
    Framebuffer = 7,
    AcpiTables = 8,
}

#[repr(C)]
pub struct MemoryMap {}
impl ImplRequest for MemoryMap {
    type Response = MemoryMapResponse;

    const ID: [u64; 2] = [0x67cf_3d9d_378a_806f, 0xe304_acdf_c50c_3c62];
    const REVISION: u64 = 0;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RdspResponse {
    pub address: NonNull<c_void>,
}

pub struct RdspRequest {}
impl ImplRequest for RdspRequest {
    type Response = RdspResponse;

    const ID: [u64; 2] = [0xc5e7_7b6b_397e_7b43, 0x2763_7845_accd_cf3c];
    const REVISION: u64 = 0;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HddmResponse {
    pub offset: u64,
}

pub struct HddmRequest {}
impl ImplRequest for HddmRequest {
    type Response = HddmResponse;

    const ID: [u64; 2] = [0x48dc_f1cb_8ad2_b852, 0x6398_4e95_9a98_244b];
    const REVISION: u64 = 0;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExecutableAddressResponse {
    pub physical_base: u64,
    pub virtual_base:  u64,
}

pub struct ExecutableAddress {}
impl ImplRequest for ExecutableAddress {
    type Response = ExecutableAddressResponse;

    const ID: [u64; 2] = [0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487];
    const REVISION: u64 = 0;
}
