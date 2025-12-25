#![feature(alloc_layout_extra)]
#![no_std]
#![no_main]

use core::{
    alloc::{
        GlobalAlloc,
        Layout,
    },
    cell::UnsafeCell,
};

use lazy_static::lazy_static;
use x86_64::{
    PhysAddr,
    VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator,
        Mapper,
        OffsetPageTable,
        Page,
        PageSize,
        PageTable,
        PageTableFlags,
        PhysFrame,
        Size4KiB,
        Translate,
        mapper::MapperFlush,
    },
};

use crate::limine::MemoryMapEntryType;

mod limine;

#[unsafe(link_section = ".limine_requests")]
#[used]
static BASE_REVISION: limine::BaseRevision = limine::BaseRevision::new(4);

#[unsafe(link_section = ".limine_requests")]
#[used]
static FRAMEBUFFER_INFO: limine::Request<limine::FramebufferInfo> =
    limine::Request::from(limine::FramebufferInfo {});

#[unsafe(link_section = ".limine_requests")]
#[used]
static MEMORY_MAP: limine::Request<limine::MemoryMap> = limine::Request::from(limine::MemoryMap {});

#[unsafe(link_section = ".limine_requests")]
#[used]
static RDSP_ADDRESS: limine::Request<limine::RdspRequest> =
    limine::Request::from(limine::RdspRequest {});

#[unsafe(link_section = ".limine_requests")]
#[used]
static HDDM_OFFSET: limine::Request<limine::HddmRequest> =
    limine::Request::from(limine::HddmRequest {});

#[unsafe(link_section = ".limine_requests")]
#[used]
static EXECUTABLE_ADDRESS: limine::Request<limine::ExecutableAddress> =
    limine::Request::from(limine::ExecutableAddress {});

unsafe fn hddm_page_table() -> OffsetPageTable<'static> {
    let Some(hddm_offset) = (unsafe { HDDM_OFFSET.response() }) else {
        panic!("Unable to get MemoryMap from Limine");
    };
    let phys_offset = VirtAddr::new(hddm_offset.offset);

    let (p4_addr, _) = Cr3::read();
    let phys = p4_addr.start_address();
    let virt = phys_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { OffsetPageTable::new(&mut *page_table_ptr, phys_offset) }
}

fn usable_frames(page_table: &OffsetPageTable) -> impl Iterator<Item = PhysFrame> {
    let Some(memory_map) = (unsafe { MEMORY_MAP.response() }) else {
        panic!("Unable to get MemoryMap from Limine");
    };
    let entries = memory_map
        .entries()
        .iter()
        .map(|entry| unsafe { entry.as_ref() });

    let usable = entries.filter(|entry| entry.typ == MemoryMapEntryType::Usable);
    let regions = usable.map(|entry| entry.base..entry.base + entry.length);
    let frame_addresses = regions.flat_map(|region| region.step_by(4096));
    let virt_addresses =
        frame_addresses.filter_map(|addr| page_table.translate_addr(VirtAddr::new(addr)));

    virt_addresses.map(PhysFrame::containing_address)
}

struct PageAllocator {
    page_table:  OffsetPageTable<'static>,
    used_frames: usize,
}

unsafe impl FrameAllocator<Size4KiB> for PageAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = usable_frames(&self.page_table).nth(self.used_frames);
        self.used_frames += 1;
        frame
    }
}

lazy_static! {
    static ref PAGE_ALLOCATOR: Locked<PageAllocator> = Locked::new({
        let active_page_table = unsafe { hddm_page_table() };
        PageAllocator {
            page_table:  active_page_table,
            used_frames: 0,
        }
    });
}

/*
let page_table = &PAGE_ALLOCATOR.page_table;
        let Some(next_frame) = usable_frames(page_table).nth(PAGE_ALLOCATOR.used_frames) else {
            return Err(());
        };

        talc.get_allocated_span(heap)

        page_table.map_to(Page::containing_address(address), frame, flags, frame_allocator)
*/

pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Self {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
    }
}

// Basic Bump Allocator
struct BumpAllocator {
    head:  VirtAddr,
    avail: usize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            head:  VirtAddr::new(0x8000_0000),
            avail: 0,
        }
    }
}

unsafe impl GlobalAlloc for Locked<BumpAllocator> {
    unsafe fn alloc(
        &self,
        layout: core::alloc::Layout,
    ) -> *mut u8 {
        let mut allocator = self.lock();

        let Ok(head) = usize::try_from(allocator.head.as_u64()) else {
            unreachable!()
        };

        let Ok(align) = u64::try_from(layout.align()) else {
            unreachable!()
        };

        allocator.head = allocator.head.align_up(align);
        allocator.avail -= layout.padding_needed_for(head);

        while allocator.avail < layout.size() {
            let Ok(avail) = u64::try_from(allocator.avail) else {
                unreachable!()
            };

            let mut page_allocator = PAGE_ALLOCATOR.lock();

            let page: Page<Size4KiB> = Page::containing_address(allocator.head + avail);
            let frame = page_allocator.allocate_frame().expect("OOM!");

            // Incredibly unsafe to have both at the same time but :shrug:
            let mut active_page_table = unsafe { hddm_page_table() };
            unsafe {
                active_page_table
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::WRITABLE | PageTableFlags::PRESENT,
                        &mut *page_allocator,
                    )
                    .expect("uh what")
                    .flush();
            }

            allocator.avail += 4096;
        }

        let Ok(layout_size) = u64::try_from(layout.size()) else {
            unreachable!()
        };

        let alloc_head = allocator.head;
        allocator.head += layout_size;

        alloc_head.as_mut_ptr()
    }

    unsafe fn dealloc(
        &self,
        _ptr: *mut u8,
        _layout: core::alloc::Layout,
    ) {
        // Nothing required as no freeing occurs! How Optimal!
    }
}

#[global_allocator]
static ALLOCATOR: Locked<BumpAllocator> = Locked::new(BumpAllocator::new());

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    assert!(
        BASE_REVISION.is_supported(),
        "Base Revision is not Supported!"
    );

    unsafe {
        let Some(_memory_map) = MEMORY_MAP.response() else {
            panic!("Unable to get MemoryMap from Limine");
        };

        // Get Maximum Physical Address Width (M)
        let cpuid = raw_cpuid::CpuId::new();
        let capacity = cpuid
            .get_processor_capacity_feature_info()
            .expect("idk what do do here");
        let _address_bits = capacity.physical_address_bits();

        // TODO: Setup Paging outside of what was supplied via Limine HDDM
    }

    let framebuffer = unsafe { FRAMEBUFFER_INFO.response() };
    if let Some(data) = framebuffer {
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
