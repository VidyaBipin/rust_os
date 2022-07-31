
use ::usb_core::host;
use ::usb_core::host::{Handle,EndpointAddr};

mod device0;

pub struct UsbHost
{
    pub(crate) host: crate::HostRef,
}

impl host::HostController for UsbHost
{
	fn init_interrupt(&self, endpoint: EndpointAddr, period_ms: usize, max_packet_size: usize) -> Handle<dyn host::InterruptEndpoint> {
        todo!("");
	}
	fn init_isoch(&self, endpoint: EndpointAddr, max_packet_size: usize) -> Handle<dyn host::IsochEndpoint> {
		todo!("init_isoch({:?}, max_packet_size={})", endpoint, max_packet_size);
	}
	fn init_control(&self, endpoint: EndpointAddr, max_packet_size: usize) -> Handle<dyn host::ControlEndpoint> {
        if endpoint.dev_addr() == 0 {
            Handle::new(device0::Device0::new(self.host.clone(), max_packet_size))
                .ok().expect("Should fit")
        }
        else {
            todo!("");
        }
	}
	fn init_bulk_out(&self, endpoint: EndpointAddr, max_packet_size: usize) -> Handle<dyn host::BulkEndpointOut> {
        todo!("");
	}
	fn init_bulk_in(&self, endpoint: EndpointAddr, max_packet_size: usize) -> Handle<dyn host::BulkEndpointIn> {
        todo!("");
	}


	// Root hub maintainence
	fn set_port_feature(&self, port: usize, feature: host::PortFeature) {
        let p = self.host.regs.port(port as u8);
        let mask = match feature
            {
            host::PortFeature::Connection => 1 << 0,
            host::PortFeature::Enable   => 1 << 1,
            host::PortFeature::Suspend  => return,
            host::PortFeature::OverCurrent  => 1 << 3,
            host::PortFeature::Reset  => 1 << 4,
            host::PortFeature::Power  => 1 << 9,
            host::PortFeature::LowSpeed => return,
            host::PortFeature::CConnection => return,//1 << 17,
            host::PortFeature::CEnable => return,//1 << 18,
            host::PortFeature::CSuspend => return,
            host::PortFeature::COverCurrent => return,//1 << 20,
            host::PortFeature::CReset => return,//1 << 21,
            host::PortFeature::Test => return,
            host::PortFeature::Indicator => 2 << 14,
            };
        log_trace!("set_port_feature({},{:?}) {:#x}", port, feature, mask);
        p.set_sc(p.sc() | mask);
	}
	fn clear_port_feature(&self, port: usize, feature: host::PortFeature) {
        let p = self.host.regs.port(port as u8);
        let mask = match feature
            {
            host::PortFeature::Connection => 1 << 0,
            host::PortFeature::Enable   => 1 << 1,
            host::PortFeature::Suspend  => return,
            host::PortFeature::OverCurrent  => 1 << 3,
            host::PortFeature::Reset  => 1 << 4,
            host::PortFeature::Power  => 1 << 9,
            host::PortFeature::LowSpeed => return,
            host::PortFeature::CConnection => 1 << 17,
            host::PortFeature::CEnable => 1 << 18,
            host::PortFeature::CSuspend => return,
            host::PortFeature::COverCurrent => 1 << 20,
            host::PortFeature::CReset => 1 << 21,
            host::PortFeature::Test => return,
            host::PortFeature::Indicator => 3 << 14,
            };
        log_trace!("clear_port_feature({},{:?}) {:#x}", port, feature, mask);
        p.set_sc(p.sc() & !mask);
	}
	fn get_port_feature(&self, port: usize, feature: host::PortFeature) -> bool {
        let p = self.host.regs.port(port as u8);
        let mask = match feature
            {
            host::PortFeature::Connection => 1 << 0,
            host::PortFeature::Enable   => 1 << 1,
            host::PortFeature::Suspend  => return false,
            host::PortFeature::OverCurrent  => 1 << 3,
            host::PortFeature::Reset  => 1 << 4,
            host::PortFeature::Power  => 1 << 9,
            host::PortFeature::LowSpeed =>
                match (p.sc() >> 10) & 0xF
                {
                _ => return false,
                },
            host::PortFeature::CConnection => 1 << 17,
            host::PortFeature::CEnable => 1 << 18,
            host::PortFeature::CSuspend => return false,
            host::PortFeature::COverCurrent => 1 << 20,
            host::PortFeature::CReset => 1 << 21,
            host::PortFeature::Test => return false,
            host::PortFeature::Indicator => 3 << 14,
            };
        let rv = p.sc() & mask != 0;
        log_trace!("get_port_feature({}, {:?}): {} ({:#x})",  port, feature, rv, mask);
        rv
	}

	fn async_wait_root(&self) -> host::AsyncWaitRoot {
		struct AsyncWaitRoot {
            host: super::HostRef,
		}
		impl core::future::Future for AsyncWaitRoot {
			type Output = usize;
			fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context) -> core::task::Poll<Self::Output> {
                // Register for wake first
				*self.host.port_update_waker.lock() = cx.waker().clone();
                // Then check if there's a bit available
                if let Some(idx) = self.host.port_update.get_first_set_and_clear() {
                    //*self.host.port_update_waker.lock() = 
                    return core::task::Poll::Ready(idx);
                }
				core::task::Poll::Pending
			}
		}
		usb_core::host::AsyncWaitRoot::new(AsyncWaitRoot {
			host: self.host.reborrow(),
			}).ok().expect("Over-size task in `async_wait_root`")
	}
}

/// Create an `AsyncWaitIo` instance (boxes if required)
fn make_asyncwaitio<'a, T>(f: impl ::core::future::Future<Output=T> + Send + Sync + 'a) -> host::AsyncWaitIo<'a, T> {
    host::AsyncWaitIo::new(f)
        .unwrap_or_else(|v| host::AsyncWaitIo::new(
            ::kernel::lib::mem::boxed::Box::pin(v)).ok().unwrap()
            )
}