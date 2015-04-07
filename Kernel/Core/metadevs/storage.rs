// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/metadevs/storage.rs
// - Storage (block device) subsystem
use _common::*;
use core::atomic::{AtomicUsize,ATOMIC_USIZE_INIT};
use sync::mutex::LazyMutex;
use async::{ReadHandle,WriteHandle};
use lib::VecMap;

module_define!{Storage, [], init}

/// A unique handle to a storage volume (logical)
pub struct VolumeHandle
{
	lv_idx: usize,
}

pub struct PhysicalVolumeReg
{
	idx: usize,
}

/// Helper to print out the size of a volume/size as a pretty SI base 2 number
pub struct SizePrinter(pub u64);

/// Physical volume instance provided by driver
///
/// Provides the low-level methods to manipulate the underlying storage
pub trait PhysicalVolume
{
	/// Returns the volume name (must be unique to the system)
	fn name(&self) -> &str;	// Local lifetime string
	/// Returns the size of a filesystem block, must be a power of two >512
	fn blocksize(&self) -> usize;
	/// Returns the number of blocks in this volume (i.e. the capacity)
	fn capacity(&self) -> u64;
	
	/// Reads a number of blocks from the volume into the provided buffer
	///
	/// Reads `count` blocks starting with `blockidx` into the buffer `dst` (which will/should
	/// be the size of `count` blocks). The read is performed with the provided priority, where
	/// 0 is higest, and 255 is lowest.
	fn read<'a>(&'a self, prio: u8, blockidx: u64, count: usize, dst: &'a mut [u8]) -> Result<ReadHandle<'a,'a>, ()>;
	/// Writer a number of blocks to the volume
	fn write<'a>(&'a self, prio: u8, blockidx: u64, count: usize, src: &'a [u8]) -> Result<WriteHandle<'a,'a>,()>;
	/// Erases a number of blocks from the volume
	///
	/// Erases (requests the underlying storage forget about) `count` blocks starting at `blockidx`.
	/// This is functionally equivalent to the SSD "TRIM" command.
	fn wipe(&mut self, blockidx: u64, count: usize);
}

/// Registration for a physical volume handling driver
pub trait Mapper: Send + Sync
{
	fn name(&self) -> &str;
	fn handles_pv(&self, pv: &PhysicalVolume) -> usize;
}

/// A single logical volume, composed of 1 or more physical blocks
struct LogicalVolume
{
	block_size: usize,	///< Logical block size (max physical block size)
	region_size: Option<usize>,	///< Number of bytes in each physical region, None = JBOD
	regions: Vec<PhysicalRegion>,
}
/// Physical region used by a logical volume
struct PhysicalRegion
{
	volume: usize,
	block_count: usize,	// usize to save space in average case
	first_block: u64,
}

static S_NEXT_PV_IDX: AtomicUsize = ATOMIC_USIZE_INIT;
static S_PHYSICAL_VOLUMES: LazyMutex<VecMap<usize,Box<PhysicalVolume+Send>>> = lazymutex_init!();
static S_LOGICAL_VOLUMES: LazyMutex<VecMap<usize,LogicalVolume>> = lazymutex_init!();
static S_MAPPERS: LazyMutex<Vec<&'static Mapper>> = lazymutex_init!();

// TODO: Maintain a set of registered volumes. Mappers can bind onto a volume and register new LVs
// TODO: Maintain set of active mappings (set of PVs -> set of LVs)
// NOTE: Should unbinding of LVs be allowed? (Yes, for volume removal)

fn init()
{
	S_PHYSICAL_VOLUMES.init( || VecMap::new() );
	S_LOGICAL_VOLUMES.init( || VecMap::new() );
	S_MAPPERS.init( || Vec::new() );
}

/// Register a physical volume
pub fn register_pv(pv: Box<PhysicalVolume+Send>) -> PhysicalVolumeReg
{
	log_trace!("register_pv(pv = \"{}\")", pv.name());
	let pv_id = S_NEXT_PV_IDX.fetch_add(1, ::core::atomic::Ordering::Relaxed);

	// Now that a new PV has been inserted, handlers should be informed
	let mut best_mapper: Option<&Mapper> = None;
	let mut best_mapper_level = 0;
	let mappers = S_MAPPERS.lock();
	for &mapper in mappers.iter()
	{
		let level = mapper.handles_pv(&*pv);
		if level == 0
		{
			// Ignore (doesn't handle)
		}
		else if level < best_mapper_level
		{
			// Ignore (weaker handle)
		}
		else if level == best_mapper_level
		{
			// Fight!
			log_warning!("LV Mappers {} and {} are fighting over {}",
				mapper.name(), best_mapper.unwrap().name(), pv.name());
		}
		else
		{
			best_mapper = Some(mapper);
			best_mapper_level = level;
		}
	}
	if let Some(mapper) = best_mapper
	{
		// Poke mapper
		log_error!("TODO: Invoke mapper {} on volume {}", mapper.name(), pv.name());
		unimplemented!();
	}
	
	// Wait until after checking for a handler before we add the PV to the list
	S_PHYSICAL_VOLUMES.lock().insert(pv_id, pv);
	
	PhysicalVolumeReg { idx: pv_id }
}

/// Register a mapper with the storage subsystem
// TODO: How will it be unregistered?
pub fn register_mapper(mapper: &'static Mapper)
{
	S_MAPPERS.lock().push(mapper);
	
	// Check unbound PVs
	for (_id,pv) in S_PHYSICAL_VOLUMES.lock().iter()
	{
		let level = mapper.handles_pv(&**pv);
		if level == 0
		{
			// Ignore
		}
		else
		{
			log_error!("TODO: Mapper {} wants to handle volume {}", mapper.name(), pv.name());
		}
	}
}

/// Function called when a new volume is registered (physical or logical)
fn new_volume(volidx: usize)
{
}

pub fn enum_pvs() -> Vec<(usize,String)>
{
	S_PHYSICAL_VOLUMES.lock().iter().map(|(k,v)| (*k, String::from_str(v.name())) ).collect()
}

impl ::core::fmt::Display for SizePrinter
{
	fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result
	{
		if self.0 < 4096
		{
			write!(f, "{}B", self.0)
		}
		else if self.0 < 4096 * 1024
		{
			write!(f, "{}KiB", self.0/1024)
		}
		else if self.0 < 4096 * 1024 * 1024
		{
			write!(f, "{}MiB", self.0/(1024*1024))
		}
		else //if self.0 < 4096 * 1024 * 1024
		{
			write!(f, "{}GiB", self.0/(1024*1024*1024))
		}
	}
}

// vim: ft=rust