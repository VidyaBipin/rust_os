// "Tifflin" Kernel
// - By John Hodge (thePowersGang)
//
// Core/irqs.rs
//! Core IRQ Abstraction
use crate::prelude::*;
use core::sync::atomic::AtomicBool;
use crate::arch::sync::Spinlock;
use crate::arch::interrupts;
use crate::lib::{VecMap};
use crate::lib::mem::Arc;

/// A handle for an IRQ binding that pokes an async event when the IRQ fires
pub struct EventHandle
{
	_binding: BindingHandle,
	event: Arc<crate::futures::flag::SingleFlag>,
}
pub struct ObjectHandle( #[allow(dead_code)] BindingHandle );

struct BindingHandle(u32, u32);

#[derive(Default)]
struct IRQBinding
{
	#[allow(dead_code)]
	arch_handle: interrupts::IRQHandle,
	has_fired: AtomicBool,	// Set to true if the IRQ fires while the lock is held by this CPU
	//handlers: Spinlock<Queue<Handler>>,
	handlers: Spinlock<Vec<Box<dyn FnMut()->bool + Send + 'static>>>,
}

struct Bindings
{
	mapping: VecMap<u32, Box<IRQBinding>>,
	next_index: usize,
}

// Notes:
// - Store a map of interrupt IDs against 
// - Hand out 'Handle' structures containing a pointer to the handler on that queue?
// - Per IRQ queue of
/// Map of IRQ numbers to core's dispatcher bindings. Bindings are boxed so the address is known in the constructor
static S_IRQ_BINDINGS: crate::sync::mutex::Mutex<Bindings> = crate::sync::mutex::Mutex::new(Bindings { mapping: VecMap::new(), next_index: 0 } );

// SAFE: The SleepObject here is static, so is never invalidated
static S_IRQ_WORKER_SIGNAL: crate::threads::SleepObject<'static> = unsafe { crate::threads::SleepObject::new("IRQ Worker") };
static S_TIMER_PENDING: AtomicBool = AtomicBool::new(false);
static S_IRQ_WORKER: crate::lib::LazyStatic<crate::threads::WorkerThread> = lazystatic_init!();

pub fn init() {
	// SAFE: Called in a single-threaded context? (Not fully conttrolled)
	S_IRQ_WORKER.prep(|| crate::threads::WorkerThread::new("IRQ Worker", irq_worker));
}

fn bind(num: u32, obj: Box<dyn FnMut()->bool + Send>) -> BindingHandle
{	
	log_trace!("bind(num={}, obj={:?})", num, "TODO"/*obj*/);
	// 1. (if not already) bind a handler on the architecture's handlers
	let mut map_lh = S_IRQ_BINDINGS.lock();
	let index = map_lh.next_index;
	map_lh.next_index += 1;
	let binding = match map_lh.mapping.entry(num)
		{
		crate::lib::vec_map::Entry::Occupied(e) => e.into_mut(),
		// - Vacant, create new binding (pokes arch IRQ clode)
		crate::lib::vec_map::Entry::Vacant(e) => e.insert( IRQBinding::new_boxed(num) ),
		};
	// 2. Add this handler to the meta-handler
	binding.handlers.lock().push( obj );
	
	BindingHandle( num, index as u32 )
}
impl Drop for BindingHandle
{
	fn drop(&mut self)
	{
		todo!("Drop IRQ binding handle: IRQ {} idx {}", self.0, self.1);
	}
}

fn irq_worker()
{
	loop {
		S_IRQ_WORKER_SIGNAL.wait();
		log_trace!("irq_worker: Wake");
		for (irqnum,b) in S_IRQ_BINDINGS.lock().mapping.iter()
		{
			if b.has_fired.swap(false, ::core::sync::atomic::Ordering::Relaxed)
			{
				log_trace!("irq_worker({:p}): IRQ{} fired", &**b, irqnum);
				if let Some(mut lh) = b.handlers.try_lock_cpu() {
					for handler in &mut *lh {
						handler();
					}
				}
			}
		}
		if S_TIMER_PENDING.swap(false, ::core::sync::atomic::Ordering::SeqCst)
		{
			crate::time::time_tick();
		}
	}
}

/// Function called by the architecture's timer irq (which will be off the worker) to trigger an IRQ
pub(super) fn timer_trigger()
{
	log_trace!("timer_trigger");
	S_TIMER_PENDING.store(true, ::core::sync::atomic::Ordering::SeqCst);
	S_IRQ_WORKER_SIGNAL.signal();
}

/// Bind an event waiter to an interrupt
pub fn bind_event(num: u32) -> EventHandle
{
	let ev = Arc::new( crate::futures::flag::SingleFlag::new() );
	EventHandle {
		event: ev.clone(),
		_binding: bind(num, Box::new(move || { ev.trigger(); true })),
		//_binding: bind(num, Box::new(HandlerEvent { event: ev })),
		}
}

pub fn bind_object(num: u32, obj: Box<dyn FnMut()->bool + Send + 'static>) -> ObjectHandle
{
	ObjectHandle( bind(num, obj) )
}

impl IRQBinding
{
	fn new_boxed(num: u32) -> Box<IRQBinding>
	{
		let mut rv = Box::new( IRQBinding::default());
		assert!(num < 256, "{} < 256 failed", num);
		// TODO: Use a better function, needs to handle IRQ routing etc.
		// - In theory, the IRQ num shouldn't be a u32, instead be an opaque IRQ index
		//   that the arch code understands (e.g. value for PciLineA that gets translated into an IOAPIC line)
		let context = &*rv as *const IRQBinding as *const ();
		rv.arch_handle = match interrupts::bind_gsi(num as usize, IRQBinding::handler_raw, context)
			{
			Ok(v) => v,
			Err(e) => panic!("Unable to bind handler to GSI {}: {:?}", num, e),
			};
		rv
	}
	
	fn handler_raw(info: *const ())
	{
		// SAFE: 'info' pointer should be an IRQBinding instance
		unsafe {
			let binding_ref = &*(info as *const IRQBinding);
			binding_ref.handle();
		}
	}
	//#[req_safe(irq)]
	fn handle(&self)
	{
		// The CPU owns the lock, so we don't care about ordering
		self.has_fired.store(true, ::core::sync::atomic::Ordering::Relaxed);
		
		// TODO: Can this force a wakeup/switch-to the IRQ worker?
		S_IRQ_WORKER_SIGNAL.signal();
	}
}

impl EventHandle
{
	pub fn get_event(&self) -> &crate::futures::flag::SingleFlag
	{
		&*self.event
	}
}

