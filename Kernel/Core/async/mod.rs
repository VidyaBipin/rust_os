// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/async/mod.rs
/*! Asynchronous IO and waiting support.

 The Tifflin asynch IO model is based around waiter handlers that contain sufficient information
 to either sleep the thread, or poll for a condition.
*/
use prelude::*;

pub use self::mutex::Mutex;

pub mod mutex;
pub mod timer;
pub mod event;
pub mod queue;
pub mod poll;

/// Trait for primitive waiters
///
/// Primitive waiters are the lowest level async objects, mostly provided by this module
pub trait PrimitiveWaiter:
	::core::fmt::Debug
{
	/// Return true if the waiter is already complete (and signalled)
	fn is_complete(&self) -> bool;
	
	/// Polls the waiter, returning true if the event has triggered
	fn poll(&self) -> bool;
	/// Runs the completion handler
	fn run_completion(&mut self);
	/// Binds this waiter to signal the provided sleep object
	/// 
	/// Called before the completion handler
	fn bind_signal(&mut self, sleeper: &mut ::threads::SleepObject) -> bool;
	
	/// Unbind waiters from this sleep object
	fn unbind_signal(&mut self);
	
	/// 
	fn is_ready(&mut self) -> bool {
		if self.poll() {
			self.run_completion();
			true
		}
		else {
			false
		}
	}
}

/// A more generic waiter object, that can handle state transitions
pub trait Waiter:
	::core::fmt::Debug
{
	/// Returns true if the waiter is completed (i.e. waiting will do nothing)
	fn is_complete(&self) -> bool;
	
	/// Request a primitive wait object
	fn get_waiter(&mut self) -> &mut PrimitiveWaiter;
	/// Called when the wait returns
	///
	/// Return true to indicate that this waiter is complete
	fn complete(&mut self) -> bool;
}


impl<T: PrimitiveWaiter> Waiter for T {
	fn is_complete(&self) -> bool {
		self.is_complete()
	}
	fn get_waiter(&mut self) -> &mut PrimitiveWaiter {
		self
	}
	fn complete(&mut self) -> bool {
		true
	}
}


impl<'a> Waiter+'a
{
	/// Wait on a single wait object
	pub fn wait(&mut self)
	{
		while !self.is_complete()
		{
			log_debug!("Waiting on {:?}", self);
			let completed = {
				let prim = self.get_waiter();
				let mut obj = ::threads::SleepObject::new("wait_on_list");
				log_trace!("- bind");
				if prim.bind_signal( &mut obj ) {
					obj.wait();
				}
				else {
					while !prim.poll() {
						// TODO: Take a nap
					}
				}
				prim.unbind_signal();
				log_trace!("- sleep over");
				prim.is_ready()
				};
			log_trace!("completed = {}", completed);
			if completed {
				self.complete();
			}
		}
	}
}

/// Error type from wait_on_list
pub enum WaitError
{
	Timeout,
}

/// Wait on the provided list of Waiter trait objects
///
pub fn wait_on_list(waiters: &mut [&mut Waiter], timeout: Option<u64>) -> Option<usize>
{
	log_trace!("wait_on_list(waiters = {:?}, timeout = {:?})", waiters, timeout);
	if waiters.len() == 0
	{
		panic!("wait_on_list - Nothing to wait on");
	}
	
	if timeout.is_some() {
		todo!("Support timeouts in wait_on_list");
	}
	
	// Wait on primitives from the waiters, returning the indexes of those that need a state advance
	
	// - If there are no incomplete waiters, return None
	if waiters.iter().filter(|x| !x.is_complete()).count() == 0 {
		return None;
	}
	
	// - Create an object for them to signal
	let mut obj = ::threads::SleepObject::new("wait_on_list");
	let force_poll = waiters.iter_mut()
		.filter( |x| !x.is_complete() )
		.fold(false, |v,x| v | !x.get_waiter().bind_signal( &mut obj) )
		;
	
	if force_poll
	{
		log_trace!("- Polling");
		let mut n_passes = 0;
		// While none of the active waiters returns true from poll()
		while !waiters.iter_mut().filter(|x| !x.is_complete()).fold(false, |r,e| r || e.get_waiter().poll())
		{
			n_passes += 1;
			// TODO: Take a short nap
		}
		log_trace!("- Fire ({} passes)", n_passes);
	}
	else
	{
		// - Wait the current thread on that object
		log_trace!(" Sleeping");
		obj.wait();
	}
	
	for ent in waiters.iter_mut().filter(|x| !x.is_complete()) {
		ent.get_waiter().unbind_signal();
	}
	::core::mem::drop(obj);
	
	// Run completion handlers (via .is_ready and .complete), counting the number of changed waiters
	let mut n_complete = 0;
	for ent in waiters.iter_mut().filter(|x| !x.is_complete())
	{
		if ent.get_waiter().is_ready() && ent.complete()
		{
			n_complete += 1;
		}
	}
	Some( n_complete )
}

