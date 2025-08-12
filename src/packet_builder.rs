/// UDP 패킷 생성기
/// 표준화된 내부 데이터 구조체를 UDP 바이너리 패킷으로 직렬화

use crate::data_parser::{ParsedData, StandardizedTrade, StandardizedOrderBookUpdate, StandardizedTradeBatch};
use crate::protocol::{PacketHeader, OrderBookItem, TradeTickItem, MESSAGE_TYPE_ORDER_BOOK, MESSAGE_TYPE_TRADE_TICK};
use crate::events::SystemEvent;
use crate::errors::{CryptoFeederError, Result};

use log::{debug, warn};
use crossbeam_queue::ArrayQueue;
use std::collections::HashMap;
use std::sync::{atomic::{AtomicU64, Ordering}};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct PacketBuilder {
    sequence_counter: AtomicU64,
    payload_pool: PayloadPool,
}

pub struct UdpPacket {
    pub data: Vec<u8>,
    pub size: usize,
}

impl PacketBuilder {
    pub fn new() -> Self {
        Self {
            sequence_counter: AtomicU64::new(1),
            payload_pool: PayloadPool::with_capacity(1024, 1400),
        }
    }

    /// 파싱된 데이터로부터 UDP 패킷들 생성
    pub fn build_packets(&self, parsed_data: ParsedData) -> Result<Vec<UdpPacket>> {
        match parsed_data {
            ParsedData::Trade(trade) => self.build_trade_packets(trade),
            ParsedData::TradeBatch(batch) => self.build_trade_batch_packets(batch),
            ParsedData::OrderBook(order_book) => self.build_order_book_packets(order_book),
        }
    }

    /// 시스템 이벤트 패킷 생성 (교차 시장 식별을 위해 exchange_display를 그대로 사용)
    pub fn build_event_packet_with_exchange(&self, event: SystemEvent, exchange_display: &str) -> Result<UdpPacket> {
        let mut header = PacketHeader::new();
        let current_timestamp = self.get_current_timestamp_nanos();
        
        header.protocol_version = 1;
        header.sequence_number = self.sequence_counter.fetch_add(1, Ordering::SeqCst);
        header.exchange_timestamp = current_timestamp;
        header.local_timestamp = current_timestamp;
        header.message_type = event.get_message_type();
        header.set_flags_and_count(true, 1); // 이벤트는 항상 단일 아이템
        header.set_symbol(&event.get_symbol());
        header.set_exchange(exchange_display);

        let payload = event.get_payload_bytes();
        self.create_packet(header, vec![payload])
    }

    /// (하위호환) 기존 API. 내부 매핑 기반 문자열을 사용함
    pub fn build_event_packet(&self, event: SystemEvent) -> Result<UdpPacket> {
        self.build_event_packet_with_exchange(event.clone(), &event.get_exchange())
    }

    /// 체결 데이터 패킷 생성 (단일 trade용 - 호환성 유지)
    fn build_trade_packets(&self, trade: StandardizedTrade) -> Result<Vec<UdpPacket>> {
        // 단일 trade를 벡터로 래핑하여 배치 처리 함수 사용
        self.build_trade_packets_batch(vec![trade])
    }

    /// 여러 체결 데이터를 타임스탬프별로 그룹핑하여 패킷 생성 (최적화된 버전)
    pub fn build_trade_packets_batch(&self, trades: Vec<StandardizedTrade>) -> Result<Vec<UdpPacket>> {
        if trades.is_empty() {
            return Ok(vec![]);
        }

        // 타임스탬프별로 trade들을 그룹핑
        let mut trade_groups: HashMap<u64, Vec<StandardizedTrade>> = HashMap::new();
        
        for trade in trades {
            trade_groups.entry(trade.timestamp).or_insert_with(Vec::new).push(trade);
        }

        let mut all_packets = Vec::new();

        // 각 타임스탬프 그룹별로 패킷 생성
        for (timestamp, mut trades_in_group) in trade_groups {
            if trades_in_group.is_empty() {
                continue;
            }

            // 체결틱 정렬: ask(is_buyer_taker=true) 오름차순, bid 내림차순
            // 같은 가격일 때는 메시지 순서 보장을 위해 stable sort 사용
            trades_in_group.sort_by(|a, b| {
                match (a.is_buyer_taker, b.is_buyer_taker) {
                    (true, true) => a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal), // ask 오름차순
                    (false, false) => b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal), // bid 내림차순
                    (true, false) => std::cmp::Ordering::Less, // ask가 bid보다 먼저
                    (false, true) => std::cmp::Ordering::Greater, // bid가 ask보다 뒤에
                }
            });

            // 심볼과 거래소가 다른 trade들은 별도 처리
            let mut symbol_groups: HashMap<(String, String), Vec<&StandardizedTrade>> = HashMap::new();
            
            for trade in &trades_in_group {
                let key = (trade.symbol.clone(), trade.exchange.clone());
                symbol_groups.entry(key).or_insert_with(Vec::new).push(trade);
            }

            // 각 심볼-거래소 조합별로 패킷 생성
            for ((symbol, exchange), symbol_trades) in symbol_groups {
                let mut trade_items = Vec::new();
                
                for trade in &symbol_trades {
                    let trade_item = TradeTickItem::new(trade.price, trade.quantity, trade.is_buyer_taker);
                    trade_items.push(trade_item.to_bytes());
                }

                // 패킷 크기 제한을 고려하여 청크로 분할 (최대 50개 trade/패킷)
                let chunks: Vec<_> = trade_items.chunks(50).collect();
                let total_chunks = chunks.len();

                for (chunk_index, chunk) in chunks.into_iter().enumerate() {
                    let mut header = PacketHeader::new();
                    self.setup_header(&mut header, &symbol, &exchange, MESSAGE_TYPE_TRADE_TICK, timestamp);
                    
                    let is_last = chunk_index == total_chunks - 1;
                    header.set_flags_and_count(is_last, chunk.len() as u8);

                    let packet = self.create_packet(header, chunk.to_vec())?;
                    all_packets.push(packet);
                }

                debug!("📦 체결 패킷 생성: {} {} - {} 패킷, {} 체결 (타임스탬프: {})", 
                       exchange, symbol, total_chunks, symbol_trades.len(), timestamp);
            }
        }

        Ok(all_packets)
    }

    /// WS 메시지 단위 배치된 Trade 묶음을 패킷으로 생성
    fn build_trade_batch_packets(&self, batch: StandardizedTradeBatch) -> Result<Vec<UdpPacket>> {
        let mut trades = batch.trades;
        if trades.is_empty() { return Ok(vec![]); }
        // 정렬 규칙 적용
        trades.sort_by(|a, b| {
            match (a.is_buyer_taker, b.is_buyer_taker) {
                (true, true) => a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal),
                (false, false) => b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal),
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
            }
        });

        // 80개씩 청크 분할하여 바로 평탄화 직렬화
        let chunks: Vec<_> = trades.chunks(80).collect();
        let total_chunks = chunks.len();

        let mut packets = Vec::new();
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let mut header = PacketHeader::new();
            // 배치의 공통 메타데이터 사용
            self.setup_header(&mut header, &batch.symbol, &batch.exchange, MESSAGE_TYPE_TRADE_TICK, batch.exchange_timestamp);
            let is_last = chunk_index == total_chunks - 1;
            header.set_flags_and_count(is_last, chunk.len() as u8);
            // 평탄화된 아이템 버퍼 조립 (풀 이용)
            let needed = chunk.len() * std::mem::size_of::<TradeTickItem>();
            let mut buf = self.payload_pool.acquire_buffer(needed);
            for t in chunk {
                let item = TradeTickItem::new(t.price, t.quantity, t.is_buyer_taker);
                item.append_to_vec(&mut buf);
            }
            let packet = self.create_packet_from_flat(header, buf)?;
            packets.push(packet);
        }
        Ok(packets)
    }

    // 마이크로 배칭 경로는 명세 변경에 따라 제거되었습니다.

    /// 오더북 데이터 패킷 생성
    fn build_order_book_packets(&self, order_book: StandardizedOrderBookUpdate) -> Result<Vec<UdpPacket>> {
        let mut packets = Vec::new();
        let mut all_items: Vec<OrderBookItem> = Vec::new();

        // Bids 정렬 (가격 내림차순) 후 OrderBookItem으로 변환
        let mut bids = order_book.bids;
        bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
        
        for bid in bids {
            if bid.quantity > 0.0 { // 수량이 0인 항목은 제외
                all_items.push(OrderBookItem::new(bid.price, bid.quantity, false)); // false = bid
            }
        }

        // Asks 정렬 (가격 오름차순) 후 OrderBookItem으로 변환
        let mut asks = order_book.asks;
        asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));
        
        for ask in asks {
            if ask.quantity > 0.0 { // 수량이 0인 항목은 제외
                all_items.push(OrderBookItem::new(ask.price, ask.quantity, true)); // true = ask
            }
        }

        if all_items.is_empty() {
            warn!("⚠️ 오더북 업데이트에 유효한 아이템이 없음: {} {}", 
                  order_book.exchange, order_book.symbol);
            return Ok(vec![]);
        }

        // 80개씩 청크로 나누어 패킷 생성 (MTU 안전 범위)
        let chunks: Vec<_> = all_items.chunks(80).collect();
        let total_chunks = chunks.len();

        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let mut header = PacketHeader::new();
            self.setup_header(&mut header, &order_book.symbol, &order_book.exchange, 
                            MESSAGE_TYPE_ORDER_BOOK, order_book.timestamp);
            
            let is_last = chunk_index == total_chunks - 1;
            header.set_flags_and_count(is_last, chunk.len() as u8);

            // 평탄화된 아이템 버퍼 조립 (풀 이용)
            let needed = chunk.len() * std::mem::size_of::<OrderBookItem>();
            let mut buf = self.payload_pool.acquire_buffer(needed);
            for ob in chunk {
                ob.append_to_vec(&mut buf);
            }
            let packet = self.create_packet_from_flat(header, buf)?;
            packets.push(packet);
        }

        debug!("📦 오더북 패킷 생성: {} {} - {} 패킷, {} 아이템", 
               order_book.exchange, order_book.symbol, packets.len(), all_items.len());

        Ok(packets)
    }

    /// 패킷 헤더 기본 설정
    fn setup_header(&self, header: &mut PacketHeader, symbol: &str, exchange: &str, 
                   message_type: u8, exchange_timestamp: u64) {
        header.protocol_version = 1;
        header.sequence_number = self.sequence_counter.fetch_add(1, Ordering::SeqCst);
        header.exchange_timestamp = exchange_timestamp;
        header.local_timestamp = self.get_current_timestamp_nanos();
        header.message_type = message_type;
        header.set_symbol(symbol);
        header.set_exchange(exchange);
    }

    /// 헤더와 아이템들로 최종 패킷 생성
    fn create_packet(&self, header: PacketHeader, item_bytes: Vec<Vec<u8>>) -> Result<UdpPacket> {
        let header_bytes = header.to_bytes();
        let items_total: usize = item_bytes.iter().map(|b| b.len()).sum();
        let mut packet_data = Vec::with_capacity(header_bytes.len() + items_total);
        packet_data.extend_from_slice(&header_bytes);
        for item in item_bytes {
            packet_data.extend_from_slice(&item);
        }

        // MTU 안전 범위 검증
        if packet_data.len() > 1472 {
            return Err(CryptoFeederError::SerializationError(
                format!("패킷 크기가 MTU를 초과함: {} bytes", packet_data.len())
            ));
        }

        Ok(UdpPacket {
            size: packet_data.len(),
            data: packet_data,
        })
    }

    /// 평탄화된 아이템 바이트로 최종 패킷 생성 (추가 할당 최소화)
    fn create_packet_from_flat(&self, header: PacketHeader, items_flat: Vec<u8>) -> Result<UdpPacket> {
        let header_bytes = header.to_bytes();
        let mut packet_data = Vec::with_capacity(header_bytes.len() + items_flat.len());
        packet_data.extend_from_slice(&header_bytes);
        packet_data.extend_from_slice(&items_flat);

        // MTU 안전 범위 검증
        if packet_data.len() > 1472 {
            return Err(CryptoFeederError::SerializationError(
                format!("패킷 크기가 MTU를 초과함: {} bytes", packet_data.len())
            ));
        }

        Ok(UdpPacket { size: packet_data.len(), data: packet_data })
    }

    /// 현재 시간을 나노초 단위로 반환
    fn get_current_timestamp_nanos(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }

    /// 시퀀스 번호 조회 (메트릭스/테스트용)
    pub fn get_sequence_number(&self) -> u64 {
        self.sequence_counter.load(Ordering::SeqCst)
    }
}

/// 고정 크기 Vec<u8> 슬랩 풀 (모듈 스코프)
pub struct PayloadPool {
    queue: ArrayQueue<Vec<u8>>,
    buf_capacity: usize,
}

impl PayloadPool {
    fn with_capacity(pool_size: usize, buf_capacity: usize) -> Self {
        let queue = ArrayQueue::new(pool_size);
        // 초기 버퍼 준비(최대 64개 선할당)
        for _ in 0..pool_size.min(64) {
            let _ = queue.push(Vec::with_capacity(buf_capacity));
        }
        Self { queue, buf_capacity }
    }

    fn acquire_buffer(&self, min_len: usize) -> Vec<u8> {
        if let Some(mut buf) = self.queue.pop() {
            if buf.capacity() < min_len {
                buf = Vec::with_capacity(min_len.max(self.buf_capacity));
            }
            buf.clear();
            buf
        } else {
            Vec::with_capacity(min_len.max(self.buf_capacity))
        }
    }

    #[allow(dead_code)]
    fn release_buffer(&self, mut buf: Vec<u8>) {
        buf.clear();
        let _ = self.queue.push(buf);
    }
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_parser::StandardizedTrade;

    #[test]
    fn test_packet_builder_creation() {
        let builder = PacketBuilder::new();
        assert_eq!(builder.get_sequence_number(), 1);
    }

    #[test]
    fn test_trade_packet_building() {
        let builder = PacketBuilder::new();
        let trade = StandardizedTrade {
            symbol: "BTC^USDT".to_string(),
            exchange: "BinanceSpot".to_string(),
            price: 50000.0,
            quantity: 1.5,
            is_buyer_taker: true,
            timestamp: 1640995200000000000, // 2022-01-01 00:00:00 UTC in nanoseconds
        };

        let packets = builder.build_trade_packets(trade).unwrap();
        assert_eq!(packets.len(), 1);
        
        let packet = &packets[0];
        assert!(packet.size > 67); // 헤더 + 최소 1개 TradeTickItem
        assert!(packet.size <= 1472); // MTU 안전 범위
    }

    #[test]
    fn test_sequence_number_increment() {
        let builder = PacketBuilder::new();
        let initial_seq = builder.get_sequence_number();
        
        // 첫 번째 패킷 생성
        let trade1 = StandardizedTrade {
            symbol: "BTC/USDT".to_string(),
            exchange: "binance".to_string(),
            price: 50000.0,
            quantity: 1.0,
            is_buyer_taker: false,
            timestamp: 1640995200000000000,
        };
        
        let _packets1 = builder.build_trade_packets(trade1).unwrap();
        assert_eq!(builder.get_sequence_number(), initial_seq + 1);
        
        // 두 번째 패킷 생성
        let trade2 = StandardizedTrade {
            symbol: "ETH/USDT".to_string(),
            exchange: "binance".to_string(),
            price: 3000.0,
            quantity: 2.0,
            is_buyer_taker: true,
            timestamp: 1640995260000000000,
        };
        
        let _packets2 = builder.build_trade_packets(trade2).unwrap();
        assert_eq!(builder.get_sequence_number(), initial_seq + 2);
    }

    #[test]
    fn test_trade_batch_building_ws_message_unit() {
        let builder = PacketBuilder::new();
        // 하나의 WS 메시지에 3건 체결이 들어온 상황을 가정
        let t1 = StandardizedTrade { symbol: "XRP^USDT".into(), exchange: "BinanceSpot".into(), price: 0.5, quantity: 1000.0, is_buyer_taker: true, timestamp: 1 };
        let t2 = StandardizedTrade { symbol: "XRP^USDT".into(), exchange: "BinanceSpot".into(), price: 0.49, quantity: 2000.0, is_buyer_taker: false, timestamp: 1 };
        let t3 = StandardizedTrade { symbol: "XRP^USDT".into(), exchange: "BinanceSpot".into(), price: 0.51, quantity: 3000.0, is_buyer_taker: true, timestamp: 1 };

        let batch = StandardizedTradeBatch { symbol: "XRP^USDT".into(), exchange: "BinanceSpot".into(), exchange_timestamp: 1, trades: vec![t1, t2, t3] };
        let packets = builder.build_trade_batch_packets(batch).unwrap();
        assert_eq!(packets.len(), 1); // 3건이므로 1패킷
        // 헤더 검사
        let header = PacketHeader::from_bytes(&packets[0].data[..std::mem::size_of::<PacketHeader>()]);
        assert_eq!(header.item_count(), 3);
        assert!(header.is_last());
        assert_eq!(header.message_type, MESSAGE_TYPE_TRADE_TICK);
    }
}