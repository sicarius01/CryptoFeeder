/// 패킷 디코더 유틸리티
/// UDP 멀티캐스트로 전송된 패킷을 수신하고 사람이 읽을 수 있는 형태로 출력

use std::net::UdpSocket;
use std::mem;
use std::fs;
use std::collections::HashMap;
use socket2::{Domain, Type, Protocol, Socket};
use std::time::{Instant, Duration};

// 프로토콜 구조체 재정의 (크로스 바이너리 호환성을 위해)
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
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

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct OrderBookItem {
    price: i64,
    quantity_with_flags: i64,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct TradeTickItem {
    price: i64,
    quantity_with_flags: i64,
}

impl PacketHeader {
    fn is_last(&self) -> bool {
        (self.flags_and_count & 0b1000_0000) != 0
    }

    fn item_count(&self) -> u8 {
        self.flags_and_count & 0b0111_1111
    }

    fn symbol_as_string(&self) -> String {
        let end = self.symbol.iter().position(|&b| b == 0).unwrap_or(self.symbol.len());
        String::from_utf8_lossy(&self.symbol[..end]).to_string()
    }

    fn exchange_as_string(&self) -> String {
        let end = self.exchange.iter().position(|&b| b == 0).unwrap_or(self.exchange.len());
        String::from_utf8_lossy(&self.exchange[..end]).to_string()
    }
}

impl TradeTickItem {
    fn is_buyer_taker(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }

    fn quantity(&self) -> i64 {
        self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF
    }

    fn get_real_price(&self) -> f64 {
        self.price as f64 / 100_000_000.0
    }

    fn get_real_quantity(&self) -> f64 {
        self.quantity() as f64 / 100_000_000.0
    }
}

impl OrderBookItem {
    fn get_real_price(&self) -> f64 {
        self.price as f64 / 100_000_000.0
    }

    fn get_real_quantity(&self) -> f64 {
        (self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF) as f64 / 100_000_000.0
    }

    fn is_ask(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }
}

struct Stats {
    total_packets: u64,
    total_bytes: u64,
    orderbook_packets: u64,
    tradetick_packets: u64,
    orderbook_items: u64,
    tradetick_items: u64,
}

impl Stats {
    fn new() -> Self { Self { total_packets:0, total_bytes:0, orderbook_packets:0, tradetick_packets:0, orderbook_items:0, tradetick_items:0 } }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 CryptoFeeder 패킷 디코더 시작");

    let args: Vec<String> = std::env::args().collect();
    // 우선순위: CLI 포트 > config.ini 기본 포트 > 55555
    let mut udp_cfg = read_udp_config_from_ini().unwrap_or(UdpCfg {
        multicast_addr: "239.255.1.1".to_string(),
        port: 55555,
        interface_addr: "0.0.0.0".to_string(),
    });
    if args.len() >= 2 {
        if let Ok(p) = args[1].parse::<u16>() { udp_cfg.port = p; }
    }

    println!(
        "📡 멀티캐스트 그룹 {}:{} 수신 대기 중...",
        udp_cfg.multicast_addr, udp_cfg.port
    );

    // UDP 소켓 생성 및 멀티캐스트 그룹 가입
    let bind_addr = format!("0.0.0.0:{}", udp_cfg.port);
    // reuseaddr를 설정하여 멀티캐스트 포트 공유 가능하도록 함 (Windows 호환)
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.bind(&bind_addr.parse::<std::net::SocketAddr>()?.into())?;
    let socket: UdpSocket = sock.into();
    socket.join_multicast_v4(&udp_cfg.multicast_addr.parse()?, &udp_cfg.interface_addr.parse()?)?;

    let mut buffer = [0u8; 1500]; // MTU 크기 버퍼
    let mut stats = Stats::new();
    let mut interval_map: HashMap<String, IntervalStat> = HashMap::new();
    let start = Instant::now();

    println!("✅ 수신 준비 완료\n");

    loop {
        match socket.recv_from(&mut buffer) {
            Ok((size, addr)) => {
                println!("📦 패킷 수신: {} bytes from {}", size, addr);
                stats.total_packets += 1;
                stats.total_bytes += size as u64;

                if let Err(e) = decode_packet(&buffer[..size], &mut stats, &mut interval_map) {
                    eprintln!("❌ 디코딩 오류: {}", e);
                }
                if start.elapsed() >= Duration::from_secs(5) && stats.total_packets % 10 == 0 {
                    println!(
                        "📊 요약: pkts={} bytes={} avg={:.1}B ob_pkts={} ob_items={} tr_pkts={} tr_items={}",
                        stats.total_packets, stats.total_bytes,
                        (stats.total_bytes as f64 / stats.total_packets.max(1) as f64),
                        stats.orderbook_packets, stats.orderbook_items,
                        stats.tradetick_packets, stats.tradetick_items
                    );
                }
                println!("{}", "─".repeat(80));
            },
            Err(e) => {
                eprintln!("❌ 수신 오류: {}", e);
                break;
            }
        }
    }

    Ok(())
}

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
    // 포트는 symbol_config에서 세션별로 관리되므로 여기서는 기본값만 사용
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
        if (line.starts_with('[') && line.ends_with(']')) { continue; }
        if let Some((k, v)) = line.split_once('=') {
            cfg.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    cfg
}

#[derive(Debug, Clone)]
struct IntervalStat {
    last: Option<Instant>,
    count: u64,
    sum_ms: f64,
}

impl IntervalStat {
    fn update(&mut self, now: Instant) -> Option<f64> {
        let dt_ms = self.last.map(|prev| (now - prev).as_secs_f64() * 1000.0);
        self.last = Some(now);
        if let Some(dt) = dt_ms {
            self.count += 1;
            self.sum_ms += dt;
            Some(dt)
        } else {
            None
        }
    }
    fn avg(&self) -> f64 { if self.count == 0 { 0.0 } else { self.sum_ms / self.count as f64 } }
}

fn decode_packet(data: &[u8], stats: &mut Stats, intervals: &mut HashMap<String, IntervalStat>) -> Result<(), Box<dyn std::error::Error>> {
    if data.len() < mem::size_of::<PacketHeader>() {
        return Err("패킷이 너무 작음".into());
    }

    // 헤더 디코딩
    let header = unsafe {
        std::ptr::read_unaligned(data.as_ptr() as *const PacketHeader)
    };

    // packed struct 필드 직접 접근 방지를 위해 로컬 변수에 복사
    let protocol_version = header.protocol_version;
    let sequence_number = header.sequence_number;
    let message_type = header.message_type;
    let exchange_timestamp = header.exchange_timestamp;
    let local_timestamp = header.local_timestamp;
    
    println!("📋 헤더 정보:");
    println!("  - 프로토콜 버전: {}", protocol_version);
    println!("  - 시퀀스 번호: {}", sequence_number);
    let ex = header.exchange_as_string();
    let sym = header.symbol_as_string();
    println!("  - 거래소: {}", ex);
    println!("  - 심볼: {}", sym);
    println!("  - 메시지 타입: {} ({})", message_type, 
             match message_type {
                 0 => "OrderBook",
                 1 => "TradeTick",
                 _ => "Unknown"
             });
    println!("  - 아이템 수: {}", header.item_count());
    println!("  - 마지막 패킷: {}", header.is_last());
    println!("  - 거래소 타임스탬프: {} ns", exchange_timestamp);
    println!("  - 로컬 타임스탬프: {} ns", local_timestamp);

    // 페이로드 디코딩
    let payload_start = mem::size_of::<PacketHeader>();
    let payload = &data[payload_start..];
    
    match header.message_type {
        0 => {
            stats.orderbook_packets += 1;
            stats.orderbook_items += header.item_count() as u64;
            decode_order_book_items(payload, header.item_count())?
        },
        1 => {
            stats.tradetick_packets += 1;
            stats.tradetick_items += header.item_count() as u64;
            decode_trade_tick_items(payload, header.item_count())?
        },
        _ => println!("⚠️ 알 수 없는 메시지 타입"),
    }

    // 오더북 패킷 간격 측정 (BinanceFutures의 depth0ms 확인용 휴리스틱)
    if message_type == 0 {
        let key = format!("{}|{}", ex, sym);
        let entry = intervals.entry(key.clone()).or_insert(IntervalStat { last: None, count: 0, sum_ms: 0.0 });
        if let Some(dt) = entry.update(Instant::now()) {
            let avg = entry.avg();
            if ex == "BinanceFutures" {
                let classification = if avg < 50.0 { "depth0ms 추정" } else { "(>=50ms)" };
                println!("⏱️ 간격 측정 [{}]: 최신 {:.1} ms, 평균 {:.1} ms → {}", key, dt, avg, classification);
            } else {
                println!("⏱️ 간격 측정 [{}]: 최신 {:.1} ms, 평균 {:.1} ms", key, dt, avg);
            }
        }
    }

    Ok(())
}

fn decode_order_book_items(payload: &[u8], count: u8) -> Result<(), Box<dyn std::error::Error>> {
    println!("📈 오더북 데이터:");
    
    let item_size = mem::size_of::<OrderBookItem>();
    
    for i in 0..count {
        let start = (i as usize) * item_size;
        let end = start + item_size;
        
        if end > payload.len() {
            return Err("페이로드가 너무 작음".into());
        }
        
        let item = unsafe {
            std::ptr::read_unaligned(payload[start..].as_ptr() as *const OrderBookItem)
        };
        
        println!("  #{}: ${:.8} x {:.8}", 
                 i + 1, item.get_real_price(), item.get_real_quantity());
    }
    
    Ok(())
}

fn decode_trade_tick_items(payload: &[u8], count: u8) -> Result<(), Box<dyn std::error::Error>> {
    println!("💹 체결 데이터:");
    
    let item_size = mem::size_of::<TradeTickItem>();
    
    for i in 0..count {
        let start = (i as usize) * item_size;
        let end = start + item_size;
        
        if end > payload.len() {
            return Err("페이로드가 너무 작음".into());
        }
        
        let item = unsafe {
            std::ptr::read_unaligned(payload[start..].as_ptr() as *const TradeTickItem)
        };
        
        let side = if item.is_buyer_taker() { "BUY" } else { "SELL" };
        
        println!("  #{}: ${:.8} x {:.8} [{}]", 
                 i + 1, item.get_real_price(), item.get_real_quantity(), side);
    }
    
    Ok(())
}