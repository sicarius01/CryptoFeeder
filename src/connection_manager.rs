/// WebSocket 연결 관리자
/// 거래소별 WebSocket 연결 생성, 유지, 모니터링 및 재연결 담당

use crate::config::{Config, ExchangeConfig, SymbolSession, ExchangeEndpoint};
use crate::data_parser::DataParser;
use crate::packet_builder::PacketBuilder;
use crate::udp_broadcaster::UdpMulticaster;
use crate::errors::{CryptoFeederError, Result};
use crate::events::{
    SystemEvent,
    ConnectionStatus,
    exchange_name_to_id,
    CONNECTION_STATUS_CONNECTING,
    CONNECTION_STATUS_CONNECTED,
    CONNECTION_STATUS_DISCONNECTED,
    CONNECTION_STATUS_RECONNECTING,
    CONNECTION_STATUS_FAILED,
};

use futures_util::{SinkExt, StreamExt};
use log::{info, warn, error, debug};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;

pub struct ConnectionManager {
    config: Arc<Config>,
    data_parser: Arc<DataParser>,
    packet_builder: Arc<PacketBuilder>,
    udp_broadcaster: Arc<UdpMulticaster>,
}

impl ConnectionManager {
    pub fn new(
        config: Arc<Config>,
        data_parser: Arc<DataParser>,
        packet_builder: Arc<PacketBuilder>,
        udp_broadcaster: Arc<UdpMulticaster>,
    ) -> Self {
        Self {
            config,
            data_parser,
            packet_builder,
            udp_broadcaster,
        }
    }

    /// 모든 연결 시작
    pub async fn run(&self) -> Result<()> {
        info!("🌐 연결 관리자 시작");

        let mut handles = Vec::new();

        // symbol_config가 있으면 그것을 우선 사용, 없으면 기본 거래소 설정 사용
        if let Some(symbol_config) = &self.config.symbol_config {
            info!("📋 심볼 설정 파일 기반으로 연결 생성");
            
            // 각 거래소의 세션별 연결 작업 생성
            for exchange_name in symbol_config.get_exchange_names() {
                if let Some(sessions) = symbol_config.get_exchange_sessions(&exchange_name) {
                    for (session_idx, session) in sessions.iter().enumerate() {
                        let manager = self.clone();
                        let exchange_name_clone = exchange_name.clone();
                        let session_clone = session.clone();
                        
                        let handle = tokio::spawn(async move {
                            manager.manage_symbol_session(&exchange_name_clone, session_idx, &session_clone).await
                        });
                        
                        handles.push(handle);
                    }
                }
            }
        } else {
            info!("⚠️ 심볼 설정 파일이 없음. 기본 거래소 설정 사용");
            
            // 기존 방식으로 각 거래소에 대한 연결 작업 생성
            for exchange_config in &self.config.exchanges {
                if !exchange_config.enabled {
                    info!("⏭️ {} 거래소는 비활성화됨", exchange_config.name);
                    continue;
                }

                let manager = self.clone();
                let exchange_config = exchange_config.clone();
                
                let handle = tokio::spawn(async move {
                    manager.manage_exchange_connection(exchange_config).await
                });
                
                handles.push(handle);
            }
        }

        // 모든 연결 작업 완료 대기
        for handle in handles {
            if let Err(e) = handle.await {
                error!("연결 관리 작업 실패: {}", e);
            }
        }

        Ok(())
    }

    /// 단일 거래소 연결 관리 (재연결 로직 포함)
    async fn manage_exchange_connection(&self, exchange_config: ExchangeConfig) -> Result<()> {
        let mut retry_count = 0;
        const MAX_RETRY_COUNT: u32 = 10; // 최대 10회 시도 후 포기

        loop {
            info!("🔌 {} 거래소 연결 시도 중... (시도 #{}/{})", exchange_config.name, retry_count + 1, MAX_RETRY_COUNT);

            match self.connect_to_exchange(&exchange_config).await {
                Ok(_) => {
                    info!("✅ {} 거래소 연결 성공", exchange_config.name);
                    retry_count = 0; // 성공 시 재시도 카운트 리셋
                },
                Err(e) => {
                    error!("❌ {} 거래소 연결 실패: {}", exchange_config.name, e);
                    
                    retry_count += 1;
                    
                    // 최대 재시도 횟수 초과 시 포기
                    if retry_count >= MAX_RETRY_COUNT {
                        error!("💀 {} 거래소 최대 재시도 횟수({}) 초과. 연결 포기", exchange_config.name, MAX_RETRY_COUNT);
                        return Err(CryptoFeederError::Other(
                            format!("{} 거래소 연결 실패 - 최대 재시도 횟수 초과", exchange_config.name)
                        ));
                    }
                    
                    // 지수 백오프 재연결 로직
                    let delay = self.calculate_backoff_delay(retry_count - 1);
                    warn!("🔄 {}초 후 {} 거래소 재연결 시도", delay.as_secs(), exchange_config.name);
                    
                    time::sleep(delay).await;
                }
            }
        }
    }

    /// 단일 거래소에 연결
    async fn connect_to_exchange(&self, exchange_config: &ExchangeConfig) -> Result<()> {
        let url = Url::parse(&exchange_config.ws_url)?;
        info!("🚀 {} 연결 중: {}", exchange_config.name, url);

        // WebSocket 연결
        let (ws_stream, response) = connect_async(url).await
            .map_err(|e| CryptoFeederError::WebSocketError(e))?;

        info!("🤝 {} WebSocket 연결 성공 (상태: {})", 
              exchange_config.name, response.status());

        let (mut write, mut read) = ws_stream.split();

        // Ping은 별도 구현하지 않고 WebSocket 내장 핑 사용
        let ping_handle: Option<tokio::task::JoinHandle<()>> = None;

        // 메시지 수신 루프
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    debug!("📨 {} 텍스트 메시지 수신: {} bytes", exchange_config.name, text.len());
                    
                    if let Err(e) = self.process_message(&exchange_config.name, text.into_bytes()).await {
                        error!("❌ {} 메시지 처리 실패: {}", exchange_config.name, e);
                        // 메시지 처리 실패는 연결을 끊지 않음
                    }
                },
                Ok(Message::Binary(data)) => {
                    debug!("📨 {} 바이너리 메시지 수신: {} bytes", exchange_config.name, data.len());
                    
                    if let Err(e) = self.process_message(&exchange_config.name, data).await {
                        error!("❌ {} 메시지 처리 실패: {}", exchange_config.name, e);
                    }
                },
                Ok(Message::Ping(data)) => {
                    debug!("🏓 {} Ping 수신, Pong 응답 중", exchange_config.name);
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        error!("❌ {} Pong 응답 실패: {}", exchange_config.name, e);
                        break;
                    }
                },
                Ok(Message::Pong(_)) => {
                    debug!("🏓 {} Pong 수신됨", exchange_config.name);
                },
                Ok(Message::Close(frame)) => {
                    info!("🔒 {} 연결 정상 종료: {:?}", exchange_config.name, frame);
                    break;
                },
                Ok(Message::Frame(_)) => {
                    // Raw frame 메시지는 무시
                    debug!("🔧 {} Raw frame 수신", exchange_config.name);
                },
                Err(e) => {
                    error!("❌ {} WebSocket 오류: {}", exchange_config.name, e);
                    break;
                }
            }
        }

        // Ping 작업 정리
        if let Some(handle) = ping_handle {
            handle.abort();
        }

        warn!("🔌 {} WebSocket 연결 종료됨", exchange_config.name);
        Err(CryptoFeederError::Other(format!("{} 연결 종료", exchange_config.name)))
    }

    /// 수신된 메시지 처리
    async fn process_message(&self, exchange: &str, data: Vec<u8>) -> Result<()> {
        // 데이터 파싱
        let parsed_data = self.data_parser.parse_message(exchange, data)?;

        // UDP 패킷 생성 및 전송
        // TradeTick의 배치 전송을 극대화하기 위해, 같은 폴링 사이클의 여러 Trade를 묶을 수 있는 경로를 우선 사용
        let packets = self.packet_builder.build_packets(parsed_data)?;
        
        for packet in packets {
            self.udp_broadcaster.send_packet(packet).await?;
        }

        Ok(())
    }

    /// 지수 백오프 지연 시간 계산
    fn calculate_backoff_delay(&self, retry_count: u32) -> Duration {
        // 최초 시도: 1초
        // 이후 시도: 2배씩 증가 (2초, 4초, 8초, 16초, 32초, 60초 최대)
        let base_delay = 1000; // 1초 (milliseconds)
        let max_delay = 60000;  // 60초 (milliseconds)
        
        let delay_ms = if retry_count == 0 {
            base_delay
        } else {
            (base_delay * (2_u64.pow(retry_count.min(6)))).min(max_delay)
        };

        Duration::from_millis(delay_ms)
    }

    /// 심볼 세션별 연결 관리 (재연결 로직 포함)
    async fn manage_symbol_session(&self, exchange_name: &str, session_idx: usize, session: &SymbolSession) -> Result<()> {
        let mut retry_count = 0;
        const MAX_RETRY_COUNT: u32 = 10;

        let session_type = if session.is_btc_session { "BTC" } else { "일반" };
        let symbols_str = session.symbols.join(", ");

        loop {
            info!("🔌 {} [{}세션 #{}] 연결 시도 중... (심볼: {}) (시도 #{}/{})", 
                  exchange_name, session_type, session_idx, symbols_str, retry_count + 1, MAX_RETRY_COUNT);

            // 상태 이벤트: CONNECTING (표시용 거래소명 그대로 기록)
            let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_DISCONNECTED, CONNECTION_STATUS_CONNECTING, retry_count, 0).await;

            match self.connect_to_symbol_session(exchange_name, session_idx, session).await {
                Ok(_) => {
                    info!("✅ {} [{}세션 #{}] 연결 성공", exchange_name, session_type, session_idx);
                    // 상태 이벤트: CONNECTED (표시용 거래소명 그대로 기록)
                    let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_CONNECTING, CONNECTION_STATUS_CONNECTED, retry_count, 0).await;
                    retry_count = 0; // 성공 시 재시도 카운트 리셋
                },
                Err(e) => {
                    error!("❌ {} [{}세션 #{}] 연결 실패: {}", exchange_name, session_type, session_idx, e);
                    
                    retry_count += 1;
                    
                    if retry_count >= MAX_RETRY_COUNT {
                        error!("💀 {} [{}세션 #{}] 최대 재시도 횟수({}) 초과. 연결 포기", 
                               exchange_name, session_type, session_idx, MAX_RETRY_COUNT);
                        // 상태 이벤트: FAILED (표시용 거래소명 그대로 기록)
                        let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_RECONNECTING, CONNECTION_STATUS_FAILED, retry_count, 0).await;
                        return Err(CryptoFeederError::Other(
                            format!("{} [{}세션 #{}] 연결 실패 - 최대 재시도 횟수 초과", 
                                    exchange_name, session_type, session_idx)
                        ));
                    }
                    
                    let delay = self.calculate_backoff_delay(retry_count - 1);
                    warn!("🔄 {}초 후 {} [{}세션 #{}] 재연결 시도", 
                          delay.as_secs(), exchange_name, session_type, session_idx);
                    // 상태 이벤트: RECONNECTING (표시용 거래소명 그대로 기록)
                    let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_CONNECTED, CONNECTION_STATUS_RECONNECTING, retry_count, 0).await;
                    
                    time::sleep(delay).await;
                }
            }
        }
    }

    /// 심볼 세션에 대한 WebSocket 연결
    async fn connect_to_symbol_session(&self, exchange_name: &str, session_idx: usize, session: &SymbolSession) -> Result<()> {
        // endpoint.ini에서 거래소 엔드포인트 정보 가져오기
        let ws_url = if let Some(endpoint_config) = &self.config.endpoint_config {
            if let Some(endpoint) = endpoint_config.get_exchange_endpoint(exchange_name) {
                if !endpoint.enabled {
                    warn!("⚠️ {} 거래소가 비활성화됨. 연결을 건너뜁니다.", exchange_name);
                    return Err(CryptoFeederError::Other(format!("{} 거래소가 비활성화됨", exchange_name)));
                }
                self.build_websocket_url_from_endpoint(endpoint, &session.symbols)?
            } else {
                warn!("⚠️ {} 거래소 엔드포인트 설정을 찾을 수 없음. 기본 URL 사용", exchange_name);
                self.build_default_websocket_url(exchange_name, &session.symbols)?
            }
        } else {
            warn!("⚠️ endpoint.ini 파일이 없음. 기본 URL 사용");
            self.build_default_websocket_url(exchange_name, &session.symbols)?
        };

        let url = Url::parse(&ws_url)?;
        info!("🚀 {} [세션 #{}] 연결 중: {}", exchange_name, session_idx, url);

        // WebSocket 연결
        let (ws_stream, response) = connect_async(url).await
            .map_err(|e| CryptoFeederError::WebSocketError(e))?;

        info!("🤝 {} [세션 #{}] WebSocket 연결 성공 (상태: {})", 
              exchange_name, session_idx, response.status());

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // 구독 메시지 전송 (거래소별로 다름)
        if let Some(subscription_msg) = self.build_subscription_message(exchange_name, &session.symbols)? {
            ws_sender.send(subscription_msg).await
                .map_err(|e| CryptoFeederError::WebSocketError(e))?;
            info!("📨 {} [세션 #{}] 구독 메시지 전송 완료", exchange_name, session_idx);
        }

        // 메시지 수신 루프
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("📥 {} [세션 #{}] 텍스트 메시지 수신: {} bytes", 
                           exchange_name, session_idx, text.len());
                    
                    if let Err(e) = self.process_message(exchange_name, text.into_bytes()).await {
                        error!("❌ {} [세션 #{}] 메시지 처리 실패: {}", exchange_name, session_idx, e);
                    }
                },
                Ok(Message::Binary(data)) => {
                    debug!("📥 {} [세션 #{}] 바이너리 메시지 수신: {} bytes", 
                           exchange_name, session_idx, data.len());
                    
                    if let Err(e) = self.process_message(exchange_name, data).await {
                        error!("❌ {} [세션 #{}] 메시지 처리 실패: {}", exchange_name, session_idx, e);
                    }
                },
                Ok(Message::Close(_)) => {
                    info!("🔌 {} [세션 #{}] WebSocket 연결 종료됨", exchange_name, session_idx);
                    // 상태 이벤트: DISCONNECTED (표시용 거래소명 그대로 기록)
                    let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_CONNECTED, CONNECTION_STATUS_DISCONNECTED, 0, 0).await;
                    break;
                },
                Ok(Message::Ping(payload)) => {
                    debug!("🏓 {} [세션 #{}] Ping 수신, Pong 응답", exchange_name, session_idx);
                    ws_sender.send(Message::Pong(payload)).await
                        .map_err(|e| CryptoFeederError::WebSocketError(e))?;
                },
                Ok(Message::Pong(_)) => {
                    debug!("🏓 {} [세션 #{}] Pong 수신", exchange_name, session_idx);
                },
                Ok(Message::Frame(_)) => {
                    // Frame 메시지는 일반적으로 내부적으로 처리되므로 무시
                    debug!("🔧 {} [세션 #{}] Frame 메시지 수신 (무시)", exchange_name, session_idx);
                },
                Err(e) => {
                    error!("❌ {} [세션 #{}] WebSocket 오류: {}", exchange_name, session_idx, e);
                    // 상태 이벤트: DISCONNECTED (표시용 거래소명 그대로 기록)
                    let _ = self.send_connection_event(exchange_name, CONNECTION_STATUS_CONNECTED, CONNECTION_STATUS_DISCONNECTED, 0, 0).await;
                    return Err(CryptoFeederError::WebSocketError(e));
                }
            }
        }

        info!("🔚 {} [세션 #{}] 메시지 수신 루프 종료", exchange_name, session_idx);
        Err(CryptoFeederError::Other("WebSocket 연결이 예기치 않게 종료됨".to_string()))
    }

    /// endpoint.ini 설정을 기반으로 WebSocket URL 생성
    fn build_websocket_url_from_endpoint(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        match endpoint.exchange_name.as_str() {
            name if name.starts_with("Binance") => {
                self.build_binance_websocket_url_from_endpoint(endpoint, symbols)
            },
            name if name.starts_with("Okx") => {
                self.build_okx_websocket_url(endpoint, symbols)
            },
            name if name.starts_with("Bybit") => {
                self.build_bybit_websocket_url(endpoint, symbols)
            },
            name if name.starts_with("Upbit") => {
                self.build_upbit_websocket_url(endpoint, symbols)
            },
            name if name.starts_with("Bithumb") => {
                self.build_bithumb_websocket_url(endpoint, symbols)
            },
            name if name.starts_with("Coinbase") => {
                self.build_coinbase_websocket_url(endpoint, symbols)
            },
            _ => {
                warn!("⚠️ {} 거래소의 상세 URL 생성 로직이 구현되지 않음. 기본 URL 사용", endpoint.exchange_name);
                Ok(endpoint.ws_url_base.clone())
            }
        }
    }

    /// 기본 WebSocket URL 생성 (endpoint.ini가 없을 때)
    fn build_default_websocket_url(&self, exchange_name: &str, symbols: &[String]) -> Result<String> {
        match exchange_name {
            "BinanceSpot" => {
                self.build_binance_websocket_url_legacy("wss://stream.binance.com:9443/ws/", symbols)
            },
            "BinanceFutures" => {
                self.build_binance_websocket_url_legacy("wss://fstream.binance.com/ws/", symbols)
            },
            _ => {
                warn!("⚠️ {} 거래소의 기본 URL을 찾을 수 없음. 임시 URL 사용", exchange_name);
                Ok("wss://stream.binance.com:9443/ws/btcusdt@trade".to_string())
            }
        }
    }

    /// endpoint 설정을 사용한 Binance WebSocket URL 생성
    fn build_binance_websocket_url_from_endpoint(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        self.build_binance_websocket_url_legacy(&endpoint.ws_url_base, symbols)
    }

    /// 레거시 Binance WebSocket URL 생성 (기존 로직)
    fn build_binance_websocket_url_legacy(&self, base_url: &str, symbols: &[String]) -> Result<String> {
        // Combined Stream: Spot uses @trade, Futures uses @aggTrade
        let is_futures = base_url.contains("fstream.binance.com");
        let trade_topic = if is_futures { "aggTrade" } else { "trade" };
        let depth_topic = "depth"; // consider @100ms if needed
        let mut streams: Vec<String> = Vec::new();
        for symbol in symbols {
            let binance_symbol = symbol.replace("^", "").to_lowercase();
            streams.push(format!("{}@{}", &binance_symbol, trade_topic));
            streams.push(format!("{}@{}", &binance_symbol, depth_topic));
        }
        let url = if base_url.ends_with("/ws/") {
            // 과거 단일 스트림 베이스가 들어온 경우, combined stream 엔드포인트로 교체
            base_url.replace("/ws/", "/stream?") + &format!("streams={}", streams.join("/"))
        } else if base_url.ends_with("/ws") {
            base_url.replace("/ws", "/stream?") + &format!("streams={}", streams.join("/"))
        } else if base_url.ends_with("/stream") || base_url.ends_with("/stream/") {
            let sep = if base_url.ends_with('/') { "" } else { "/" };
            format!("{}{}?streams={}", base_url, sep, streams.join("/"))
        } else {
            // 기본 가정: /stream? 기반
            let sep = if base_url.ends_with('/') { "" } else { "/" };
            format!("{}{}stream?streams={}", base_url, sep, streams.join("/"))
        };
        Ok(url)
    }

    /// OKX WebSocket URL 생성
    fn build_okx_websocket_url(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        // OKX는 구독 후 메시지로 심볼을 지정하므로 기본 URL만 사용
        info!("🔗 OKX {} 세션 연결 준비: {} 심볼", endpoint.exchange_name, symbols.len());
        for symbol in symbols {
            debug!("   └─ 심볼: {}", symbol);
        }
        Ok(endpoint.ws_url_base.clone())
    }

    /// Bybit WebSocket URL 생성  
    fn build_bybit_websocket_url(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        // Bybit은 구독 후 메시지로 심볼을 지정하므로 기본 URL만 사용
        info!("🔗 Bybit {} 세션 연결 준비: {} 심볼", endpoint.exchange_name, symbols.len());
        for symbol in symbols {
            debug!("   └─ 심볼: {}", symbol);
        }
        Ok(endpoint.ws_url_base.clone())
    }

    /// Upbit WebSocket URL 생성
    fn build_upbit_websocket_url(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        // Upbit은 연결 후 별도 구독 메시지로 심볼 지정
        info!("🔗 Upbit {} 세션 연결 준비: {} 심볼", endpoint.exchange_name, symbols.len());
        for symbol in symbols {
            debug!("   └─ 심볼: {}", symbol);
        }
        Ok(endpoint.ws_url_base.clone())
    }

    /// Bithumb WebSocket URL 생성
    fn build_bithumb_websocket_url(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        // Bithumb은 연결 후 별도 구독 메시지로 심볼 지정
        info!("🔗 Bithumb {} 세션 연결 준비: {} 심볼", endpoint.exchange_name, symbols.len());
        for symbol in symbols {
            debug!("   └─ 심볼: {}", symbol);
        }
        Ok(endpoint.ws_url_base.clone())
    }

    /// Coinbase WebSocket URL 생성
    fn build_coinbase_websocket_url(&self, endpoint: &ExchangeEndpoint, symbols: &[String]) -> Result<String> {
        // Coinbase는 연결 후 별도 구독 메시지로 심볼 지정
        info!("🔗 Coinbase {} 세션 연결 준비: {} 심볼", endpoint.exchange_name, symbols.len());
        for symbol in symbols {
            debug!("   └─ 심볼: {}", symbol);
        }
        Ok(endpoint.ws_url_base.clone())
    }

/// 거래소별 구독 메시지 생성
fn build_subscription_message(&self, exchange_name: &str, _symbols: &[String]) -> Result<Option<Message>> {
        match exchange_name {
            "BinanceSpot" | "BinanceFutures" => {
                // Binance는 URL에서 구독을 처리하므로 별도 메시지 불필요
                Ok(None)
            },
            _ => {
                // 다른 거래소들은 나중에 구현
                Ok(None)
            }
        }
    }
}

impl ConnectionManager {
    async fn send_connection_event(&self, exchange_name: &str, previous_status: u8, current_status: u8, retry_count: u32, error_code: u64) -> Result<()> {
        let exchange_id = infer_exchange_id_from_display(exchange_name);
        let event = SystemEvent::ConnectionStatus(ConnectionStatus::new(exchange_id, previous_status, current_status, retry_count, error_code));
        // 표시용 문자열(ex: BinanceSpot/BinanceFutures)을 그대로 헤더에 기록
        let packet = self.packet_builder.build_event_packet_with_exchange(event, exchange_name)?;
        self.udp_broadcaster.send_packet(packet).await
    }
}

fn infer_exchange_id_from_display(display_name: &str) -> u16 {
    let lower = display_name.to_lowercase();
    if lower.contains("binance") { return exchange_name_to_id("binance"); }
    if lower.contains("okx") { return exchange_name_to_id("okx"); }
    if lower.contains("bybit") { return exchange_name_to_id("bybit"); }
    if lower.contains("upbit") { return exchange_name_to_id("upbit"); }
    if lower.contains("bithumb") { return exchange_name_to_id("bithumb"); }
    if lower.contains("coinbase") { return exchange_name_to_id("coinbase"); }
    0
}

// Clone trait 구현 (Arc로 래핑된 필드들을 위해)
impl Clone for ConnectionManager {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            data_parser: Arc::clone(&self.data_parser),
            packet_builder: Arc::clone(&self.packet_builder),
            udp_broadcaster: Arc::clone(&self.udp_broadcaster),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExchangeEndpoint;

    #[test]
    fn test_backoff_delay_calculation() {
        let config = Arc::new(Config::load().unwrap());
        let data_parser = Arc::new(DataParser::new());
        let packet_builder = Arc::new(PacketBuilder::new());
        let udp_broadcaster = Arc::new(UdpMulticaster::new(&config.udp).unwrap());
        
        let manager = ConnectionManager::new(config, data_parser, packet_builder, udp_broadcaster);

        // 지수 백오프 테스트
        assert_eq!(manager.calculate_backoff_delay(0), Duration::from_secs(1));
        assert_eq!(manager.calculate_backoff_delay(1), Duration::from_secs(2));
        assert_eq!(manager.calculate_backoff_delay(2), Duration::from_secs(4));
        assert_eq!(manager.calculate_backoff_delay(3), Duration::from_secs(8));
        assert_eq!(manager.calculate_backoff_delay(10), Duration::from_secs(60)); // 최대치
    }

    #[test]
    fn test_binance_combined_stream_url_spot() {
        let config = Arc::new(Config::load().unwrap());
        let data_parser = Arc::new(DataParser::new());
        let packet_builder = Arc::new(PacketBuilder::new());
        let udp_broadcaster = Arc::new(UdpMulticaster::new(&config.udp).unwrap());
        let manager = ConnectionManager::new(config, data_parser, packet_builder, udp_broadcaster);

        let endpoint = ExchangeEndpoint {
            exchange_name: "BinanceSpot".to_string(),
            ws_url_base: "wss://stream.binance.com:9443/ws/".to_string(),
            timeout_ms: 5000,
            ping_interval_ms: Some(30000),
            enabled: true,
        };
        let url = manager.build_binance_websocket_url_from_endpoint(&endpoint, &vec!["BTC^USDT".into(), "ETH^USDT".into()]).unwrap();
        assert!(url.contains("wss://stream.binance.com:9443/stream?streams="));
        assert!(url.contains("btcusdt@trade/btcusdt@depth/ethusdt@trade/ethusdt@depth"));
    }

    #[test]
    fn test_binance_combined_stream_url_futures() {
        let config = Arc::new(Config::load().unwrap());
        let data_parser = Arc::new(DataParser::new());
        let packet_builder = Arc::new(PacketBuilder::new());
        let udp_broadcaster = Arc::new(UdpMulticaster::new(&config.udp).unwrap());
        let manager = ConnectionManager::new(config, data_parser, packet_builder, udp_broadcaster);

        let endpoint = ExchangeEndpoint {
            exchange_name: "BinanceFutures".to_string(),
            ws_url_base: "wss://fstream.binance.com/ws/".to_string(),
            timeout_ms: 5000,
            ping_interval_ms: Some(30000),
            enabled: true,
        };
        let url = manager.build_binance_websocket_url_from_endpoint(&endpoint, &vec!["BTC^USDT".into(), "ETH^USDT".into()]).unwrap();
        assert!(url.contains("wss://fstream.binance.com/stream?streams="));
        assert!(url.contains("btcusdt@aggTrade/btcusdt@depth/ethusdt@aggTrade/ethusdt@depth"));
    }
}