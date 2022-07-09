//! PCI binding for EHCI
use kernel::device_manager;

pub struct PciDriver;

impl device_manager::Driver for PciDriver {
	fn name(&self) -> &str {
		"ohci-pci"
	}
	fn bus_type(&self) -> &str {
		"pci"
	}
	fn handles(&self, bus_dev: &dyn device_manager::BusDevice) -> u32
	{
		let class = bus_dev.get_attr("class").unwrap_u32();
		if class & 0xFF_FF_FF_00 == 0x0C0320_00 { 
			1
		}
		else {
			0
		}
	}
	fn bind(&self, bus_dev: &mut dyn device_manager::BusDevice) -> device_manager::DriverBindResult
	{
		let irq = bus_dev.get_irq(0);
		let base = bus_dev.bind_io(0);

		Ok( crate::BusDev::new_boxed(irq, base)? )
	}
}

