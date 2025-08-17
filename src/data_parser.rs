/// 데이터 파서 모듈
/// simd-json을 사용하여 거래소별 JSON 데이터를 표준화된 구조체로 변환

use crate::errors::{CryptoFeederError, Result};
use crate::config::Config;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use simd_json;
use serde_json;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardizedTrade {
    pub symbol: String,
    pub exchange: String,
    pub price: f64,
    pub quantity: f64,
    pub is_buyer_taker: bool,
    pub timestamp: u64, // nanoseconds since Unix epoch
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardizedTradeBatch {
    pub symbol: String,
    pub exchange: String,
    pub exchange_timestamp: u64,
    pub trades: Vec<StandardizedTrade>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardizedOrderBookUpdate {
    pub symbol: String,
    pub exchange: String,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: u64, // nanoseconds since Unix epoch
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: f64,
    pub quantity: f64,
}

// Binance WebSocket 메시지 구조체
#[derive(Debug, Deserialize)]
struct BinanceDepthUpdate {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "U")]
    first_update_id: u64,
    #[serde(rename = "u")]
    final_update_id: u64,
    #[serde(rename = "b")]
    bids: Vec<[String; 2]>, // [price, quantity]
    #[serde(rename = "a")]
    asks: Vec<[String; 2]>, // [price, quantity]
}

#[derive(Debug, Deserialize)]
struct BinanceTrade {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "t")]
    trade_id: u64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    #[serde(rename = "b", default)]
    buyer_order_id: Option<u64>,
    #[serde(rename = "a", default)]
    seller_order_id: Option<u64>,
    #[serde(rename = "T")]
    trade_time: u64,
    #[serde(rename = "m")]
    is_buyer_market_maker: bool,
}

#[derive(Debug, Deserialize)]
struct BinanceAggTrade {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: u64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "a", default)]
    agg_trade_id: Option<u64>,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    quantity: String,
    #[serde(rename = "T")]
    trade_time: u64,
    #[serde(rename = "m")]
    is_buyer_market_maker: bool,
}

pub struct DataParser {
    // 거래소별 파서 함수 맵
    parsers: HashMap<String, fn(&mut [u8]) -> Result<ParsedData>>,
}

#[derive(Debug)]
pub enum ParsedData {
    Trade(StandardizedTrade),
    TradeBatch(StandardizedTradeBatch),
    OrderBook(StandardizedOrderBookUpdate),
    IndexPrice { symbol: String, exchange: String, value: f64, timestamp: u64 },
    MarkPrice { symbol: String, exchange: String, value: f64, timestamp: u64 },
    FundingRate { symbol: String, exchange: String, value: f64, timestamp: u64 },
    Liquidation { symbol: String, exchange: String, price: f64, quantity: f64, is_sell: bool, timestamp: u64 },
    Multi(Vec<ParsedData>),
}

impl DataParser {
    pub fn new() -> Self {
        Self::new_with_config(None)
    }

    pub fn new_with_config(config: Option<&Config>) -> Self {
        let mut parsers = HashMap::new();
        
        // 기본 Binance 파서 등록 (하위 호환성)
        let binance_parser = Self::parse_binance_message as fn(&mut [u8]) -> Result<ParsedData>;
        parsers.insert("binance".to_string(), binance_parser);
        
        // symbol_config가 있으면 거기서 거래소 목록 가져오기
        if let Some(config) = config {
            if let Some(symbol_config) = &config.symbol_config {
                for exchange_name in symbol_config.get_exchange_names() {
                    let parser = Self::get_parser_for_exchange(&exchange_name);
                    parsers.insert(exchange_name, parser);
                }
                debug!("symbol_config에서 {}개 거래소 파서 등록됨", parsers.len() - 1);
            } else {
                warn!("symbol_config가 없어서 기본 파서만 사용");
            }
        } else {
            warn!("Config가 제공되지 않아서 기본 파서만 사용");
        }
        
        Self { parsers }
    }

    /// 거래소명에 따라 적절한 파서 함수 반환
    fn get_parser_for_exchange(exchange_name: &str) -> fn(&mut [u8]) -> Result<ParsedData> {
        match exchange_name {
            name if name.starts_with("Binance") => Self::parse_binance_message,
            _ => Self::parse_default_message,
        }
    }

    /// 거래소별 메시지 파싱
    pub fn parse_message(&self, exchange: &str, mut data: Vec<u8>) -> Result<ParsedData> {
        debug!("파싱 시작 - 거래소: {}, 데이터 크기: {} bytes", exchange, data.len());
        
        let parser = self.parsers.get(exchange)
            .ok_or_else(|| CryptoFeederError::JsonParseError(
                format!("지원되지 않는 거래소: {}", exchange)
            ))?;

        let parsed = parser(&mut data)?;
        // 연결 컨텍스트에서 전달된 표시명으로 교체하여 시장 구분 보장 (예: BinanceSpot / BinanceFutures)
        let adjusted = match parsed {
            ParsedData::Trade(mut t) => { t.exchange = exchange.to_string(); ParsedData::Trade(t) }
            ParsedData::TradeBatch(mut b) => { b.exchange = exchange.to_string(); ParsedData::TradeBatch(b) }
            ParsedData::OrderBook(mut ob) => { ob.exchange = exchange.to_string(); ParsedData::OrderBook(ob) }
            ParsedData::IndexPrice { symbol, exchange: _, value, timestamp } => ParsedData::IndexPrice { symbol, exchange: exchange.to_string(), value, timestamp },
            ParsedData::MarkPrice { symbol, exchange: _, value, timestamp } => ParsedData::MarkPrice { symbol, exchange: exchange.to_string(), value, timestamp },
            ParsedData::FundingRate { symbol, exchange: _, value, timestamp } => ParsedData::FundingRate { symbol, exchange: exchange.to_string(), value, timestamp },
            ParsedData::Liquidation { symbol, exchange: _, price, quantity, is_sell, timestamp } => ParsedData::Liquidation { symbol, exchange: exchange.to_string(), price, quantity, is_sell, timestamp },
            ParsedData::Multi(items) => {
                let adj: Vec<ParsedData> = items.into_iter().map(|it| match it {
                    ParsedData::Trade(mut t) => { t.exchange = exchange.to_string(); ParsedData::Trade(t) }
                    ParsedData::TradeBatch(mut b) => { b.exchange = exchange.to_string(); ParsedData::TradeBatch(b) }
                    ParsedData::OrderBook(mut ob) => { ob.exchange = exchange.to_string(); ParsedData::OrderBook(ob) }
                    ParsedData::IndexPrice { symbol, exchange: _, value, timestamp } => ParsedData::IndexPrice { symbol, exchange: exchange.to_string(), value, timestamp },
                    ParsedData::MarkPrice { symbol, exchange: _, value, timestamp } => ParsedData::MarkPrice { symbol, exchange: exchange.to_string(), value, timestamp },
                    ParsedData::FundingRate { symbol, exchange: _, value, timestamp } => ParsedData::FundingRate { symbol, exchange: exchange.to_string(), value, timestamp },
                    ParsedData::Liquidation { symbol, exchange: _, price, quantity, is_sell, timestamp } => ParsedData::Liquidation { symbol, exchange: exchange.to_string(), price, quantity, is_sell, timestamp },
                    ParsedData::Multi(_) => ParsedData::Multi(Vec::new()),
                }).collect();
                ParsedData::Multi(adj)
            }
        };
        Ok(adjusted)
    }

    /// Binance 메시지 파싱
    fn parse_binance_message(data: &mut [u8]) -> Result<ParsedData> {
        // simd-json으로 1차 파싱
        let mut root = simd_json::from_slice::<serde_json::Value>(data)
            .map_err(|e| {
                let raw_text = String::from_utf8_lossy(data);
                debug!("JSON 파싱 실패 - 원본: {}", raw_text);
                CryptoFeederError::JsonParseError(format!("SIMD JSON 파싱 실패: {}", e))
            })?;

        // Binance Combined Stream 포맷 처리: { "stream": "...", "data": { ... 실제 이벤트 ... } }
        let event_obj = match root.get_mut("data") {
            Some(data_obj) => data_obj.take(), // take로 소유권 이동
            None => root,                       // 단일 스트림 포맷
        };

        let event_type = event_obj
            .get("e")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CryptoFeederError::JsonParseError("이벤트 타입 필드 누락".to_string()))?;

        debug!("Binance 이벤트 타입: {}", event_type);

        match event_type {
            "depthUpdate" => {
                debug!("Binance 오더북 업데이트 파싱 중");
                let update: BinanceDepthUpdate = serde_json::from_value(event_obj)
                    .map_err(|e| CryptoFeederError::JsonParseError(format!("오더북 업데이트 파싱 실패: {}", e)))?;

                debug!("Binance Depth 증분 업데이트: {} bids, {} asks", update.bids.len(), update.asks.len());
                Ok(ParsedData::OrderBook(Self::convert_binance_depth_update(update)?))
            }
            "markPriceUpdate" => {
                let sym = event_obj.get("s").and_then(|v| v.as_str()).ok_or_else(|| CryptoFeederError::JsonParseError("symbol 누락".into()))?;
                let mark = event_obj.get("p").and_then(|v| v.as_str()).ok_or_else(|| CryptoFeederError::JsonParseError("mark 누락".into()))?;
                let index = event_obj.get("i").and_then(|v| v.as_str()).ok_or_else(|| CryptoFeederError::JsonParseError("index 누락".into()))?;
                let funding = event_obj.get("r").and_then(|v| v.as_str()).ok_or_else(|| CryptoFeederError::JsonParseError("funding 누락".into()))?;
                let ts = event_obj.get("E").and_then(|v| v.as_u64()).unwrap_or(0) * 1_000_000;
                let symbol_std = Self::normalize_binance_symbol(sym);
                let exchange = Self::normalize_exchange_name("binance", "futures");
                let mark_f = mark.parse::<f64>().map_err(|e| CryptoFeederError::JsonParseError(format!("mark 파싱 실패: {}", e)))?;
                let index_f = index.parse::<f64>().map_err(|e| CryptoFeederError::JsonParseError(format!("index 파싱 실패: {}", e)))?;
                let funding_f = funding.parse::<f64>().map_err(|e| CryptoFeederError::JsonParseError(format!("funding 파싱 실패: {}", e)))?;
                Ok(ParsedData::Multi(vec![
                    ParsedData::IndexPrice { symbol: symbol_std.clone(), exchange: exchange.clone(), value: index_f, timestamp: ts },
                    ParsedData::MarkPrice { symbol: symbol_std.clone(), exchange: exchange.clone(), value: mark_f, timestamp: ts },
                    ParsedData::FundingRate { symbol: symbol_std, exchange, value: funding_f, timestamp: ts },
                ]))
            }
            "forceOrder" => {
                let o = event_obj.get("o").ok_or_else(|| CryptoFeederError::JsonParseError("forceOrder:o 누락".into()))?;
                let sym = o.get("s").and_then(|v| v.as_str()).ok_or_else(|| CryptoFeederError::JsonParseError("symbol 누락".into()))?;
                let side = o.get("S").and_then(|v| v.as_str()).unwrap_or("");
                let price = o.get("ap").or_else(|| o.get("p")).and_then(|v| v.as_str()).unwrap_or("0");
                let qty = o.get("q").and_then(|v| v.as_str()).unwrap_or("0");
                let ts = event_obj.get("E").and_then(|v| v.as_u64()).unwrap_or(0) * 1_000_000;
                let symbol_std = Self::normalize_binance_symbol(sym);
                let exchange = Self::normalize_exchange_name("binance", "futures");
                let price_f = price.parse::<f64>().unwrap_or(0.0);
                let qty_f = qty.parse::<f64>().unwrap_or(0.0);
                let is_sell = side.eq_ignore_ascii_case("SELL");
                Ok(ParsedData::Liquidation { symbol: symbol_std, exchange, price: price_f, quantity: qty_f, is_sell, timestamp: ts })
            }
            "trade" => {
                debug!("Binance 체결 데이터 파싱 중");
                let trade: BinanceTrade = serde_json::from_value(event_obj)
                    .map_err(|e| CryptoFeederError::JsonParseError(format!("체결 데이터 파싱 실패: {}", e)))?;
                Ok(ParsedData::Trade(Self::convert_binance_trade(trade)?))
            }
            "aggTrade" => {
                debug!("Binance aggTrade 체결 데이터 파싱 중");
                let trade: BinanceAggTrade = serde_json::from_value(event_obj)
                    .map_err(|e| CryptoFeederError::JsonParseError(format!("aggTrade 데이터 파싱 실패: {}", e)))?;
                let t = Self::convert_binance_agg_trade(trade)?;
                let batch = StandardizedTradeBatch {
                    symbol: t.symbol.clone(),
                    exchange: t.exchange.clone(),
                    exchange_timestamp: t.timestamp,
                    trades: vec![t],
                };
                Ok(ParsedData::TradeBatch(batch))
            }
            _ => {
                debug!("알 수 없는 Binance 이벤트 타입: {}", event_type);
                Err(CryptoFeederError::JsonParseError(format!("지원되지 않는 이벤트 타입: {}", event_type)))
            }
        }
    }

    /// Binance 오더북 업데이트를 표준화된 구조체로 변환
    fn convert_binance_depth_update(update: BinanceDepthUpdate) -> Result<StandardizedOrderBookUpdate> {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        // Bids 변환
        for bid in update.bids {
            let price = bid[0].parse::<f64>()
                .map_err(|e| CryptoFeederError::JsonParseError(format!("Bid 가격 파싱 실패: {}", e)))?;
            let quantity = bid[1].parse::<f64>()
                .map_err(|e| CryptoFeederError::JsonParseError(format!("Bid 수량 파싱 실패: {}", e)))?;
            
            bids.push(OrderBookLevel { price, quantity });
        }

        // Asks 변환
        for ask in update.asks {
            let price = ask[0].parse::<f64>()
                .map_err(|e| CryptoFeederError::JsonParseError(format!("Ask 가격 파싱 실패: {}", e)))?;
            let quantity = ask[1].parse::<f64>()
                .map_err(|e| CryptoFeederError::JsonParseError(format!("Ask 수량 파싱 실패: {}", e)))?;
            
            asks.push(OrderBookLevel { price, quantity });
        }

        Ok(StandardizedOrderBookUpdate {
            symbol: Self::normalize_binance_symbol(&update.symbol),
            exchange: Self::normalize_exchange_name("binance", "spot"),
            bids,
            asks,
            timestamp: update.event_time * 1_000_000, // milliseconds to nanoseconds
        })
    }

    /// Binance 체결 데이터를 표준화된 구조체로 변환
    fn convert_binance_trade(trade: BinanceTrade) -> Result<StandardizedTrade> {
        let price = trade.price.parse::<f64>()
            .map_err(|e| CryptoFeederError::JsonParseError(format!("체결 가격 파싱 실패: {}", e)))?;
        let quantity = trade.quantity.parse::<f64>()
            .map_err(|e| CryptoFeederError::JsonParseError(format!("체결 수량 파싱 실패: {}", e)))?;

        Ok(StandardizedTrade {
            symbol: Self::normalize_binance_symbol(&trade.symbol),
            exchange: Self::normalize_exchange_name("binance", "spot"),
            price,
            quantity,
            is_buyer_taker: !trade.is_buyer_market_maker, // 바이낸스는 market maker 플래그 제공
            timestamp: trade.trade_time * 1_000_000, // milliseconds to nanoseconds
        })
    }

    /// Binance aggTrade 데이터를 표준화된 구조체로 변환 (Futures 등에서 사용)
    fn convert_binance_agg_trade(trade: BinanceAggTrade) -> Result<StandardizedTrade> {
        let price = trade.price.parse::<f64>()
            .map_err(|e| CryptoFeederError::JsonParseError(format!("aggTrade 가격 파싱 실패: {}", e)))?;
        let quantity = trade.quantity.parse::<f64>()
            .map_err(|e| CryptoFeederError::JsonParseError(format!("aggTrade 수량 파싱 실패: {}", e)))?;

        Ok(StandardizedTrade {
            symbol: Self::normalize_binance_symbol(&trade.symbol),
            // exchange 표기는 호출자에서 세션 표시명으로 덮어씀 (parse_message에서)
            exchange: Self::normalize_exchange_name("binance", "futures"),
            price,
            quantity,
            is_buyer_taker: !trade.is_buyer_market_maker,
            timestamp: trade.trade_time * 1_000_000,
        })
    }

    /// 기본 메시지 파서 (다른 거래소용 - 현재는 로그만 출력)
    fn parse_default_message(data: &mut [u8]) -> Result<ParsedData> {
        let raw_text = String::from_utf8_lossy(data);
        debug!("기본 파서로 메시지 처리 중 (아직 구현되지 않음): {}", 
               &raw_text[..std::cmp::min(100, raw_text.len())]);
        
        // 임시로 더미 데이터 반환 (실제로는 거래소별 파싱 로직 필요)
        Ok(ParsedData::Trade(StandardizedTrade {
            symbol: "UNKNOWN^USDT".to_string(),
            exchange: "Unknown".to_string(),
            price: 0.0,
            quantity: 0.0,
            is_buyer_taker: false,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        }))
    }

    /// Binance 심볼을 표준 형식으로 변환 (BTCUSDT -> BTC^USDT)
    /// A^B 형태에서 A는 거래 코인, B는 통화 화폐
    fn normalize_binance_symbol(symbol: &str) -> String {
        let upper_symbol = symbol.to_uppercase();
        
        // 주요 quote currency 순서대로 확인 (긴 것부터)
        if upper_symbol.ends_with("USDT") {
            let base = &upper_symbol[..upper_symbol.len() - 4];
            format!("{}^USDT", base)
        } else if upper_symbol.ends_with("USDC") {
            let base = &upper_symbol[..upper_symbol.len() - 4];
            format!("{}^USDC", base)
        } else if upper_symbol.ends_with("BUSD") {
            let base = &upper_symbol[..upper_symbol.len() - 4];
            format!("{}^BUSD", base)
        } else if upper_symbol.ends_with("BTC") {
            let base = &upper_symbol[..upper_symbol.len() - 3];
            format!("{}^BTC", base)
        } else if upper_symbol.ends_with("ETH") {
            let base = &upper_symbol[..upper_symbol.len() - 3];
            format!("{}^ETH", base)
        } else if upper_symbol.ends_with("BNB") {
            let base = &upper_symbol[..upper_symbol.len() - 3];
            format!("{}^BNB", base)
        } else {
            symbol.to_string()
        }
    }

    /// 거래소 이름을 표준 형식으로 변환
    fn normalize_exchange_name(exchange: &str, market_type: &str) -> String {
        match exchange.to_lowercase().as_str() {
            "binance" => {
                match market_type {
                    "spot" => "BinanceSpot".to_string(),
                    "futures" => "BinanceFutures".to_string(),
                    _ => "BinanceSpot".to_string(), // 기본값은 현물
                }
            },
            "bybit" => {
                match market_type {
                    "spot" => "BybitSpot".to_string(),
                    "linear" => "BybitLinear".to_string(),
                    "inverse" => "BybitInverse".to_string(),
                    _ => "BybitSpot".to_string(),
                }
            },
            "okx" => {
                match market_type {
                    "spot" => "OkxSpot".to_string(),
                    "swap" => "OkxSwap".to_string(),
                    "futures" => "OkxFutures".to_string(),
                    _ => "OkxSpot".to_string(),
                }
            },
            _ => format!("{}_{}", exchange, market_type),
        }
    }
}

impl Default for DataParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_symbol_normalization() {
        assert_eq!(DataParser::normalize_binance_symbol("BTCUSDT"), "BTC^USDT");
        assert_eq!(DataParser::normalize_binance_symbol("ETHUSDT"), "ETH^USDT");
        assert_eq!(DataParser::normalize_binance_symbol("ADABTC"), "ADA^BTC");
    }

    #[test]
    fn test_exchange_normalization() {
        assert_eq!(DataParser::normalize_exchange_name("binance", "spot"), "BinanceSpot");
        assert_eq!(DataParser::normalize_exchange_name("binance", "futures"), "BinanceFutures");
        assert_eq!(DataParser::normalize_exchange_name("bybit", "linear"), "BybitLinear");
        assert_eq!(DataParser::normalize_exchange_name("okx", "swap"), "OkxSwap");
    }

    #[test]
    fn test_parser_creation() {
        let parser = DataParser::new();
        assert!(parser.parsers.contains_key("binance"));
    }

    #[test]
    fn test_parse_binance_combined_trade() {
        let json = r#"{
            "stream":"btcusdt@trade",
            "data":{
                "e":"trade","E":1700000000000,"s":"BTCUSDT","t":1,
                "p":"50000.00","q":"0.10000000","T":1700000000000,
                "m":false
            }
        }"#;
        let mut bytes = json.as_bytes().to_vec();
        let res = (DataParser::parse_binance_message)(&mut bytes);
        match res {
            Ok(ParsedData::Trade(t)) => {
                assert_eq!(t.symbol, "BTC^USDT");
                assert_eq!(t.exchange, "BinanceSpot");
                assert!(t.is_buyer_taker); // m=false => buyer taker
                assert_eq!(t.price, 50000.0);
            }
            _ => panic!("unexpected parse result"),
        }
    }

    #[test]
    fn test_parse_binance_combined_depth() {
        let json = r#"{
            "stream":"ethusdt@depth",
            "data":{
                "e":"depthUpdate","E":1700000001000,"s":"ETHUSDT",
                "U":100,"u":110,
                "b":[["3000.10","1.00000000"]],
                "a":[["3001.20","2.00000000"]]
            }
        }"#;
        let mut bytes = json.as_bytes().to_vec();
        let res = (DataParser::parse_binance_message)(&mut bytes);
        match res {
            Ok(ParsedData::OrderBook(ob)) => {
                assert_eq!(ob.symbol, "ETH^USDT");
                assert_eq!(ob.exchange, "BinanceSpot");
                assert_eq!(ob.bids.len(), 1);
                assert_eq!(ob.asks.len(), 1);
            }
            _ => panic!("unexpected parse result"),
        }
    }
}