/*!
 * Network stack wrapper
 */
#[macro_use]
extern crate kernel;
use ::kernel_test_network::HexDump;
use std::sync::Arc;

struct Args
{
	master_addr: std::net::SocketAddr,

	sim_ip: network::ipv4::Address,
}

fn main()
{
	let args = {
        let mut it = std::env::args();
        it.next().unwrap();
		Args {
            master_addr: {
                let a = it.next().unwrap();
                match std::net::ToSocketAddrs::to_socket_addrs(&a)
                {
                Err(e) => panic!("Unable to parse '{}' as a socket addr: {}", a, e),
                Ok(mut v) => v.next().unwrap(),
                }
                },
			sim_ip: {
				let std_ip: std::net::Ipv4Addr = it.next().unwrap().parse().unwrap();
				let o = std_ip.octets();
				network::ipv4::Address::new(o[0], o[1], o[2], o[3])
				},
			}
        };
    
    kernel::threads::init();
    (network::S_MODULE.init)();
        
    let stream = match std::net::UdpSocket::bind("0.0.0.0:0")
        {
        Ok(v) => v,
        Err(e) => {
            println!("Cannot connect to server: {}", e);
            return
            },
        };
	stream.connect( args.master_addr ).expect("Unable to connect");
	// - Set a timeout, in case the parent fails
	stream.set_read_timeout(Some(::std::time::Duration::from_secs(1))).expect("Unable to set read timeout");
	let stream = Arc::new(stream);
    stream.send(&[0]).expect("Unable to send marker to server");
    
    let mac = *b"RSK\x12\x34\x56";
    let nic_handle = network::nic::register(mac, TestNic::new(1, stream.clone()));
	// TODO: Make this a command instead
    network::ipv4::add_interface(mac, args.sim_ip, 24);

    let (tx,rx) = ::std::sync::mpsc::channel();
    ::std::thread::spawn(move || {
        loop
        {
            const MTU: usize = 1560;
            let mut buf = [0; 4 + MTU];
            let len = match stream.recv(&mut buf)
                {
                Ok(len) => len,
                Err(e) => {
                    println!("Error receiving packet: {:?}", e);
                    break;
                    },
                };
            if len == 0 {
                println!("ERROR: Zero-sized packet?");
                break;
            }
            if len < 4 {
                println!("ERROR: Runt packet {:?}", HexDump(&buf[..len]));
                break;
            }
            let id = u32::from_le_bytes(std::convert::TryInto::try_into(&buf[..4]).unwrap());
            let data = &mut buf[4..len];
            if id == 0
            {
                let line = match std::str::from_utf8(data)
                    {
                    Ok(v) => v,
                    Err(e) => panic!("Bad UTF-8 from server: {:?} - {:?}", e, HexDump(&data)),
                    };
                println!("COMMAND {:?}", line);

                tx.send(line.to_owned()).expect("Failed to send command to main thread");
            }
            else
            {
                let buf = data.to_owned();
                log_notice!("RX #{} {:?}", id, HexDump(data));
                let nic = match id
                    {
                    0 => unreachable!(),
                    1 => &nic_handle,
                    _ => panic!("Unknown NIC ID {}", id),
                    };
                nic.packet_received(buf);
            }
        }
        });


	// Monitor stdin for commands
	let mut tcp_conn_handles = ::std::collections::HashMap::new();
	let mut tcp_server_handles = ::std::collections::HashMap::new();
	
    loop
    {
		let mut line = ::kernel::arch::imp::threads::test_pause_thread(|| rx.recv()).unwrap();

		let mut it = ::cmdline_words_parser::parse_posix(&mut line[..]);
		let cmd = match it.next()
			{
			Some(c) => c,
			None => {
				log_notice!("stdin empty");
				break
				},
			};
		match cmd
		{
		"" => {},
		"exit" => {
			log_notice!("exit command");
			break
			},
		"ipv4-add" => {
			},
		// Listen on a port/interface
		"tcp-listen" => {
			let index: usize = it.next().unwrap().parse().unwrap();
			let port : u16   = it.next().unwrap().parse().unwrap();
			log_notice!("tcp-listen {} = *:{}", index, port);
			tcp_server_handles.insert(index, ::network::tcp::ServerHandle::listen(port).unwrap());
			println!("OK");
			},
		"tcp-accept" => {
			let c_index: usize = it.next().unwrap().parse().unwrap();
			let s_index: usize = it.next().unwrap().parse().unwrap();
			log_notice!("tcp-accept {} = [{}]", c_index, s_index);
			let s = tcp_server_handles.get_mut(&s_index).expect("BUG: Bad server index");
			tcp_conn_handles.insert(c_index, s.accept().expect("No waiting connection"));
			println!("OK");
			},
		// Make a connection
		"tcp-connect" => {
			// Get dest ip & dest port
			let index: usize = it.next().unwrap().parse().unwrap();
			let ip: ::network::Address = parse_addr(it.next().expect("Missing IP")).unwrap();
			let port: u16 = it.next().unwrap().parse().unwrap();
			log_notice!("tcp-connect {} = {:?}:{}", index, ip, port);
			tcp_conn_handles.insert(index, ::network::tcp::ConnectionHandle::connect(ip, port).unwrap());
			println!("OK");
			},
		// Close a TCP connection
		"tcp-close" => {
			let index: usize = it.next().unwrap().parse().unwrap();
			todo!("tcp-close {}", index);
			},
		"tcp-send" => {
			let index: usize = it.next().unwrap().parse().unwrap();
			let bytes = parse_hex_bytes(it.next().unwrap()).unwrap();
			let h = &tcp_conn_handles[&index];
			log_notice!("tcp-send {} {:?}", index, bytes);
			h.send_data(&bytes).unwrap();
			println!("OK");
			},
		"tcp-recv-assert" => {
			let index: usize = it.next().unwrap().parse().unwrap();
			let read_size: usize = it.next().unwrap().parse().unwrap();
			let exp_bytes = parse_hex_bytes(it.next().unwrap()).unwrap();
			// - Receive bytes, check that they equal an expected value
			// NOTE: No wait
			log_notice!("tcp-recv-assert {} {} == {:?}", index, read_size, exp_bytes);
			let h = &tcp_conn_handles[&index];

			let mut buf = vec![0; read_size];
			let len = h.recv_data(&mut buf).unwrap();
			assert_eq!(&buf[..len], &exp_bytes[..]);
			println!("OK");
			},
		_ => panic!("ERROR: Unknown command '{}'", cmd),
		}
    }
}

fn parse_hex_bytes(s: &str) -> Option<Vec<u8>>
{
	let mut nibble = 0;
	let mut cur_byte = 0;
	let mut rv = Vec::new();
	for c in s.chars()
	{
		if c.is_whitespace() {
			continue ;
		}
		let d = c.to_digit(16)?;

		cur_byte |= d << (4 * (1 - nibble));
		nibble += 1;

		if nibble == 2 {
			rv.push(cur_byte as u8);
			cur_byte = 0;
			nibble = 0;
		}
	}

	if nibble != 0 {
		None
	}
	else {
		Some(rv)
	}
}

fn parse_addr(s: &str) -> Option<::network::Address>
{
	if s.contains(".") {
		let mut it = s.split('.');
		let b1: u8 = it.next()?.parse().ok()?;
		let b2: u8 = it.next()?.parse().ok()?;
		let b3: u8 = it.next()?.parse().ok()?;
		let b4: u8 = it.next()?.parse().ok()?;
		if it.next().is_some() {
			return None;
		}
		Some( ::network::Address::Ipv4(::network::ipv4::Address::new(b1, b2, b3, b4)) )
	}
	else {
		None
	}
}

struct TestNic
{
	number: u32,
    stream: Arc<std::net::UdpSocket>,
    waiter: std::sync::Mutex< Option<kernel::threads::SleepObjectRef> >,
    // NOTE: Kernel sync queue
    packets: std::sync::Mutex< std::collections::VecDeque< Vec<u8> > >,
}

impl TestNic
{
    fn new(number: u32, stream: Arc<std::net::UdpSocket>) -> TestNic
    {
        TestNic {
			number,
            stream,
            waiter: Default::default(),
            packets: Default::default(),
            }
    }

	fn packet_received(&self, buf: Vec<u8>)
	{	
		self.packets.lock().unwrap().push_back( buf );
		match *self.waiter.lock().unwrap()
		{
		Some(ref v) => v.signal(),
		None => println!("No registered waiter yet?"),
		}
	}
}
impl network::nic::Interface for TestNic
{
    fn tx_raw(&self, pkt: network::nic::SparsePacket<'_>) {
		let it = pkt.into_iter().flat_map(|v| v.iter());
		let num_enc = self.number.to_le_bytes();
		let it = Iterator::chain( num_enc.iter(), it );
        let buf: Vec<u8> = it.copied().collect();
		log_notice!("TX {:?}", HexDump(&buf));
        self.stream.send(&buf).unwrap();
    }
    //fn tx_async<'a,'s>(&'s self, _: kernel::_async3::ObjectHandle, _: kernel::_async3::StackPush<'a, 's>, _: network::nic::SparsePacket<'_>) -> Result<(), network::nic::Error> {
    //    todo!("TestNic::tx_async")
    //}
    fn rx_wait_register(&self, channel: &kernel::threads::SleepObject<'_>) {
        *self.waiter.lock().unwrap() = Some(channel.get_ref());
    }
	fn rx_wait_unregister(&self, _channel: &kernel::threads::SleepObject) {
        *self.waiter.lock().unwrap() = None;
    }

    fn rx_packet(&self) -> Result<network::nic::PacketHandle, network::nic::Error> {
        let mut lh = self.packets.lock().unwrap();
        if let Some(v) = lh.pop_front()
        {
            struct RxPacketHandle(Vec<u8>);
            impl<'a> network::nic::RxPacket for RxPacketHandle {
                fn len(&self) -> usize {
                    self.0 .len()
                }
                fn num_regions(&self) -> usize {
                    1
                }
                fn get_region(&self, idx: usize) -> &[u8] {
                    assert!(idx == 0);
                    &self.0
                }
                fn get_slice(&self, range: ::core::ops::Range<usize>) -> Option<&[u8]> {
                    let b = self.get_region(0);
                    b.get(range)
                }
            }

            Ok(network::nic::PacketHandle::new(RxPacketHandle(v)).ok().unwrap())
        }
        else
        {
            Err(network::nic::Error::NoPacket)
        }
    }
}
