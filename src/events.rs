/// 이벤트 패킷 구조체 및 관리
/// event_packet.md 명세에 따라 구현

use std::mem;
use serde::{Serialize, Deserialize};

// 이벤트 메시지 타입 상수
pub const MESSAGE_TYPE_SYSTEM_HEARTBEAT: u8 = 100;
pub const MESSAGE_TYPE_CONNECTION_STATUS: u8 = 101;
pub const MESSAGE_TYPE_SUBSCRIPTION_STATUS: u8 = 102;
pub const MESSAGE_TYPE_SYSTEM_STATS: u8 = 103;
pub const MESSAGE_TYPE_ERROR_EVENT: u8 = 104;

// 거래소 ID 상수
pub const EXCHANGE_ID_BINANCE: u16 = 1;
pub const EXCHANGE_ID_OKX: u16 = 2;
pub const EXCHANGE_ID_BYBIT: u16 = 3;
pub const EXCHANGE_ID_UPBIT: u16 = 4;
pub const EXCHANGE_ID_BITHUMB: u16 = 5;
pub const EXCHANGE_ID_COINBASE: u16 = 6;

// 연결 상태 상수
pub const CONNECTION_STATUS_DISCONNECTED: u8 = 0;
pub const CONNECTION_STATUS_CONNECTING: u8 = 1;
pub const CONNECTION_STATUS_CONNECTED: u8 = 2;
pub const CONNECTION_STATUS_RECONNECTING: u8 = 3;
pub const CONNECTION_STATUS_FAILED: u8 = 4;

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct SystemHeartbeat {
    pub uptime_seconds: u64,
    pub active_connections: u32,
    pub total_packets_sent: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub exchange_id: u16,
    pub previous_status: u8,
    pub current_status: u8,
    pub retry_count: u32,
    pub error_code: u64,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatus {
    pub exchange_id: u16,
    pub subscription_type: u8,
    pub status: u8,
    pub symbol_short: [u8; 12],
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_usage_percent: u32,
    pub memory_usage_mb: u32,
    pub packets_per_second: u32,
    pub bytes_per_second: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub error_type: u32,
    pub exchange_id: u16,
    pub severity: u16,
    pub error_details: u64,
}

// 이벤트 타입 enum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    Heartbeat(SystemHeartbeat),
    ConnectionStatus(ConnectionStatus),
    SubscriptionStatus(SubscriptionStatus),
    SystemStats(SystemStats),
    ErrorEvent(ErrorEvent),
}

impl SystemHeartbeat {
    pub fn new(uptime_seconds: u64, active_connections: u32, total_packets_sent: u32) -> Self {
        Self {
            uptime_seconds,
            active_connections,
            total_packets_sent,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl ConnectionStatus {
    pub fn new(exchange_id: u16, previous_status: u8, current_status: u8, retry_count: u32, error_code: u64) -> Self {
        Self {
            exchange_id,
            previous_status,
            current_status,
            retry_count,
            error_code,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl SubscriptionStatus {
    pub fn new(exchange_id: u16, subscription_type: u8, status: u8, symbol: &str) -> Self {
        let mut symbol_short = [0u8; 12];
        let bytes = symbol.as_bytes();
        let len = bytes.len().min(11); // null terminator를 위한 공간 확보
        symbol_short[..len].copy_from_slice(&bytes[..len]);
        
        Self {
            exchange_id,
            subscription_type,
            status,
            symbol_short,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl SystemStats {
    pub fn new(cpu_usage_percent: u32, memory_usage_mb: u32, packets_per_second: u32, bytes_per_second: u32) -> Self {
        Self {
            cpu_usage_percent,
            memory_usage_mb,
            packets_per_second,
            bytes_per_second,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl ErrorEvent {
    pub fn new(error_type: u32, exchange_id: u16, severity: u16, error_details: u64) -> Self {
        Self {
            error_type,
            exchange_id,
            severity,
            error_details,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let ptr = self as *const Self as *const u8;
            std::slice::from_raw_parts(ptr, mem::size_of::<Self>()).to_vec()
        }
    }
}

impl SystemEvent {
    pub fn get_message_type(&self) -> u8 {
        match self {
            SystemEvent::Heartbeat(_) => MESSAGE_TYPE_SYSTEM_HEARTBEAT,
            SystemEvent::ConnectionStatus(_) => MESSAGE_TYPE_CONNECTION_STATUS,
            SystemEvent::SubscriptionStatus(_) => MESSAGE_TYPE_SUBSCRIPTION_STATUS,
            SystemEvent::SystemStats(_) => MESSAGE_TYPE_SYSTEM_STATS,
            SystemEvent::ErrorEvent(_) => MESSAGE_TYPE_ERROR_EVENT,
        }
    }

    pub fn get_payload_bytes(&self) -> Vec<u8> {
        match self {
            SystemEvent::Heartbeat(h) => h.to_bytes(),
            SystemEvent::ConnectionStatus(c) => c.to_bytes(),
            SystemEvent::SubscriptionStatus(s) => s.to_bytes(),
            SystemEvent::SystemStats(s) => s.to_bytes(),
            SystemEvent::ErrorEvent(e) => e.to_bytes(),
        }
    }

    pub fn get_symbol(&self) -> String {
        match self {
            SystemEvent::SubscriptionStatus(s) => {
                let end = s.symbol_short.iter().position(|&b| b == 0).unwrap_or(s.symbol_short.len());
                String::from_utf8_lossy(&s.symbol_short[..end]).to_string()
            },
            _ => "SYSTEM".to_string(),
        }
    }

    pub fn get_exchange(&self) -> String {
        let exchange_id = match self {
            SystemEvent::ConnectionStatus(c) => c.exchange_id,
            SystemEvent::SubscriptionStatus(s) => s.exchange_id,
            SystemEvent::ErrorEvent(e) => e.exchange_id,
            _ => return "FEEDER".to_string(),
        };
        
        match exchange_id {
            EXCHANGE_ID_BINANCE => "binance".to_string(),
            EXCHANGE_ID_OKX => "okx".to_string(),
            EXCHANGE_ID_BYBIT => "bybit".to_string(),
            EXCHANGE_ID_UPBIT => "upbit".to_string(),
            EXCHANGE_ID_BITHUMB => "bithumb".to_string(),
            EXCHANGE_ID_COINBASE => "coinbase".to_string(),
            _ => "unknown".to_string(),
        }
    }
}

// 컴파일 타임에 구조체 크기 검증
const _: () = assert!(mem::size_of::<SystemHeartbeat>() == 16);
const _: () = assert!(mem::size_of::<ConnectionStatus>() == 16);
const _: () = assert!(mem::size_of::<SubscriptionStatus>() == 16);
const _: () = assert!(mem::size_of::<SystemStats>() == 16);
const _: () = assert!(mem::size_of::<ErrorEvent>() == 16);

/// 거래소 이름을 거래소 ID로 변환
pub fn exchange_name_to_id(name: &str) -> u16 {
    match name.to_lowercase().as_str() {
        "binance" => EXCHANGE_ID_BINANCE,
        "okx" => EXCHANGE_ID_OKX,
        "bybit" => EXCHANGE_ID_BYBIT,
        "upbit" => EXCHANGE_ID_UPBIT,
        "bithumb" => EXCHANGE_ID_BITHUMB,
        "coinbase" => EXCHANGE_ID_COINBASE,
        _ => 0, // 알 수 없는 거래소
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_struct_sizes() {
        assert_eq!(mem::size_of::<SystemHeartbeat>(), 16);
        assert_eq!(mem::size_of::<ConnectionStatus>(), 16);
        assert_eq!(mem::size_of::<SubscriptionStatus>(), 16);
        assert_eq!(mem::size_of::<SystemStats>(), 16);
        assert_eq!(mem::size_of::<ErrorEvent>(), 16);
    }

    #[test]
    fn test_exchange_id_mapping() {
        assert_eq!(exchange_name_to_id("binance"), EXCHANGE_ID_BINANCE);
        assert_eq!(exchange_name_to_id("Binance"), EXCHANGE_ID_BINANCE);
        assert_eq!(exchange_name_to_id("BINANCE"), EXCHANGE_ID_BINANCE);
        assert_eq!(exchange_name_to_id("unknown"), 0);
    }

    #[test]
    fn test_system_event_methods() {
        let heartbeat = SystemEvent::Heartbeat(SystemHeartbeat::new(3600, 5, 12345));
        
        assert_eq!(heartbeat.get_message_type(), MESSAGE_TYPE_SYSTEM_HEARTBEAT);
        assert_eq!(heartbeat.get_symbol(), "SYSTEM");
        assert_eq!(heartbeat.get_exchange(), "FEEDER");
        assert_eq!(heartbeat.get_payload_bytes().len(), 16);
    }

    #[test]
    fn test_connection_status_event() {
        let event = SystemEvent::ConnectionStatus(ConnectionStatus::new(
            EXCHANGE_ID_BINANCE,
            CONNECTION_STATUS_DISCONNECTED,
            CONNECTION_STATUS_CONNECTING,
            1,
            0
        ));
        
        assert_eq!(event.get_message_type(), MESSAGE_TYPE_CONNECTION_STATUS);
        assert_eq!(event.get_exchange(), "binance");
    }
}