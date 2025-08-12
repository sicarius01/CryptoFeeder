/// UDP 패킷 프로토콜 구조체 정의
/// udp_packet_detail/udp_packet.md 명세에 따라 구현

use std::mem;

// C와 동일한 메모리 레이아웃을 보장합니다.
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct PacketHeader {
    pub protocol_version: u8,      // 1B
    pub sequence_number: u64,      // 8B
    pub exchange_timestamp: u64,   // 8B
    pub local_timestamp: u64,      // 8B
    pub message_type: u8,          // 1B
    pub flags_and_count: u8,       // 1B
    pub symbol: [u8; 20],          // 20B, null-terminated UTF-8
    pub exchange: [u8; 20],        // 20B, null-terminated UTF-8
} // 총 67 바이트

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct OrderBookItem {
    pub price: i64,                // 8B, Scaled by 10^8
    pub quantity_with_flags: i64,  // 8B, quantity + is_ask flag
} // 총 16 바이트

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct TradeTickItem {
    pub price: i64,                // 8B, Scaled by 10^8
    pub quantity_with_flags: i64,  // 8B, quantity + is_buyer_taker flag
} // 총 16 바이트

// 메시지 타입 상수
pub const MESSAGE_TYPE_ORDER_BOOK: u8 = 0;
pub const MESSAGE_TYPE_TRADE_TICK: u8 = 1;

// 스케일링 상수
pub const PRICE_SCALE: i64 = 100_000_000; // 10^8
pub const QUANTITY_SCALE: i64 = 100_000_000; // 10^8

impl PacketHeader {
    pub fn new() -> Self {
        Self {
            protocol_version: 1,
            sequence_number: 0,
            exchange_timestamp: 0,
            local_timestamp: 0,
            message_type: 0,
            flags_and_count: 0,
            symbol: [0; 20],
            exchange: [0; 20],
        }
    }

    /// 헤더를 바이트 배열에서 역직렬화 (테스트용)
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= mem::size_of::<Self>());
        unsafe { *(bytes.as_ptr() as *const Self) }
    }

    /// is_last 플래그를 추출하는 헬퍼 함수
    pub fn is_last(&self) -> bool {
        (self.flags_and_count & 0b1000_0000) != 0
    }

    /// item_count를 추출하는 헬퍼 함수
    pub fn item_count(&self) -> u8 {
        self.flags_and_count & 0b0111_1111
    }

    /// message_type이 TradeTick인지 확인하는 헬퍼 함수
    pub fn is_trade_tick(&self) -> bool {
        self.message_type == MESSAGE_TYPE_TRADE_TICK
    }

    /// message_type이 OrderBook인지 확인하는 헬퍼 함수
    pub fn is_order_book(&self) -> bool {
        self.message_type == MESSAGE_TYPE_ORDER_BOOK
    }

    /// is_last와 item_count로 flags_and_count 필드를 설정하는 헬퍼 함수
    pub fn set_flags_and_count(&mut self, is_last: bool, count: u8) {
        // count가 최대값(80)을 넘지 않도록 보장 (MTU 안전 범위 유지)
        let item_count = count.min(80) & 0b0111_1111; 
        if is_last {
            self.flags_and_count = item_count | 0b1000_0000;
        } else {
            self.flags_and_count = item_count;
        }
    }

    /// 패킷 정보를 한번에 설정하는 편의 함수
    pub fn set_packet_info(&mut self, message_type: u8, count: u8, is_last: bool) {
        self.message_type = message_type;
        self.set_flags_and_count(is_last, count);
    }

    /// 심볼 설정 (null-terminated UTF-8)
    pub fn set_symbol(&mut self, symbol: &str) {
        self.symbol = [0; 20];
        // 허용 문자만 유지 (레이턴시 보호를 위해 간단한 필터)
        // A-Z, a-z, 0-9, '^', '-', '_'
        let sanitized: String = symbol
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '^' || *c == '-' || *c == '_')
            .collect();
        let bytes = sanitized.as_bytes();
        let len = bytes.len().min(19); // null terminator를 위한 공간 확보
        self.symbol[..len].copy_from_slice(&bytes[..len]);
    }

    /// 거래소 설정 (null-terminated UTF-8)
    pub fn set_exchange(&mut self, exchange: &str) {
        self.exchange = [0; 20];
        // 허용 문자만 유지
        let sanitized: String = exchange
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '^' || *c == '-' || *c == '_')
            .collect();
        let bytes = sanitized.as_bytes();
        let len = bytes.len().min(19); // null terminator를 위한 공간 확보
        self.exchange[..len].copy_from_slice(&bytes[..len]);
    }

    /// 헤더를 바이트 배열로 직렬화
    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl TradeTickItem {
    pub fn new(price: f64, quantity: f64, is_buyer_taker: bool) -> Self {
        let scaled_price = (price * PRICE_SCALE as f64) as i64;
        let scaled_quantity = (quantity * QUANTITY_SCALE as f64) as i64;
        
        let mut item = Self {
            price: scaled_price,
            quantity_with_flags: 0,
        };
        
        item.set_quantity_and_flag(scaled_quantity, is_buyer_taker);
        item
    }

    /// is_buyer_taker 플래그를 추출하는 헬퍼 함수
    pub fn is_buyer_taker(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }

    /// 실제 수량을 추출하는 헬퍼 함수
    pub fn quantity(&self) -> i64 {
        self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF
    }

    /// 수량과 is_buyer_taker 플래그를 설정하는 헬퍼 함수
    pub fn set_quantity_and_flag(&mut self, quantity: i64, is_buyer_taker: bool) {
        let masked_quantity = quantity & 0x7FFF_FFFF_FFFF_FFFF;
        self.quantity_with_flags = if is_buyer_taker {
            masked_quantity | (1i64 << 63)
        } else {
            masked_quantity
        };
    }

    /// TradeTickItem을 바이트 배열로 직렬화
    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }

    /// TradeTickItem을 지정된 버퍼 끝에 추가 (추가 할당 회피)
    pub fn append_to_vec(&self, dst: &mut Vec<u8>) {
        let start_len = dst.len();
        dst.resize(start_len + mem::size_of::<Self>(), 0);
        unsafe {
            let src_ptr = self as *const Self as *const u8;
            let dst_ptr = dst.as_mut_ptr().add(start_len);
            std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, mem::size_of::<Self>());
        }
    }

    /// 실제 가격을 추출 (스케일링 해제)
    pub fn get_real_price(&self) -> f64 {
        self.price as f64 / PRICE_SCALE as f64
    }

    /// 실제 수량을 추출 (스케일링 해제)
    pub fn get_real_quantity(&self) -> f64 {
        self.quantity() as f64 / QUANTITY_SCALE as f64
    }
}

impl OrderBookItem {
    pub fn new(price: f64, quantity: f64, is_ask: bool) -> Self {
        let scaled_price = (price * PRICE_SCALE as f64) as i64;
        let scaled_quantity = (quantity * QUANTITY_SCALE as f64) as i64;
        
        let mut item = Self {
            price: scaled_price,
            quantity_with_flags: 0,
        };
        
        item.set_quantity_and_flag(scaled_quantity, is_ask);
        item
    }

    /// is_ask 플래그를 추출하는 헬퍼 함수
    pub fn is_ask(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }

    /// 실제 수량을 추출하는 헬퍼 함수
    pub fn quantity(&self) -> i64 {
        self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF
    }

    /// 수량과 is_ask 플래그를 설정하는 헬퍼 함수
    pub fn set_quantity_and_flag(&mut self, quantity: i64, is_ask: bool) {
        let masked_quantity = quantity & 0x7FFF_FFFF_FFFF_FFFF;
        self.quantity_with_flags = if is_ask {
            masked_quantity | (1i64 << 63)
        } else {
            masked_quantity
        };
    }

    /// OrderBookItem을 바이트 배열로 직렬화
    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }

    /// OrderBookItem을 지정된 버퍼 끝에 추가 (추가 할당 회피)
    pub fn append_to_vec(&self, dst: &mut Vec<u8>) {
        let start_len = dst.len();
        dst.resize(start_len + mem::size_of::<Self>(), 0);
        unsafe {
            let src_ptr = self as *const Self as *const u8;
            let dst_ptr = dst.as_mut_ptr().add(start_len);
            std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, mem::size_of::<Self>());
        }
    }

    /// 실제 가격을 추출 (스케일링 해제)
    pub fn get_real_price(&self) -> f64 {
        self.price as f64 / PRICE_SCALE as f64
    }

    /// 실제 수량을 추출 (스케일링 해제)
    pub fn get_real_quantity(&self) -> f64 {
        self.quantity() as f64 / QUANTITY_SCALE as f64
    }
}

// 컴파일 타임에 구조체 크기 검증
const _: () = assert!(mem::size_of::<PacketHeader>() == 67);
const _: () = assert!(mem::size_of::<OrderBookItem>() == 16);
const _: () = assert!(mem::size_of::<TradeTickItem>() == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_header_size() {
        assert_eq!(mem::size_of::<PacketHeader>(), 67);
    }

    #[test]
    fn test_order_book_item_size() {
        assert_eq!(mem::size_of::<OrderBookItem>(), 16);
    }

    #[test]
    fn test_trade_tick_item_size() {
        assert_eq!(mem::size_of::<TradeTickItem>(), 16);
    }

    #[test]
    fn test_flags_and_count() {
        let mut header = PacketHeader::new();
        header.set_flags_and_count(true, 42);
        
        assert!(header.is_last());
        assert_eq!(header.item_count(), 42);
    }

    #[test]
    fn test_trade_tick_flags() {
        let item = TradeTickItem::new(50000.0, 1.5, true);
        
        assert!(item.is_buyer_taker());
        assert_eq!(item.get_real_price(), 50000.0);
        assert_eq!(item.get_real_quantity(), 1.5);
    }

    #[test]
    fn test_scaling() {
        let item = OrderBookItem::new(42000.12345678, 0.00000001, true);
        
        // 스케일링 정확성 검증 (소수점 8자리까지)
        assert!((item.get_real_price() - 42000.12345678).abs() < 0.00000001);
        assert!((item.get_real_quantity() - 0.00000001).abs() < 0.000000001);
        assert!(item.is_ask());
    }
    
    #[test]
    fn test_orderbook_flags() {
        let ask_item = OrderBookItem::new(50000.0, 1.5, true);
        let bid_item = OrderBookItem::new(49999.0, 2.0, false);
        
        assert!(ask_item.is_ask());
        assert!(!bid_item.is_ask());
        assert_eq!(ask_item.get_real_price(), 50000.0);
        assert_eq!(ask_item.get_real_quantity(), 1.5);
        assert_eq!(bid_item.get_real_price(), 49999.0);
        assert_eq!(bid_item.get_real_quantity(), 2.0);
    }
}