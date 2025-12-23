use core::{
    cell::UnsafeCell,
    ffi::{
        CStr,
        c_void,
    },
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

    #[allow(clippy::missing_const_for_fn)]
    pub fn is_supported(&self) -> bool {
        self.ver == 0
    }
}

pub struct FullResponse<T> {
    pub revision: u64,
    pub data:     T,
}

impl<T> Deref for FullResponse<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub trait ImplRequest {
    const ID: [u64; 2];
    const REVISION: u64;

    type Response;
}

#[repr(C)]
pub struct Request<T: ImplRequest> {
    id:       [u64; 4],
    revision: u64,

    response: UnsafeCell<*const FullResponse<T::Response>>,
    data:     T,
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
            response: UnsafeCell::new(core::ptr::null()),
            data,
        }
    }

    pub unsafe fn response(&self) -> Option<&FullResponse<T::Response>> {
        let ptr = self.response.get();
        unsafe { (*ptr).as_ref() }
    }
}

unsafe impl<T: ImplRequest> Sync for Request<T> {}

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

#[repr(C)]
pub struct FramebufferResponse {
    num_buffers: u64,
    buffers:     NonNull<NonNull<FramebufferDescriptor>>,
}

impl FramebufferResponse {
    pub fn as_slice(&self) -> &[NonNull<FramebufferDescriptor>] {
        let Ok(num_buffers) = usize::try_from(self.num_buffers) else {
            unreachable!("Limine only supports 64-bit systems")
        };
        unsafe { core::slice::from_raw_parts(self.buffers.as_ptr(), num_buffers) }
    }
}

#[repr(u8)]
pub enum FramebufferMemoryModel {
    Rgb = 1,
}

#[repr(C)]
pub struct FramebufferDescriptor {
    pub address: NonNull<c_void>,

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

#[derive(Default)]
#[repr(C)]
pub struct FramebufferInfo {}
impl ImplRequest for FramebufferInfo {
    type Response = FramebufferResponse;

    const ID: [u64; 2] = [0x9d58_27dc_d881_dd75, 0xa314_8604_f6fa_b11b];
    const REVISION: u64 = 0; // TODO: Support rev1
}
