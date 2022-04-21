// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// arch/amd64/hw/hpet.rs
// - x86 High Precision Event Timer
#[allow(unused_imports)]
use crate::prelude::*;
use crate::arch::imp::acpi::AddressSpaceID;

module_define!{HPET, [APIC, ACPI], init}

struct HPET
{
	mapping_handle: crate::memory::virt::AllocHandle,
	#[allow(dead_code)]
	irq_handle: crate::arch::imp::hw::apic::IRQHandle,
	period: u64,
}

#[repr(C,packed)]
struct ACPI_HPET
{
	hw_rev_id: u8,
	flags: u8,
	pci_vendor: u16,
	addr: crate::arch::imp::acpi::GAS,
	hpet_num: u8,
	mintick: [u8; 2],	// 16-bit word
	page_protection: u8,
}

enum HPETReg
{
	CapsID  = 0x0,
	Config  = 0x1,
	ISR     = 0x2,
	MainCtr = 0xF,
	Timer0  = 0x10,
}

static S_INSTANCE: crate::lib::LazyStatic<HPET> = lazystatic_init!();

/// Reutrns the current system timestamp, in miliseconds since an arbitary point (usually power-on)
pub fn get_timestamp() -> u64
{
	if S_INSTANCE.ls_is_valid() {
		S_INSTANCE.current() / S_INSTANCE.ticks_per_ms()
	}
	else {
		0
	}
}

fn init()
{
	log_trace!("init()");
	let hpet = match crate::arch::imp::acpi::find::<ACPI_HPET>("HPET", 0)
		{
		None => {
			log_error!("No HPET, in ACPI, no timing avaliable");
			return ;
			},
		Some(v) => v,
		};

	let info = hpet.data();
	assert!(info.addr.asid == AddressSpaceID::Memory as u8);
	assert!(info.addr.address % crate::PAGE_SIZE as u64 == 0, "Address {:#x} not page aligned", { info.addr.address });
	// Assume SAFE: Shouldn't be sharing paddrs
	let mapping = unsafe { crate::memory::virt::map_hw_rw(info.addr.address, 1, "HPET").unwrap() };

	// HACK! Disable the PIT
	// - This should really be done by the ACPI code (after it determines the PIT exists)
	// SAFE: Nothing else attacks the PIT
	unsafe {
		crate::arch::x86_io::outb(0x43, 0<<7|3<<4|0);
		crate::arch::x86_io::outb(0x43, 1<<7|3<<4|0);
		crate::arch::x86_io::outb(0x43, 2<<7|3<<4|0);
		crate::arch::x86_io::outb(0x43, 3<<7|3<<4|0);
	}

	// SAFE: 'init' is called in a single-threaded context
	let inst = unsafe {
		S_INSTANCE.prep(|| HPET::new(mapping));
		S_INSTANCE.ls_unsafe_mut().bind_irq();
		&*S_INSTANCE
		};
	
	inst.oneshot(0, inst.current() + 100*1000 );
}

impl HPET
{
	pub fn new(mapping: crate::memory::virt::AllocHandle) -> HPET
	{
		let mut rv = HPET {
			mapping_handle: mapping,
			irq_handle: Default::default(),
			period: 1,
			};
		// Enable
		rv.write_reg(HPETReg::Config as usize, rv.read_reg(HPETReg::Config as usize) | (1 << 0));
		rv.period = rv.read_reg(HPETReg::CapsID as usize) >> 32;
		rv
	}
	pub fn bind_irq(&mut self)
	{
		self.irq_handle = crate::arch::imp::hw::apic::register_irq(2, HPET::irq, self as *mut _ as *const _).unwrap();
	}
	pub fn ticks_per_ms(&self) -> u64
	{
		// period = fempto (15) seconds per tick
		// Want ticks per ms
		// 
		1000*1000*1000*1000 / self.period
	}
	
	fn irq(sp: *const ())
	{
		// SAFE: Pointer associated should be an instance of HPET
		let s = unsafe{ &*(sp as *const HPET) };
		s.write_reg(HPETReg::ISR as usize, s.read_reg(HPETReg::ISR as usize));
		
		s.oneshot(0, s.current() + 100*1000 );
	}
	
	fn read_reg(&self, reg: usize) -> u64 {
		// SAFE: Hardware access, implicitly atomic on x86
		unsafe {
			::core::intrinsics::volatile_load( &(*self.regs())[reg*2] )
		}
	}
	fn write_reg(&self, reg: usize, val: u64) {
		// SAFE: Hardware access, implicitly atomic on x86
		unsafe {
			::core::intrinsics::volatile_store( &mut (*self.regs())[reg*2], val )
		}
	}
	fn regs(&self) -> *mut [u64; 0x100] {
		// SAFE: Coerces to raw pointer instantly
		unsafe { self.mapping_handle.as_int_mut(0) }
	}
	fn num_comparitors(&self) -> usize {
		((self.read_reg(HPETReg::CapsID as usize) >> 8) & 0x1F) as usize
	}
	
	fn current(&self) -> u64 {
		self.read_reg(HPETReg::MainCtr as usize)
	}
	fn oneshot(&self, comparitor: usize, value: u64) {
		assert!(comparitor < self.num_comparitors());
		let comp_reg = HPETReg::Timer0 as usize + comparitor*2;
		// Set comparitor value
		self.write_reg(comp_reg + 1, value);
		// HACK: Wire to APIC interrupt 2
		// IRQ2, Interrups Enabled, Level Triggered
		self.write_reg(comp_reg + 0, (2 << 9)|(1<<2)|(1<<1));
	}
}

// vim: ft=rust

