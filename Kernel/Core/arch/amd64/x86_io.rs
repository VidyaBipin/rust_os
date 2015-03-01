// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// arch/amd64/x86_io.rs
//! Support for x86's IO bus

/// Read a single byte
pub unsafe fn inb(port: u16) -> u8 {
	let ret : u8;
	asm!("inb %dx, %al" : "={ax}"(ret) : "{dx}"(port));
	return ret;
}
/// Write a single byte
pub unsafe fn outb(port: u16, val: u8) {
	asm!("outb %al, %dx" : : "{dx}"(port), "{al}"(val));
}

/// Read a 16-bit word
pub unsafe fn inw(port: u16) -> u16 {
	let ret : u16;
	asm!("inw %dx, %ax" : "={ax}"(ret) : "{dx}"(port));
	return ret;
}
/// Write a 16-bit word
pub unsafe fn outw(port: u16, val: u16) {
	asm!("outw %ax, %dx" : : "{dx}"(port), "{ax}"(val));
}

/// Read a 32-bit long/double-word
pub unsafe fn inl(port: u16) -> u32 {
	let ret : u32;
	asm!("inl %dx, %eax" : "={eax}"(ret) : "{dx}"(port));
	return ret;
}
/// Write a 32-bit long/double-word
pub unsafe fn outl(port: u16, val: u32) {
	asm!("outl %eax, %dx" : : "{dx}"(port), "{eax}"(val));
}

// vim: ft=rust

