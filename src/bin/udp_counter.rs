use socket2::{Domain, Type, Protocol, Socket};
use std::collections::HashMap;
use std::fs;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct UdpCfg {
	multicast_addr: String,
	port: u16,
	interface_addr: String,
}

fn read_udp_config_from_ini() -> Result<UdpCfg, Box<dyn std::error::Error>> {
	let content = fs::read_to_string("config/config.ini")?;
	let map = parse_simple_ini(&content);
	let multicast_addr = map
		.get("multicast_addr").cloned().unwrap_or_else(|| "239.255.1.1".to_string());
	// 기본 포트는 55555 (실제 세션 포트는 symbol_config.ini에서 지정)
	let port = 55555u16;
	let interface_addr = map
		.get("interface_addr").cloned().unwrap_or_else(|| "0.0.0.0".to_string());
	Ok(UdpCfg { multicast_addr, port, interface_addr })
}

fn parse_simple_ini(content: &str) -> HashMap<String, String> {
	let mut cfg = HashMap::new();
	for line in content.lines() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
		if line.starts_with('[') && line.ends_with(']') { continue; }
		if let Some((k, v)) = line.split_once('=') {
			cfg.insert(k.trim().to_string(), v.trim().to_string());
		}
	}
	cfg
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct PacketHeader {
	protocol_version: u8,
	sequence_number: u64,
	exchange_timestamp: u64,
	local_timestamp: u64,
	message_type: u8,
	flags_and_count: u8,
	symbol: [u8; 20],
	exchange: [u8; 20],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	println!("🔎 UDP 패킷 카운터 시작 (30초)");
	let mut cfg = read_udp_config_from_ini()?;
	let args: Vec<String> = std::env::args().collect();
	if args.len() >= 2 { if let Ok(p) = args[1].parse::<u16>() { cfg.port = p; } }
	println!("📡 수신: {}:{} (iface {})", cfg.multicast_addr, cfg.port, cfg.interface_addr);

	let bind_addr = format!("0.0.0.0:{}", cfg.port);
	let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
	sock.set_reuse_address(true)?;
	sock.bind(&bind_addr.parse::<std::net::SocketAddr>()?.into())?;
	let socket: UdpSocket = sock.into();
	socket.join_multicast_v4(&cfg.multicast_addr.parse()?, &cfg.interface_addr.parse()?)?;
	socket.set_read_timeout(Some(Duration::from_millis(250)))?;

	let mut counts: HashMap<u8, u64> = HashMap::new();
	let mut total: u64 = 0;
	let deadline = Instant::now() + Duration::from_secs(30);
	let mut buf = [0u8; 1500];

	while Instant::now() < deadline {
		match socket.recv(&mut buf) {
			Ok(n) if n >= std::mem::size_of::<PacketHeader>() => {
				let header = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const PacketHeader) };
				let mt = header.message_type;
				*counts.entry(mt).or_insert(0) += 1;
				total += 1;
			}
			Ok(_) => { /* too small, ignore */ }
			Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => { /* spin */ }
			Err(e) => { eprintln!("recv 오류: {}", e); }
		}
	}

	let excluded = [0u8, 1u8, 101u8];
	let mut excluded_total = 0u64;
	for mt in &excluded { excluded_total += counts.get(mt).cloned().unwrap_or(0); }
	let other_total = total.saturating_sub(excluded_total);

	println!("\n⏰ 30초 종료");
	println!("총 패킷: {}", total);
	println!("message_type 별 카운트:");
	let mut keys: Vec<u8> = counts.keys().cloned().collect();
	keys.sort_unstable();
	for k in keys { println!("  - {:3}: {}", k, counts.get(&k).unwrap()); }
	println!("\n제외(0,1,101) 제외 합계: {}", other_total);
	println!("세부(2=index, 3=mark, 4=funding, 5=liquidation): {} / {} / {} / {}",
		counts.get(&2).cloned().unwrap_or(0),
		counts.get(&3).cloned().unwrap_or(0),
		counts.get(&4).cloned().unwrap_or(0),
		counts.get(&5).cloned().unwrap_or(0),
	);

	Ok(())
}


