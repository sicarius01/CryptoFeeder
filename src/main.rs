use anyhow::Result;
use log::{info, error, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::runtime::Builder as TokioRuntimeBuilder;

mod config;
mod connection_manager;
mod data_parser;
mod packet_builder;
mod udp_broadcaster;
mod protocol;
mod errors;
mod events;

use config::Config;
use connection_manager::ConnectionManager;
use data_parser::DataParser;
use packet_builder::PacketBuilder;
use udp_broadcaster::UdpMulticaster;

fn main() -> Result<()> {
    // 설정 로드 (런타임 쓰레드 수를 적용하기 위함)
    let config = Arc::new(Config::load()?);

    // 로깅 초기화 - config에서 수준/파일 설정 적용
    let mut logger = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.logging.level));
    logger.filter_module("tokio_tungstenite", log::LevelFilter::Info)
          .filter_module("tungstenite", log::LevelFilter::Info);
    if let Some(ref path) = config.logging.file_path {
        let _ = std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new(".")));
        if let Ok(file) = std::fs::File::create(path) {
            logger.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    logger.init();

    // Tokio 런타임 구성 (runtime_threads 적용, 0 또는 None이면 기본값)
    let rt = if let Some(t) = config.runtime_threads {
        if t > 0 {
            info!("🧵 Tokio 멀티스레드 런타임 생성: worker_threads={}", t);
            TokioRuntimeBuilder::new_multi_thread().worker_threads(t).enable_all().build()
                .map_err(|e| anyhow::anyhow!("Tokio 런타임 생성 실패: {}", e))?
        } else {
            info!("🧵 Tokio 멀티스레드 런타임 생성: 기본 쓰레드 수");
            TokioRuntimeBuilder::new_multi_thread().enable_all().build()
                .map_err(|e| anyhow::anyhow!("Tokio 런타임 생성 실패: {}", e))?
        }
    } else {
        info!("🧵 Tokio 멀티스레드 런타임 생성: 기본 쓰레드 수");
        TokioRuntimeBuilder::new_multi_thread().enable_all().build()
            .map_err(|e| anyhow::anyhow!("Tokio 런타임 생성 실패: {}", e))?
    };

    rt.block_on(async_main(config))
}

async fn async_main(config: Arc<Config>) -> Result<()> {
    // 명령행 인수에서 타이머 설정 읽기
    let timeout_seconds = parse_timeout_arg();

    info!("🚀 CryptoFeeder 시작 중...");
    if let Some(timeout) = timeout_seconds {
        info!("⏰ {}초 후 자동 종료 예정", timeout);
    } else {
        info!("🔄 무한 실행 모드 (Ctrl+C로 종료)");
    }

    info!("📋 설정 로드 완료");
    
    // 심볼 설정 상태 확인 및 로깅
    if let Some(symbol_config) = &config.symbol_config {
        let exchange_count = symbol_config.get_exchange_names().len();
        info!("💎 심볼 설정 파일 발견: {}개 거래소 설정됨", exchange_count);
        
        for exchange_name in symbol_config.get_exchange_names() {
            if let Some(sessions) = symbol_config.get_exchange_sessions(&exchange_name) {
                let session_count = sessions.len();
                let btc_sessions = sessions.iter().filter(|s| s.is_btc_session).count();
                let regular_sessions = session_count - btc_sessions;
                info!("   └─ {}: {}개 세션 (BTC: {}, 일반: {})", 
                      exchange_name, session_count, btc_sessions, regular_sessions);
            }
        }
    } else {
        info!("⚠️ symbol_config.ini 파일이 없음. 기본 거래소 설정 사용");
    }

    // 엔드포인트 설정 상태 확인 및 로깅
    if let Some(endpoint_config) = &config.endpoint_config {
        let total_exchanges = endpoint_config.get_all_exchange_names().len();
        let enabled_exchanges = endpoint_config.get_enabled_exchange_names();
        info!("🔗 엔드포인트 설정 파일 발견: {}개 거래소 중 {}개 활성화됨", total_exchanges, enabled_exchanges.len());
        
        for exchange_name in &enabled_exchanges {
            if let Some(endpoint) = endpoint_config.get_exchange_endpoint(exchange_name) {
                info!("   └─ {}: {} (timeout: {}ms)", 
                      exchange_name, endpoint.ws_url_base, endpoint.timeout_ms);
            }
        }
        
        if enabled_exchanges.is_empty() {
            warn!("⚠️ 활성화된 거래소가 없습니다!");
        }
    } else {
        info!("⚠️ endpoint.ini 파일이 없음. 기본 엔드포인트 사용");
    }

    // 컴포넌트 초기화
    let udp_broadcaster = Arc::new(UdpMulticaster::new(&config.udp)?);
    let packet_builder = Arc::new(PacketBuilder::new());
    let data_parser = Arc::new(DataParser::new_with_config(Some(&config)));
    let connection_manager = ConnectionManager::new(
        config.clone(),
        data_parser.clone(),
        packet_builder.clone(),
        udp_broadcaster.clone(),
    );

    info!("🔧 모든 컴포넌트 초기화 완료");

    // (중복 제거) 메트릭스 태스크는 아래 블록 하나만 유지

    // 메트릭스 태스크 (UDP 전송량/pps/CPU)
    if config.metrics.enabled {
        let broadcaster_for_metrics = udp_broadcaster.clone();
        let interval_secs = config.metrics.interval_secs.max(1);
        tokio::spawn(async move {
            use tokio::time::{sleep, Duration};
            // sysinfo 기반 CPU 측정
            use sysinfo::{System, ProcessRefreshKind, Pid};
            let pid = std::process::id();
            let mut sys = System::new();
            let mut prev_packets = 0u64;
            let mut prev_bytes = 0u64;
            loop {
                sleep(Duration::from_secs(interval_secs)).await;
                sys.refresh_processes_specifics(ProcessRefreshKind::everything());
                let cpu_percent = sys
                    .process(Pid::from_u32(pid))
                    .map(|p| p.cpu_usage())
                    .unwrap_or(0.0);
                let stats = broadcaster_for_metrics.get_stats();
                let d_packets = stats.packets_sent.saturating_sub(prev_packets);
                let d_bytes = stats.bytes_sent.saturating_sub(prev_bytes);
                prev_packets = stats.packets_sent;
                prev_bytes = stats.bytes_sent;
                let pps = d_packets as f64 / interval_secs as f64;
                let kbps = (d_bytes as f64 / interval_secs as f64) / 1024.0;
                info!(
                    "📈 metrics: cpu={:.1}% pps={:.1} kbps={:.1} avg_packet={:.1}B ({}s)",
                    cpu_percent, pps, kbps, stats.average_packet_size(), interval_secs
                );
            }
        });
    }

    // 연결 시작
    let connection_handle = tokio::spawn(async move {
        if let Err(e) = connection_manager.run().await {
            error!("연결 관리자 치명적 오류: {}", e);
            std::process::exit(1); // 연결 실패 시 프로그램 종료
        }
    });

    info!("🌐 WebSocket 연결 시작됨");

    // 종료 신호 대기 또는 연결 관리자 종료 대기 (선택적 타이머)
    if let Some(timeout) = timeout_seconds {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("🛑 종료 신호 수신, 프로그램을 정리하는 중...");
            }
            _ = tokio::time::sleep(Duration::from_secs(timeout)) => {
                info!("⏰ {}초 타이머 만료, 프로그램 자동 종료", timeout);
            }
            result = connection_handle => {
                match result {
                    Ok(_) => info!("🔚 연결 관리자가 정상 종료됨"),
                    Err(e) => error!("❌ 연결 관리자 오류: {}", e),
                }
            }
        }
    } else {
        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("🛑 종료 신호 수신, 프로그램을 정리하는 중...");
            }
            result = connection_handle => {
                match result {
                    Ok(_) => info!("🔚 연결 관리자가 정상 종료됨"),
                    Err(e) => error!("❌ 연결 관리자 오류: {}", e),
                }
            }
        }
    }
    
    info!("✅ CryptoFeeder 종료 완료");
    Ok(())
}

/// 명령행 인수에서 타이머 설정을 파싱합니다.
/// 사용법: crypto-feeder [timeout_seconds]
/// 예시: crypto-feeder 60  (60초 후 종료)
///       crypto-feeder     (무한 실행)
fn parse_timeout_arg() -> Option<u64> {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() >= 2 {
        match args[1].parse::<u64>() {
            Ok(seconds) if seconds > 0 => {
                Some(seconds)
            },
            Ok(_) => {
                eprintln!("⚠️ 타이머는 0보다 큰 값이어야 합니다. 무한 실행 모드로 시작합니다.");
                None
            },
            Err(_) => {
                eprintln!("⚠️ 잘못된 타이머 값: '{}'. 무한 실행 모드로 시작합니다.", args[1]);
                None
            }
        }
    } else {
        None
    }
}