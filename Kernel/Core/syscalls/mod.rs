// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/syscalls/mod.rs
/// Userland system-call interface
use prelude::*;

mod objects;

#[allow(raw_pointer_derive)]
#[derive(Debug)]
pub enum Error
{
	TooManyArgs,
	BadValue,
	InvalidBuffer(*const (), usize),
	InvalidUnicode(::core::str::Utf8Error),
}
impl From<::core::str::Utf8Error> for Error {
	fn from(v: ::core::str::Utf8Error) -> Self { Error::InvalidUnicode(v) }
}

/// Entrypoint invoked by the architecture-specific syscall handler
pub fn invoke(call_id: u32, args: &[usize]) -> u64 {
	match invoke_int(call_id, args)
	{
	Ok(v) => v,
	Err(e) => {
		log_log!("Syscall formatting error in call {:#x} - {:?}", call_id, e);
		!0
		},
	}
}

use self::values::*;
#[path="../../../syscalls.inc.rs"]
mod values;

fn error_code(value: u32) -> usize {
	value as usize + usize::max_value() / 2
}

fn invoke_int(call_id: u32, mut args: &[usize]) -> Result<u64,Error>
{
	if call_id & 1 << 31 == 0
	{
		// Unbound system call
		// - Split using 15/16 into subsystems
		Ok(match call_id
		{
		// === 0: Threads and core
		// - 0/0: Userland log
		CORE_LOGWRITE => {
			let msg = try!( <&str>::get_arg(&mut args) );
			syscall_core_log(msg); 0
			},
		// - 0/1: Exit process
		CORE_EXITPROCESS => {
			let status = try!( <u32>::get_arg(&mut args) );
			syscall_core_exit(status); 0
			},
		// - 0/2: Terminate current thread
		CORE_EXITTHREAD => {
			syscall_core_terminate(); 0
			},
		// - 0/3: Start thread
		CORE_STARTTHREAD => {
			let ip = try!( <usize>::get_arg(&mut args) );
			let sp = try!( <usize>::get_arg(&mut args) );
			syscall_core_newthread(sp, ip) as u64
			},
		// - 0/4: Start process
		CORE_STARTPROCESS => {
			let ip = try!( <usize>::get_arg(&mut args) );
			let sp = try!( <usize>::get_arg(&mut args) );
			let start = try!( <usize>::get_arg(&mut args) );
			let end   = try!( <usize>::get_arg(&mut args) );
			if start > end || end > ::arch::memory::addresses::USER_END {
				return Err( Error::BadValue );
			}
			syscall_core_newprocess(ip, sp, start, end) as u64
			},
		// === 1: Window Manager / GUI
		// - 1/0: New group (requires permission, has other restrictions)
		GUI_NEWGROUP => {
			let name = try!( <&str>::get_arg(&mut args) );
			syscall_gui_newgroup(name) as u64
			},
		// - 1/1: New window
		GUI_NEWWINDOW => {
			let name = try!( <&str>::get_arg(&mut args) );
			syscall_gui_newwindow(name) as u64
			},
		// === 2: VFS
		// - 2/0: Open node (for stat)
		VFS_OPENNODE => {
			todo!("VFS_OPENNODE");
			},
		// - 2/1: Open file
		VFS_OPENFILE => {
			let name = try!( <&[u8]>::get_arg(&mut args) );
			let mode = try!( <u32>::get_arg(&mut args) );
			(match syscall_vfs_openfile(name, mode)
			{
			Ok(v) => v,
			Err(v) => (1<<31)|v,
			} as u64)
			},
		// - 2/2: Open directory
		VFS_OPENDIR => {
			todo!("VFS_OPENDIR");
			},
		// === 3: Memory Mangement
		MEM_ALLOCATE => {
			let addr = try!(<usize>::get_arg(&mut args));
			let count = try!(<usize>::get_arg(&mut args));
			// Wait? Why do I have a 'mode' here?
			log_debug!("MEM_ALLOCATE({:#x},{})", addr, count);
			::memory::virt::allocate_user(addr as *mut (), count); 0
			//match ::memory::virt::allocate_user(addr as *mut (), count)
			//{
			//Ok(_) => 0,
			//Err(e) => todo!("MEM_ALLOCATE - error {:?}", e),
			//}
			},
		MEM_REPROTECT => {
			let addr = try!(<usize>::get_arg(&mut args));
			let mode = try!(<u8>::get_arg(&mut args));
			log_debug!("MEM_REPROTECT({:#x},{})", addr, mode);
			let mode = match mode
				{
				0 => ::memory::virt::ProtectionMode::UserRO,
				1 => ::memory::virt::ProtectionMode::UserRW,
				2 => ::memory::virt::ProtectionMode::UserRX,
				3 => ::memory::virt::ProtectionMode::UserRWX,	// TODO: Should this be disallowed?
				_ => return Err( Error::BadValue ),
				};
			// SAFE: This internally does checks, but is marked as unsafe as a signal
			match unsafe { ::memory::virt::reprotect_user(addr as *mut (), mode) }
			{
			Ok( () ) => 0,
			Err( () ) => error_code(0) as u64,
			}
			},
		MEM_DEALLOCATE => {
			let addr = try!(<usize>::get_arg(&mut args));
			todo!("MEM_DEALLOCATE({:#x})", addr)
			},
		// === *: Default
		_ => {
			log_error!("Unknown syscall {:05x}", call_id);
			0
			},
		})
	}
	else
	{
		const CALL_MASK: u32 = 0x7FF;
		let handle_id = (call_id >> 0) & 0xFFFFF;
		let call_id = (call_id >> 20) & CALL_MASK;	// Call in upper part, as it's constant on user-side
		// Method call
		// - Look up the object (first argument) and dispatch using registered methods
		
		// - Call method
		if call_id == CALL_MASK {
			// Destroy object
			objects::drop_object(handle_id);
			Ok(0)
		}
		else {
			// Call a method defined for the object class?
			objects::call_object(handle_id, call_id as u16, args)
		}
	}
}

type ObjectHandle = u32;

trait SyscallArg {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error>;
}

impl<'a> SyscallArg for &'a str {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 2 {
			return Err( Error::TooManyArgs );
		}
		let ptr = args[0] as *const u8;
		let len = args[1];
		*args = &args[2..];
		// TODO: Freeze the page to prevent the user from messing with it
		// SAFE: (uncheckable) lifetime of result should really be 'args, but can't enforce that
		let bs = unsafe {
			if let Some(v) = ::memory::buf_to_slice(ptr, len) {	
				v
			}
			else {
				return Err( Error::InvalidBuffer(ptr as *const (), len) );
			} };
		
		Ok(try!( ::core::str::from_utf8(bs) ))
	}
}
macro_rules! def_slice_get_arg {
	($t:ty) => {
		impl<'a> SyscallArg for &'a [$t] {
			fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
				if args.len() < 2 {
					return Err( Error::TooManyArgs );
				}
				let ptr = args[0] as *const $t;
				let len = args[1];
				*args = &args[2..];
				// TODO: Freeze the page to prevent the user from messing with it
				// SAFE: (uncheckable) lifetime of result should really be 'args, but can't enforce that
				unsafe {
					if let Some(v) = ::memory::buf_to_slice(ptr, len) {	
						Ok(v)
					}
					else {
						Err( Error::InvalidBuffer(ptr as *const (), len) )
					}
				}
			}
		}
	};
}
def_slice_get_arg!{u8}
def_slice_get_arg!{values::ProcessSegment}

impl<'a> SyscallArg for &'a mut [u8] {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 2 {
			return Err( Error::TooManyArgs );
		}
		let ptr = args[0] as *mut u8;
		let len = args[1];
		*args = &args[2..];
		// TODO: Freeze the page to prevent the user from messing with it
		// SAFE: (uncheckable) lifetime of result should really be 'args, but can't enforce that
		unsafe {
			if let Some(v) = ::memory::buf_to_slice_mut(ptr, len) {
				Ok(v)
			}
			else {
				Err( Error::InvalidBuffer(ptr as *const (), len) )
			}
		}
	}
}
impl SyscallArg for usize {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 1 {
			return Err( Error::TooManyArgs );
		}
		let rv = args[0];
		*args = &args[1..];
		Ok( rv )
	}
}
#[cfg(target_pointer_width="64")]
impl SyscallArg for u64 {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 1 {
			return Err( Error::TooManyArgs );
		}
		let rv = args[0] as u64;
		*args = &args[1..];
		Ok( rv )
	}
}
impl SyscallArg for u32 {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 1 {
			return Err( Error::TooManyArgs );
		}
		let rv = args[0] as u32;
		*args = &args[1..];
		Ok( rv )
	}
}
impl SyscallArg for u8 {
	fn get_arg(args: &mut &[usize]) -> Result<Self,Error> {
		if args.len() < 1 {
			return Err( Error::TooManyArgs );
		}
		let rv = args[0] as u8;
		*args = &args[1..];
		Ok( rv )
	}
}

fn syscall_core_log(msg: &str) {
	log_debug!("syscall_core_log - {}", msg);
}
fn syscall_core_exit(status: u32) {
	todo!("syscall_core_exit(status={:x})", status);
}
fn syscall_core_terminate() {
	todo!("syscall_core_terminate()");
}
fn syscall_core_newthread(sp: usize, ip: usize) -> ObjectHandle {
	todo!("syscall_core_newthread(sp={:#x},ip={:#x})", sp, ip);
}
fn syscall_core_newprocess(ip: usize, sp: usize, clone_start: usize, clone_end: usize) -> ObjectHandle {
	// 1. Create a new process image (virtual address space)
	let mut process = ::threads::ProcessHandle::new("TODO", clone_start, clone_end);
	// 3. Create a new thread using that process image with the specified ip/sp
	process.start_root_thread(ip, sp);
	
	struct Process(::threads::ProcessHandle);
	impl objects::Object for Process {
		fn handle_syscall(&self, call: u16, _args: &[usize]) -> Result<u64,Error> {
			match call
			{
			_ => todo!("Process::handle_syscall({}, ...)", call),
			}
		}
	}

	objects::new_object( Process(process) )
}

fn syscall_gui_newgroup(name: &str) -> ObjectHandle {
	todo!("syscall_gui_newgroup(name={})", name);
}
fn syscall_gui_newwindow(name: &str) -> ObjectHandle {
	todo!("syscall_gui_newwindow(name={})", name);
}

fn syscall_vfs_openfile(path: &[u8], mode: u32) -> Result<ObjectHandle,u32> {
	struct File(::vfs::handle::File);

	impl objects::Object for File {
		fn handle_syscall(&self, call: u16, mut args: &[usize]) -> Result<u64,Error> {
			match call
			{
			values::VFS_FILE_READAT => {
				let ofs = try!( <u64>::get_arg(&mut args) );
				let dest = try!( <&mut [u8]>::get_arg(&mut args) );
				log_debug!("File::readat({}, {:p}+{} bytes)", ofs, dest.as_ptr(), dest.len());
				match self.0.read(ofs, dest)
				{
				Ok(count) => Ok(count as u64),
				Err(e) => todo!("File::handle_syscall READAT Error {:?}", e),
				}
				},
			values::VFS_FILE_WRITEAT => {
				todo!("File::handle_syscall WRITEAT");
				},
			values::VFS_FILE_MEMMAP => {
				let ofs = try!( <u64>::get_arg(&mut args) );
				let size = try!( <usize>::get_arg(&mut args) );
				let addr = try!( <usize>::get_arg(&mut args) );
				let mode = match try!( <u8>::get_arg(&mut args) )
					{
					0 => ::vfs::handle::MemoryMapMode::ReadOnly,
					1 => ::vfs::handle::MemoryMapMode::Execute,
					2 => ::vfs::handle::MemoryMapMode::COW,
					3 => ::vfs::handle::MemoryMapMode::WriteBack,
					v @ _ => return Err( Error::BadValue ),
					};
				log_debug!("VFS_FILE_MEMMAP({:#x}, {:#x}+{}, {:?}", ofs, addr, size, mode);
				
				match self.0.memory_map(addr, ofs, size, mode)
				{
				Ok(h) => {
					log_warning!("TODO: register memory map handle with object table");
					::core::mem::forget(h);
					Ok(0)
					},
				Err(e) => todo!("File::handle_syscall MEMMAP Error {:?}", e),
				}
				},
			_ => todo!("File::handle_syscall({}, ...)", call),
			}
		}
	}
	
	let mode = match mode
		{
		1 => ::vfs::handle::FileOpenMode::SharedRO,
		2 => ::vfs::handle::FileOpenMode::Execute,
		_ => todo!("Unkown mode {:x}", mode),
		};
	match ::vfs::handle::File::open(::vfs::Path::new(path), mode)
	{
	Ok(h) => Ok( objects::new_object( File(h) ) ),
	Err(e) => todo!("syscall_vfs_openfile - e={:?}", e),
	}
}
