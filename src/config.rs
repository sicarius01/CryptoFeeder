use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub exchanges: Vec<ExchangeConfig>,
    pub symbols: Vec<String>,
    pub udp: UdpConfig,
    pub logging: LoggingConfig,
    pub runtime_threads: Option<usize>,
    pub metrics: MetricsConfig,
    pub symbol_config: Option<SymbolConfig>,
    pub endpoint_config: Option<EndpointConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeConfig {
    pub name: String,
    pub ws_url: String,
    pub enabled: bool,
    pub connection_timeout_ms: u64,
    pub ping_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpConfig {
    pub multicast_addr: String,
    pub port: u16,
    pub interface_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub interval_secs: u64,
}

/// 심볼 설정 전체 구조체
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolConfig {
    pub exchanges: HashMap<String, ExchangeSymbolGroup>,
}

/// 거래소별 심볼 그룹 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSymbolGroup {
    pub exchange_name: String,
    pub sessions: Vec<SymbolSession>,
}

/// 세션별 심볼 목록 (WebSocket 연결 단위)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSession {
    pub symbols: Vec<String>,
    pub is_btc_session: bool,
}

/// 엔드포인트 설정 전체 구조체
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    pub exchanges: HashMap<String, ExchangeEndpoint>,
}

/// 거래소별 엔드포인트 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeEndpoint {
    pub exchange_name: String,
    pub ws_url_base: String,
    pub timeout_ms: u64,
    pub ping_interval_ms: Option<u64>,
    pub enabled: bool,
}

impl Config {
    pub fn load() -> Result<Self> {
        // config.ini 파싱 (한 번만)
        let ini_map = match read_config_file("config.ini").and_then(|c| Self::parse_ini(&c)) {
            Ok(map) => map,
            Err(e) => {
                log::warn!("config.ini 파일 읽기 실패({}), 기본 설정 사용", e);
                HashMap::new()
            }
        };

        // UDP 설정
        let udp_config = UdpConfig {
            multicast_addr: ini_map.get("multicast_addr").cloned().unwrap_or_else(|| "239.255.1.1".to_string()),
            port: ini_map.get("port").and_then(|p| p.parse::<u16>().ok()).unwrap_or(55555),
            interface_addr: ini_map.get("interface_addr").cloned().unwrap_or_else(|| "0.0.0.0".to_string()),
        };

        // 런타임 쓰레드 수 (0 또는 미지정이면 런타임 기본값 사용)
        let runtime_threads = ini_map.get("runtime_threads").and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0);

        // 메트릭스 설정 (기본 비활성, 5초 간격)
        let metrics = MetricsConfig {
            enabled: ini_map.get("metrics_enabled").map(|v| v.eq_ignore_ascii_case("true") || v == "1").unwrap_or(false),
            interval_secs: ini_map.get("metrics_interval_secs").and_then(|v| v.parse::<u64>().ok()).unwrap_or(5),
        };

        // 로깅 설정
        let logging = LoggingConfig {
            level: ini_map.get("log_level").cloned().unwrap_or_else(|| "info".to_string()),
            file_path: ini_map.get("log_file_path").cloned(),
        };

        // symbol_config.ini 파일 읽기 시도
        let symbol_config = SymbolConfig::load().ok();

        // endpoint.ini 파일 읽기 시도
        let endpoint_config = EndpointConfig::load().ok();

        // 기본 설정 (Checkpoint 1용)
        Ok(Config {
            exchanges: vec![
                ExchangeConfig {
                    name: "binance".to_string(),
                    ws_url: "wss://stream.binance.com:9443/ws/btcusdt@trade/btcusdt@depth".to_string(),
                    enabled: true,
                    connection_timeout_ms: 5000,
                    ping_interval_ms: Some(30000),
                },
            ],
            symbols: vec!["BTC^USDT".to_string()],
            udp: udp_config,
            logging,
            runtime_threads,
            metrics,
            symbol_config,
            endpoint_config,
        })
    }

    /// 간단한 INI 파일 파서
    fn parse_ini(content: &str) -> Result<HashMap<String, String>> {
        let mut config_map = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            
            // 빈 줄이나 주석 라인 무시
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // [section] 라인 무시 (간단한 구현을 위해)
            if line.starts_with('[') && line.ends_with(']') {
                continue;
            }

            // key=value 형태 파싱
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                config_map.insert(key, value);
            }
        }

        Ok(config_map)
    }

    pub fn get_btc_symbols(&self) -> Vec<String> {
        self.symbols.iter()
            .filter(|s| s.contains("BTC"))
            .cloned()
            .collect()
    }

    pub fn get_non_btc_symbols(&self) -> Vec<String> {
        self.symbols.iter()
            .filter(|s| !s.contains("BTC"))
            .cloned()
            .collect()
    }
}

impl SymbolConfig {
    /// symbol_config.ini 파일을 로드하여 SymbolConfig 생성
    pub fn load() -> Result<Self> {
        let content = read_config_file("symbol_config.ini")?;

        Self::parse(&content)
    }

    /// symbol_config.ini 내용을 파싱하여 SymbolConfig 생성
    fn parse(content: &str) -> Result<Self> {
        let mut exchanges = HashMap::new();
        let mut current_exchange: Option<String> = None;
        let mut current_sessions = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            
            // 빈 줄이나 주석 라인 무시
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // [거래소명] 섹션 시작
            if line.starts_with('[') && line.ends_with(']') {
                // 이전 거래소 데이터 저장
                if let Some(exchange_name) = current_exchange.take() {
                    let exchange_group = ExchangeSymbolGroup {
                        exchange_name: exchange_name.clone(),
                        sessions: current_sessions.clone(),
                    };
                    exchanges.insert(exchange_name, exchange_group);
                    current_sessions.clear();
                }

                // 새 거래소 설정
                current_exchange = Some(line[1..line.len()-1].to_string());
                continue;
            }

            // 심볼 라인 파싱
            if current_exchange.is_some() {
                let session = Self::parse_symbol_line(line)?;
                current_sessions.push(session);
            }
        }

        // 마지막 거래소 데이터 저장
        if let Some(exchange_name) = current_exchange {
            let exchange_group = ExchangeSymbolGroup {
                exchange_name: exchange_name.clone(),
                sessions: current_sessions,
            };
            exchanges.insert(exchange_name, exchange_group);
        }

        Ok(SymbolConfig { exchanges })
    }

    /// 심볼 라인을 파싱하여 SymbolSession 생성
    fn parse_symbol_line(line: &str) -> Result<SymbolSession> {
        let symbols: Vec<String> = line
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if symbols.is_empty() {
            return Err(crate::errors::CryptoFeederError::Other(
                format!("심볼 라인이 비어있습니다: {}", line)
            ));
        }

        // BTC가 포함된 세션인지 확인 (첫 번째 심볼만 체크, BTC는 독립 세션)
        let is_btc_session = symbols.len() == 1 && symbols[0].starts_with("BTC^");

        Ok(SymbolSession {
            symbols,
            is_btc_session,
        })
    }

    /// 특정 거래소의 세션 목록 반환
    pub fn get_exchange_sessions(&self, exchange_name: &str) -> Option<&Vec<SymbolSession>> {
        self.exchanges.get(exchange_name).map(|group| &group.sessions)
    }

    /// 모든 거래소 이름 반환
    pub fn get_exchange_names(&self) -> Vec<String> {
        self.exchanges.keys().cloned().collect()
    }

    /// 특정 거래소의 모든 심볼 반환
    pub fn get_all_symbols(&self, exchange_name: &str) -> Vec<String> {
        if let Some(group) = self.exchanges.get(exchange_name) {
            group.sessions
                .iter()
                .flat_map(|session| session.symbols.iter())
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 특정 거래소의 BTC 세션들만 반환
    pub fn get_btc_sessions(&self, exchange_name: &str) -> Vec<&SymbolSession> {
        if let Some(group) = self.exchanges.get(exchange_name) {
            group.sessions
                .iter()
                .filter(|session| session.is_btc_session)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 특정 거래소의 BTC가 아닌 세션들만 반환
    pub fn get_non_btc_sessions(&self, exchange_name: &str) -> Vec<&SymbolSession> {
        if let Some(group) = self.exchanges.get(exchange_name) {
            group.sessions
                .iter()
                .filter(|session| !session.is_btc_session)
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl EndpointConfig {
    /// endpoint.ini 파일을 로드하여 EndpointConfig 생성
    pub fn load() -> Result<Self> {
        let content = read_config_file("endpoint.ini")?;

        Self::parse(&content)
    }

    /// endpoint.ini 내용을 파싱하여 EndpointConfig 생성
    fn parse(content: &str) -> Result<Self> {
        let mut exchanges = HashMap::new();
        let mut current_exchange: Option<String> = None;
        let mut current_settings = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            
            // 빈 줄이나 주석 라인 무시
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // [거래소명] 섹션 시작
            if line.starts_with('[') && line.ends_with(']') {
                // 이전 거래소 데이터 저장
                if let Some(exchange_name) = current_exchange.take() {
                    if let Some(endpoint) = Self::parse_exchange_settings(&exchange_name, &current_settings) {
                        exchanges.insert(exchange_name, endpoint);
                    }
                    current_settings.clear();
                }

                // 새 거래소 설정
                current_exchange = Some(line[1..line.len()-1].to_string());
                continue;
            }

            // 설정 라인 파싱 (key=value)
            if let Some((key, value)) = line.split_once('=') {
                current_settings.insert(key.trim().to_string(), value.trim().to_string());
            }
        }

        // 마지막 거래소 데이터 저장
        if let Some(exchange_name) = current_exchange {
            if let Some(endpoint) = Self::parse_exchange_settings(&exchange_name, &current_settings) {
                exchanges.insert(exchange_name, endpoint);
            }
        }

        Ok(EndpointConfig { exchanges })
    }

    /// 거래소 설정을 파싱하여 ExchangeEndpoint 생성
    fn parse_exchange_settings(exchange_name: &str, settings: &HashMap<String, String>) -> Option<ExchangeEndpoint> {
        let ws_url_base = settings.get("ws_url_base")?.clone();
        let timeout_ms = settings.get("timeout_ms")?.parse().ok().unwrap_or(5000);
        let ping_interval_ms = settings.get("ping_interval_ms")
            .and_then(|s| s.parse().ok());
        let enabled = settings.get("enabled")
            .map(|s| s.parse().unwrap_or(false))
            .unwrap_or(false);

        Some(ExchangeEndpoint {
            exchange_name: exchange_name.to_string(),
            ws_url_base,
            timeout_ms,
            ping_interval_ms,
            enabled,
        })
    }

    /// 특정 거래소의 엔드포인트 정보 반환
    pub fn get_exchange_endpoint(&self, exchange_name: &str) -> Option<&ExchangeEndpoint> {
        self.exchanges.get(exchange_name)
    }

    /// 모든 활성화된 거래소 이름 반환
    pub fn get_enabled_exchange_names(&self) -> Vec<String> {
        self.exchanges
            .values()
            .filter(|endpoint| endpoint.enabled)
            .map(|endpoint| endpoint.exchange_name.clone())
            .collect()
    }

    /// 모든 거래소 이름 반환
    pub fn get_all_exchange_names(&self) -> Vec<String> {
        self.exchanges.keys().cloned().collect()
    }
}

/// 구성 파일을 여러 후보 경로에서 탐색하여 읽는다.
/// 탐색 순서:
/// 1) 현재 작업 디렉터리 기준: ./config/<name>
/// 2) 실행 파일 디렉터리 기준: <exe_dir>/config/<name>
/// 3) 개발 빌드 위치 보정: <exe_dir>/../../config/<name> (target/{debug,release}에서 프로젝트 루트로 상향)
/// 4) 환경 변수 CRYPTOFEEDER_CONFIG_DIR 지정 시: <env>/name (최우선)
fn read_config_file(name: &str) -> Result<String> {
    // 0) 환경 변수 우선
    if let Ok(custom_dir) = env::var("CRYPTOFEEDER_CONFIG_DIR") {
        let p = Path::new(&custom_dir).join(name);
        if p.exists() {
            return fs::read_to_string(&p).map_err(|e| crate::errors::CryptoFeederError::Other(
                format!("{} 읽기 실패 ({}): {}", name, p.display(), e)
            ));
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1) CWD/config/name
    candidates.push(Path::new("config").join(name));

    // 2) exe_dir/config/name
    if let Ok(exe) = env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("config").join(name));
            // 3) exe_dir/../../config/name (target/release → 프로젝트 루트/config)
            if let Some(parent1) = exe_dir.parent() {
                if let Some(parent2) = parent1.parent() {
                    candidates.push(parent2.join("config").join(name));
                }
            }
        }
    }

    for p in candidates {
        if p.exists() {
            return fs::read_to_string(&p).map_err(|e| crate::errors::CryptoFeederError::Other(
                format!("{} 읽기 실패 ({}): {}", name, p.display(), e)
            ));
        }
    }

    Err(crate::errors::CryptoFeederError::Other(format!(
        "{} 파일을 찾을 수 없습니다. CWD/config, exe_dir/config, exe_dir/../../config, CRYPTOFEEDER_CONFIG_DIR 를 확인하세요.",
        name
    )))
}