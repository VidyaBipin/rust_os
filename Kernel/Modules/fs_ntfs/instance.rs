/*
 */
use ::kernel::prelude::*;
use ::kernel::metadevs::storage::VolumeHandle;
use ::kernel::lib::mem::aref;
use crate::ondisk;
use crate::MftEntryIdx;

/// A wrapper around the instance, owned by the VFS layer
pub struct InstanceWrapper(aref::ArefInner<Instance>);
/// 
pub type InstanceRef = aref::ArefBorrow<Instance>;

pub struct Instance
{
	vol: ::block_cache::CachedVolume,
	mount_handle: ::vfs::mount::SelfHandle,
	cluster_size_blocks: usize,
	mft_record_size: usize,
	mft_data_attr: Option<(CachedMft,ondisk::AttrHandle)>,
	bs: ondisk::Bootsector,

	// A cache of loaded (open and just in-cache) MFT entries
	mft_cache: ::kernel::sync::RwLock<::kernel::lib::VecMap<u32, CachedMft>>,
}

impl Instance
{
	pub fn new(vol: VolumeHandle, bs: ondisk::Bootsector, mount_handle: ::vfs::mount::SelfHandle) -> ::vfs::Result<Box<InstanceWrapper>> {

		let cluster_size_bytes = bs.bytes_per_sector as usize * bs.sectors_per_cluster as usize;
		let cluster_size_blocks = cluster_size_bytes / vol.block_size();
		if cluster_size_bytes % vol.block_size() != 0 {
			log_error!("Unable to mount: cluster size ({:#x}) isn't a multiple of volume block size ({:#x})", cluster_size_bytes, vol.block_size());
			return Err(::vfs::Error::InconsistentFilesystem);
		}
		// Pre-calculate some useful values (cluster size, mft entry, ...)
		let mut instance = Instance {
			vol: ::block_cache::CachedVolume::new(vol),
			mount_handle,
			mft_data_attr: None,
			cluster_size_blocks,
			mft_record_size: bs.mft_record_size.get().to_bytes(cluster_size_bytes),
			bs,
			mft_cache: Default::default(),
			};
		log_debug!("cluster_size_blocks = {:#x}, mft_record_size = {:#x}", instance.cluster_size_blocks, instance.mft_record_size);
		if let None = new_mft_cache_ent(instance.mft_record_size) {
			log_error!("Unable to mount: MFT record size too large for internal hackery ({:#x})", instance.mft_record_size);
			return Err(::vfs::Error::InconsistentFilesystem);
		}
		// Locate the (optional) MFT entry for the MFT
		instance.mft_data_attr = ::kernel::futures::block_on(instance.get_attr(ondisk::MFT_ENTRY_SELF, ondisk::FileAttr::Data, ondisk::ATTRNAME_DATA, /*index*/0))?;

		// SAFE: ArefInner::new requires a stable pointer, and the immediate boxing does that
		Ok(unsafe { Box::new(InstanceWrapper(aref::ArefInner::new(instance))) })
	}
}
impl ::vfs::mount::Filesystem for InstanceWrapper
{
	fn root_inode(&self) -> ::vfs::node::InodeId {
		ondisk::MFT_ENTRY_ROOT.0 as _
	}
	fn get_node_by_inode(&self, inode_id: ::vfs::node::InodeId) -> Option<::vfs::node::Node> {
		let ent = match ::kernel::futures::block_on(self.0.get_mft_entry(MftEntryIdx(inode_id as _)))
			{
			Err(_) => return None,
			Ok(v) => v,
			};
		todo!("get_node_by_inode({})", inode_id)
	}
}

/**
 * MFT entries and attributes
 */
/*
 * TODO: Store loaded MFT entries in a pool/cache?
 */
impl Instance
{
	pub async fn get_mft_entry(&self, entry: MftEntryIdx) -> ::vfs::Result<CachedMft> {
		// Look up in a cache, and return `Arc<RwLock`
		{
			let lh = self.mft_cache.read();
			if let Some(v) = lh.get(&entry.0) {
				return Ok(v.clone());
			}
		}
		let rv = self.load_mft_entry_dyn(entry).await?;
		{
			let mut lh = self.mft_cache.write();
			let rv = lh.entry(entry.0).or_insert(rv).clone();
			// TODO: Prune the cache?
			//lh.retain(|k,v| Arc::strong_count(v) > 1);
			Ok(rv)
		}
	}
	pub async fn with_mft_entry<R>(&self, entry: MftEntryIdx, cb: impl FnOnce(&ondisk::MftEntry)->::vfs::Result<R>) -> ::vfs::Result<R> {
		let mut cb = Some(cb);
		let mut rv = None;
		self.with_mft_entry_dyn(entry, &mut |e| Ok(rv = Some(cb.take().unwrap()(e)))).await?;
		rv.take().unwrap()
	}
	async fn with_mft_entry_dyn(&self, entry: MftEntryIdx, cb: &mut dyn FnMut(&ondisk::MftEntry)->::vfs::Result<()>) -> ::vfs::Result<()> {
		let e = self.get_mft_entry(entry).await?;
		let rv = cb(&e.inner.read());
		rv
	}

	/// Load a MFT entry from the disk (wrapper that avoids recursion with `attr_read`)
	fn load_mft_entry_dyn(&self, entry_idx: MftEntryIdx) -> ::core::pin::Pin<Box< dyn ::core::future::Future<Output=::vfs::Result<CachedMft>> + '_ >> {
		Box::pin(self.load_mft_entry(entry_idx))
	}
	/// Load a MFT entry from the disk
	async fn load_mft_entry(&self, entry_idx: MftEntryIdx) -> ::vfs::Result<CachedMft> {
		let entry_idx = entry_idx.0;
		// TODO: Check the index range
		let mut rv_bytes = new_mft_cache_ent(self.mft_record_size).expect("Unexpected record size");
		let buf = ::kernel::lib::mem::Arc::get_mut(&mut rv_bytes).unwrap().inner.get_mut();
		if let Some((ref mft_ent, ref e)) = self.mft_data_attr {
			// Read from the attribute
			self.attr_read(mft_ent, e, entry_idx as u64 * self.mft_record_size as u64, buf).await?;
		}
		else {
			if self.mft_record_size > self.vol.block_size() {
				let blocks_per_entry = self.mft_record_size / self.vol.block_size();
				let blk = self.bs.mft_start * self.cluster_size_blocks as u64 + (entry_idx as usize * blocks_per_entry) as u64;
				self.vol.read_blocks(blk, buf).await?;
			}
			else {
				let entries_per_block = (self.vol.block_size() / self.mft_record_size) as u32;
				let blk = self.bs.mft_start * self.cluster_size_blocks as u64 + (entry_idx / entries_per_block) as u64;
				let ofs = entry_idx % entries_per_block;
				self.vol.read_inner(blk, ofs as usize * self.mft_record_size, buf).await?;
			}
		}
		//Ok(ondisk::MftEntry::new_owned(buf))
		// SAFE: `MftEntry` and `[u8]` have the same representation
		Ok(unsafe { ::core::mem::transmute(rv_bytes) })
	}
	/// Get a hanle to an attribute within a MFT entry
	// TODO: How to handle invalidation of the attribute info when the MFT entry is updated (at least, when an attribute is resized)
	// - Could have attribute handles be indexes into a pre-populated list
	pub async fn get_attr(&self, entry: MftEntryIdx, attr_id: ondisk::FileAttr, name: &str, index: usize) -> ::vfs::Result<Option<(CachedMft, ondisk::AttrHandle)>> {
		// Get the MFT entry
		let e = self.get_mft_entry(entry).await?;
		let mft_ent = e.inner.read();
		// Iterate attributes
		let mut rv = None;
		let mut count = 0;
		for attr in mft_ent.iter_attributes() {
			log_debug!("get_attr: ty={:#x} name={:?}", attr.ty(), attr.name());
			if attr.ty() == attr_id as u32 && attr.name() == name {
				if count == index {
					rv = Some(mft_ent.attr_handle(attr, entry));
					break;
				}
				count += 1;
			}
		}
		drop(mft_ent);
		Ok(rv.map(|a| (e, a)))
	}

	pub async fn attr_read(&self, mft_ent: &CachedMft, attr: &ondisk::AttrHandle, ofs: u64, dst: &mut [u8]) -> ::vfs::Result<usize> {
		let mft_ent = mft_ent.inner.read();
		let a = mft_ent.get_attr(attr).ok_or(::vfs::Error::Unknown("Stale ntfs AttrHandle"))?;
		match a.inner()
		{
		ondisk::MftAttribData::Resident(r) => {
			todo!("Instance::attr_read - resident");
			},
		ondisk::MftAttribData::Nonresident(r) => {
			todo!("Instance::attr_read - Non-resident");
			},
		}
	}
}

type CachedMft = ::kernel::lib::mem::Arc< MftCacheEnt<ondisk::MftEntry> >;

struct MftCacheEnt<T: ?Sized> {
	count: ::core::sync::atomic::AtomicUsize,
	inner: ::kernel::sync::RwLock<T>,
}
impl<T> MftCacheEnt<T> {
	pub fn new(inner: T) -> Self {
		Self {
			count: Default::default(),
			inner: ::kernel::sync::RwLock::new(inner),
		}
	}
}
/// An evil hack to get a `Arc<Wrapper<MftEntry>>`
fn new_mft_cache_ent(mft_size: usize) -> Option< ::kernel::lib::mem::Arc<MftCacheEnt<[u8]>> > {
	use ::kernel::lib::mem::Arc;
	type I = Arc<MftCacheEnt<[u8]>>;
	let rv = match mft_size.next_power_of_two().ilog2()
		{
		0 ..= 4 |
		5  => { Arc::new(MftCacheEnt::new([0u8; 1<< 5])) as I },	// 32
		6  => { Arc::new(MftCacheEnt::new([0u8; 1<< 6])) as I },
		7  => { Arc::new(MftCacheEnt::new([0u8; 1<< 7])) as I },
		8  => { Arc::new(MftCacheEnt::new([0u8; 1<< 8])) as I },
		9  => { Arc::new(MftCacheEnt::new([0u8; 1<< 9])) as I },	// 512
		10 => { Arc::new(MftCacheEnt::new([0u8; 1<<10])) as I },
		11 => { Arc::new(MftCacheEnt::new([0u8; 1<<11])) as I },	// 2048
		_ => return None,
		};
	Some(rv)
}
