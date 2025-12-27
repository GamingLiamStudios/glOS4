#![feature(alloc_layout_extra, abi_x86_interrupt)]
#![allow(static_mut_refs)]
#![no_std]
#![no_main]

extern crate alloc;

use alloc::rc::Rc;
use core::{
    alloc::GlobalAlloc,
    cell::UnsafeCell,
    ptr::NonNull,
};

use acpi::{
    AcpiTables,
    AmlTable,
    aml::Interpreter,
    platform::AcpiPlatform,
};
use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        ascii::{
            FONT_6X10,
            FONT_10X20,
        },
    },
    pixelcolor::Rgb888,
    prelude::{
        Drawable,
        Point,
        WebColors,
    },
    text::Text,
};
use x86_64::{
    PhysAddr,
    VirtAddr,
    instructions::tlb::Pcid,
    registers::control::Cr3,
    structures::{
        paging::{
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
        port::{
            PortRead,
            PortWrite,
        },
    },
};

use crate::{
    framebuffer::FRAMEBUFFER,
    limine::MemoryMapEntryType,
};

mod framebuffer;
mod interrupts;
mod limine;

#[unsafe(link_section = ".limine_requests")]
#[used]
static BASE_REVISION: limine::BaseRevision = limine::BaseRevision::new(4);

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

fn usable_frames() -> impl Iterator<Item = PhysFrame> {
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
    let virt_addresses = frame_addresses.map(PhysAddr::new);

    virt_addresses.map(PhysFrame::containing_address)
}

struct BumpFrameAllocator {
    used_frames: usize,
}

unsafe impl FrameAllocator<Size4KiB> for BumpFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame = usable_frames().nth(self.used_frames);

        self.used_frames += 1;
        frame
    }
}

enum StaticPageTable {
    NotInitalized,
    Offset(OffsetPageTable<'static>),
}

impl Translate for StaticPageTable {
    fn translate(
        &self,
        addr: VirtAddr,
    ) -> x86_64::structures::paging::mapper::TranslateResult {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => page_table.translate(addr),
        }
    }
}

impl<S: PageSize> Mapper<S> for StaticPageTable
where
    OffsetPageTable<'static>: Mapper<S>,
{
    unsafe fn map_to_with_table_flags<A>(
        &mut self,
        page: Page<S>,
        frame: PhysFrame<S>,
        flags: PageTableFlags,
        parent_table_flags: PageTableFlags,
        frame_allocator: &mut A,
    ) -> Result<MapperFlush<S>, x86_64::structures::paging::mapper::MapToError<S>>
    where
        Self: Sized,
        A: FrameAllocator<Size4KiB> + ?Sized,
    {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => unsafe {
                page_table.map_to_with_table_flags(
                    page,
                    frame,
                    flags,
                    parent_table_flags,
                    frame_allocator,
                )
            },
        }
    }

    fn unmap(
        &mut self,
        page: Page<S>,
    ) -> Result<(PhysFrame<S>, MapperFlush<S>), x86_64::structures::paging::mapper::UnmapError>
    {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => page_table.unmap(page),
        }
    }

    unsafe fn update_flags(
        &mut self,
        page: Page<S>,
        flags: PageTableFlags,
    ) -> Result<MapperFlush<S>, x86_64::structures::paging::mapper::FlagUpdateError> {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => unsafe { page_table.update_flags(page, flags) },
        }
    }

    unsafe fn set_flags_p4_entry(
        &mut self,
        page: Page<S>,
        flags: PageTableFlags,
    ) -> Result<
        x86_64::structures::paging::mapper::MapperFlushAll,
        x86_64::structures::paging::mapper::FlagUpdateError,
    > {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => unsafe { page_table.set_flags_p4_entry(page, flags) },
        }
    }

    unsafe fn set_flags_p3_entry(
        &mut self,
        page: Page<S>,
        flags: PageTableFlags,
    ) -> Result<
        x86_64::structures::paging::mapper::MapperFlushAll,
        x86_64::structures::paging::mapper::FlagUpdateError,
    > {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => unsafe { page_table.set_flags_p3_entry(page, flags) },
        }
    }

    unsafe fn set_flags_p2_entry(
        &mut self,
        page: Page<S>,
        flags: PageTableFlags,
    ) -> Result<
        x86_64::structures::paging::mapper::MapperFlushAll,
        x86_64::structures::paging::mapper::FlagUpdateError,
    > {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => unsafe { page_table.set_flags_p2_entry(page, flags) },
        }
    }

    fn translate_page(
        &self,
        page: Page<S>,
    ) -> Result<PhysFrame<S>, x86_64::structures::paging::mapper::TranslateError> {
        match self {
            Self::NotInitalized => panic!("StaticPageTable not initalized!"),
            Self::Offset(page_table) => page_table.translate_page(page),
        }
    }
}

static mut PAGE_TABLE: StaticPageTable = StaticPageTable::NotInitalized;
static mut FRAME_ALLOCATOR: BumpFrameAllocator = BumpFrameAllocator { used_frames: 0 };

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
    pub const fn new(virt_start: VirtAddr) -> Self {
        Self {
            head:  virt_start,
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

        let Ok(align) = u64::try_from(layout.align()) else {
            unreachable!()
        };

        let prev_head = allocator.head;
        allocator.head = allocator.head.align_up(align);

        let offset = allocator.head.as_u64() - prev_head.as_u64();
        while allocator.avail <= layout.size() + offset as usize {
            let Ok(avail) = u64::try_from(allocator.avail) else {
                unreachable!()
            };

            let page: Page<Size4KiB> = Page::containing_address(allocator.head + avail);
            let frame = unsafe { FRAME_ALLOCATOR.allocate_frame().expect("OOM!") };

            unsafe {
                PAGE_TABLE
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::WRITABLE
                            | PageTableFlags::PRESENT
                            | PageTableFlags::USER_ACCESSIBLE,
                        &mut FRAME_ALLOCATOR,
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
        allocator.avail -= layout.size() + offset as usize;

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
static mut ALLOCATOR: Locked<BumpAllocator> =
    Locked::new(BumpAllocator::new(VirtAddr::new(0x8000_0000)));

struct AcpiHandlerImpl {
    virt_start: VirtAddr,
}

#[derive(Clone)]
struct AcpiHandler {
    inner: Rc<UnsafeCell<AcpiHandlerImpl>>,
}

impl AcpiHandler {
    pub fn new(inner: AcpiHandlerImpl) -> Self {
        Self {
            inner: Rc::new(UnsafeCell::new(inner)),
        }
    }

    pub fn read_io_generic<T: PortRead>(port: u16) -> T {
        let mut port = x86_64::instructions::port::Port::new(port);
        unsafe { port.read() }
    }

    pub fn write_io_generic<T: PortWrite>(
        port: u16,
        value: T,
    ) {
        let mut port = x86_64::instructions::port::Port::new(port);
        unsafe { port.write(value) }
    }
}

impl acpi::Handler for AcpiHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        let handler = unsafe { &mut *self.inner.get() };

        let Ok(phys_addr) = u64::try_from(physical_address) else {
            unreachable!()
        };

        let Ok(size_u64) = u64::try_from(size) else {
            unreachable!()
        };

        let target: PhysFrame<Size4KiB> = PhysFrame::containing_address(PhysAddr::new(phys_addr));
        let offset = phys_addr - target.start_address().as_u64();

        let mapped_bytes = (size_u64 + offset).next_multiple_of(4096);

        let virt_start = &mut handler.virt_start;
        let virt = *virt_start; // Assumes already page aligned
        *virt_start += mapped_bytes;

        for page_offset in 0..mapped_bytes / 4096 {
            let page = Page::containing_address(virt + page_offset * 4096);
            let frame = target + page_offset;

            unsafe {
                PAGE_TABLE
                    .map_to(
                        page,
                        frame,
                        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                        &mut FRAME_ALLOCATOR,
                    )
                    .expect("Uh oh.")
                    .flush();
            }
        }

        let Ok(virtual_address) = usize::try_from(virt.as_u64()) else {
            unreachable!()
        };

        let Ok(mapped_bytes) = usize::try_from(mapped_bytes) else {
            unreachable!()
        };

        // Map frame that contains physical_address
        acpi::PhysicalMapping {
            physical_start: physical_address,
            virtual_start:  unsafe { NonNull::new_unchecked(virtual_address as *mut _) },
            region_length:  size,
            mapped_length:  mapped_bytes,
            handler:        self.clone(),
        }
    }

    fn unmap_physical_region<T>(_region: &acpi::PhysicalMapping<Self, T>) {
        // For now; don't worry bout it :3
    }

    fn read_u8(
        &self,
        address: usize,
    ) -> u8 {
        unsafe {
            self.map_physical_region(address, size_of::<u8>())
                .virtual_start
                .read()
        }
    }

    fn read_u16(
        &self,
        address: usize,
    ) -> u16 {
        unsafe {
            self.map_physical_region(address, size_of::<u16>())
                .virtual_start
                .read()
        }
    }

    fn read_u32(
        &self,
        address: usize,
    ) -> u32 {
        unsafe {
            self.map_physical_region(address, size_of::<u32>())
                .virtual_start
                .read()
        }
    }

    fn read_u64(
        &self,
        address: usize,
    ) -> u64 {
        unsafe {
            self.map_physical_region(address, size_of::<u64>())
                .virtual_start
                .read()
        }
    }

    fn write_u8(
        &self,
        address: usize,
        value: u8,
    ) {
        unsafe {
            self.map_physical_region(address, size_of::<u8>())
                .virtual_start
                .write(value);
        }
    }

    fn write_u16(
        &self,
        address: usize,
        value: u16,
    ) {
        unsafe {
            self.map_physical_region(address, size_of::<u16>())
                .virtual_start
                .write(value);
        }
    }

    fn write_u32(
        &self,
        address: usize,
        value: u32,
    ) {
        unsafe {
            self.map_physical_region(address, size_of::<u32>())
                .virtual_start
                .write(value);
        }
    }

    fn write_u64(
        &self,
        address: usize,
        value: u64,
    ) {
        unsafe {
            self.map_physical_region(address, size_of::<u64>())
                .virtual_start
                .write(value);
        }
    }

    fn read_io_u8(
        &self,
        port: u16,
    ) -> u8 {
        Self::read_io_generic(port)
    }

    fn read_io_u16(
        &self,
        port: u16,
    ) -> u16 {
        Self::read_io_generic(port)
    }

    fn read_io_u32(
        &self,
        port: u16,
    ) -> u32 {
        Self::read_io_generic(port)
    }

    fn write_io_u8(
        &self,
        port: u16,
        value: u8,
    ) {
        Self::write_io_generic(port, value);
    }

    fn write_io_u16(
        &self,
        port: u16,
        value: u16,
    ) {
        Self::write_io_generic(port, value);
    }

    fn write_io_u32(
        &self,
        port: u16,
        value: u32,
    ) {
        Self::write_io_generic(port, value);
    }

    fn read_pci_u8(
        &self,
        address: acpi::PciAddress,
        offset: u16,
    ) -> u8 {
        todo!()
    }

    fn read_pci_u16(
        &self,
        address: acpi::PciAddress,
        offset: u16,
    ) -> u16 {
        todo!()
    }

    fn read_pci_u32(
        &self,
        address: acpi::PciAddress,
        offset: u16,
    ) -> u32 {
        todo!()
    }

    fn write_pci_u8(
        &self,
        address: acpi::PciAddress,
        offset: u16,
        value: u8,
    ) {
        todo!()
    }

    fn write_pci_u16(
        &self,
        address: acpi::PciAddress,
        offset: u16,
        value: u16,
    ) {
        todo!()
    }

    fn write_pci_u32(
        &self,
        address: acpi::PciAddress,
        offset: u16,
        value: u32,
    ) {
        todo!()
    }

    fn nanos_since_boot(&self) -> u64 {
        todo!()
    }

    fn stall(
        &self,
        microseconds: u64,
    ) {
        todo!()
    }

    fn sleep(
        &self,
        milliseconds: u64,
    ) {
        todo!()
    }

    fn create_mutex(&self) -> acpi::Handle {
        acpi::Handle(0)
    }

    fn acquire(
        &self,
        mutex: acpi::Handle,
        timeout: u16,
    ) -> Result<(), acpi::aml::AmlError> {
        Ok(())
    }

    fn release(
        &self,
        mutex: acpi::Handle,
    ) {
    }
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    unsafe {
        PAGE_TABLE = StaticPageTable::Offset(hddm_page_table());
        interrupts::initalize_idt();

        FRAMEBUFFER.init();
    }

    assert!(
        BASE_REVISION.is_supported(),
        "Base Revision is not Supported!"
    );

    println!("Hello World!");

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

        let rdsp_address = {
            let Some(rdsp_address) = RDSP_ADDRESS.response() else {
                panic!("Unable to obtain RDSP Address. No ACPI on sys?");
            };

            let Ok(rdsp_address) = u64::try_from(rdsp_address.address.addr().get()) else {
                unreachable!()
            };

            #[allow(static_mut_refs)]
            let addr_u64 = PAGE_TABLE
                .translate_addr(VirtAddr::new(rdsp_address))
                .expect("Wait what")
                .as_u64();

            let Ok(addr_usize) = usize::try_from(addr_u64) else {
                unreachable!()
            };

            addr_usize
        };

        let handler = AcpiHandler::new(
            (AcpiHandlerImpl {
                virt_start: VirtAddr::new(0x6000_0000),
            }),
        );

        let Ok(acpi_tables) = AcpiTables::from_rsdp(handler.clone(), rdsp_address) else {
            panic!("I really don't know what to do from here...");
        };
        let platform = AcpiPlatform::new(acpi_tables, handler).expect("aw :(");
        let interpreter = Interpreter::new_from_platform(&platform).expect("aw :((");
        interpreter.initialize_namespace();

        println!("{}", interpreter.namespace.lock());
    }

    /*
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
    */

    #[allow(clippy::empty_loop)]
    loop {}
}
