// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Modules/fs_fat/dir.rs
use kernel::prelude::*;
use kernel::lib::mem::aref::ArefBorrow;
use kernel::vfs::{self, node};
use super::FilesystemInner;

const ERROR_SHORTCHAIN: vfs::Error = vfs::Error::Unknown("Cluster chain terminated early");

pub struct FileNode
{
	fs: ArefBorrow<FilesystemInner>,
	//parent_dir: u32,
	first_cluster: u32,
	size: u32,
}

impl FileNode
{
	pub fn new_boxed(fs: ArefBorrow<FilesystemInner>, _parent: u32, first_cluster: u32, size: u32) -> Box<FileNode> {	
		Box::new(FileNode {
			fs: fs,
			//parent_dir: parent,
			first_cluster: first_cluster,
			size: size,
			})
	}
}
impl node::NodeBase for FileNode {
	fn get_id(&self) -> node::InodeId {
		todo!("FileNode::get_id")
	}
	fn get_any(&self) -> &dyn core::any::Any {
		self
	}
}
impl node::File for FileNode {
	fn size(&self) -> u64 {
		self.size as u64
	}
	fn truncate(&self, newsize: u64) -> node::Result<u64> {
		let newsize: u32 = ::core::convert::TryFrom::try_from(newsize).unwrap_or(!0);
		if newsize < self.size {
			// Update size, and then deallocate clusters
			// - Challenge: Nothing stops the file being unlinked while it's still open. Need to ensure that this operation doesn't clobber anything
			//   if the directory is deallocated.
			// - Solution? Have a map controlled by `super::dir` that holds the parent cluster, allowing removal/update of the parent directory
			//let old_size = self.size;
			//super::dir::update_file_size(&self.fs, self.parent_dir, self.first_cluster, newsize as u32);
			//self.size = newsize;
			todo!("FileNode::truncate({:#x})", newsize);
		}
		else {
			// Allocate new clusters, then update the size
			// Update size iteratively, allocating clusters as needed
			todo!("FileNode::truncate({:#x})", newsize);
		}
	}
	fn clear(&self, ofs: u64, size: u64) -> node::Result<()> {
		todo!("FileNode::clear({:#x}+{:#x}", ofs, size);
	}
	fn read(&self, ofs: u64, buf: &mut [u8]) -> node::Result<usize> {
		// Sanity check and bound parameters
		if ofs > self.size as u64 {
			// out of range
			return Err( vfs::Error::InvalidParameter );
		}
		if ofs == self.size as u64 {
			return Ok(0);
		}
		let maxread = (self.size as u64 - ofs) as usize;
		let buf = if buf.len() > maxread { &mut buf[..maxread] } else { buf };
		let read_length = buf.len();
		log_trace!("read(@{:#x} len={:?})", ofs, read_length);
		
		// Seek to correct position in the cluster chain
		let mut clusters = super::ClusterList::chained(self.fs.reborrow(), self.first_cluster);
		for _ in 0 .. (ofs/self.fs.cluster_size as u64) {
			clusters.next();
		}
		let ofs = (ofs % self.fs.cluster_size as u64) as usize;
		
		// First incomplete cluster
		let mut cur_read_ofs = 0;
		/*let chunks = */if ofs != 0 {
				let Some(cluster) = clusters.next() else { return Err(ERROR_SHORTCHAIN); };
				let short_count = ::core::cmp::min(self.fs.cluster_size-ofs, buf.len());
				log_trace!("read(): Read partial head C{:#x} len={}", cluster, short_count);
				::kernel::futures::block_on(self.fs.with_cluster(cluster, |c| {
					buf[..short_count].clone_from_slice( &c[ofs..][..short_count] );
					}))?;
				
				cur_read_ofs += short_count;
				//buf[short_count..].chunks_mut(self.fs.cluster_size)
			}
			else {
				//buf.chunks_mut(self.fs.cluster_size)
			};

		while buf.len() - cur_read_ofs >= self.fs.cluster_size
		{
			let dst = &mut buf[cur_read_ofs..];
			let (cluster, count) = match clusters.next_extent( dst.len() / self.fs.cluster_size )
				{
				Some(v) => v,
				None => {
					log_notice!("Unexpected end of cluster chain at offset {}", cur_read_ofs);
					return Err(ERROR_SHORTCHAIN);
					},
				};
			let bytes = count * self.fs.cluster_size;
			log_trace!("read(): Read cluster extent C{:#x} + {}", cluster, count);
			::kernel::futures::block_on(self.fs.read_clusters_uncached(cluster, &mut dst[..bytes]))?;
			cur_read_ofs += bytes;
		}

		// Trailing sub-cluster data
		if buf.len() - cur_read_ofs > 0
		{
			let dst = &mut buf[cur_read_ofs..];
			let cluster = match clusters.next()
				{
				Some(v) => v,
				None => {
					log_notice!("Unexpected end of cluster chain at offset {}", cur_read_ofs);
					return Err(ERROR_SHORTCHAIN);
					},
				};
			log_trace!("read(): Read partial tail C{:#x} len={}", cluster, dst.len());
			::kernel::futures::block_on(self.fs.with_cluster(cluster, |c| {
				let bytes = dst.len();
				dst.clone_from_slice( &c[..bytes] );
			}))?
		}

		log_trace!("read(): Complete {}", read_length);
		Ok( read_length )
	}
	/// Write data to the file, can only grow the file if ofs==size
	fn write(&self, ofs: u64, buf: &[u8]) -> node::Result<usize> {
		todo!("FileNode::write({:#x}, {:p})", ofs, ::kernel::lib::SlicePtr(buf));
	}
}

