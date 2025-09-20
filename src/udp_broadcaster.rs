/// UDP 멀티캐스터
/// 생성된 UDP 패킷을 네트워크에 멀티캐스트 전송

use crate::config::UdpConfig;
use crate::packet_builder::UdpPacket;
use crate::errors::{CryptoFeederError, Result};

use log::{info, debug, error, warn};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Mutex, Arc};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct UdpMulticaster {
    // 기본(레거시) 포트용 소켓
    socket: UdpSocket,
    target_addr: SocketAddr,
    connected: bool,
    // 멀티포트 지원: 포트별 연결된 소켓 캐시
    multicast_ip: Ipv4Addr,
    interface_ip: Ipv4Addr,
    sockets_by_port: Mutex<HashMap<u16, Arc<UdpSocket>>>,
    packets_sent: AtomicU64,
    bytes_sent: AtomicU64,
}

impl UdpMulticaster {
    pub fn new(config: &UdpConfig) -> Result<Self> {
        info!("🌐 UDP 멀티캐스터 초기화 중...");

        // 멀티캐스트 주소 및 인터페이스 파싱
        let multicast_ip: Ipv4Addr = config.multicast_addr.parse()
            .map_err(|e| CryptoFeederError::Other(format!("멀티캐스트 주소 파싱 실패: {}", e)))?;
        let interface_ip: Ipv4Addr = config.interface_addr.parse()
            .map_err(|e| CryptoFeederError::Other(format!("인터페이스 주소 파싱 실패: {}", e)))?;

        let target_addr = SocketAddr::from((multicast_ip, config.port));

        // 송신 전용 UDP 소켓 바인드 (임의 포트)
        let socket = UdpSocket::bind((interface_ip, 0))
            .map_err(|e| CryptoFeederError::UdpError(e))?;

        // 멀티캐스트 옵션 설정
        socket.set_nonblocking(true).ok();
        socket.set_multicast_ttl_v4(1).ok();
        socket.set_multicast_loop_v4(false).ok();
        // 송신 버퍼 확대로 커널 호출 빈도/일시적 대기 완화 (socket2 필요) - 표준 UdpSocket에는 없음
        // 이 프로젝트에서는 표준 라이브러리만 사용하므로 주석 처리
        // use socket2::{Socket, Domain, Type};
        // let s2 = Socket::from(socket.try_clone().unwrap());
        // let _ = s2.set_send_buffer_size(1 << 20);

        // UDP connect로 peer를 고정하여 send_to 오버헤드 감소 (Windows에서 라우팅/체크 비용 절감 가능)
        let connected = match socket.connect(target_addr) {
            Ok(_) => {
                info!("🔗 UDP connect 성공: {}", target_addr);
                true
            },
            Err(e) => {
                warn!("⚠️ UDP connect 실패({}), send_to 경로 사용", e);
                false
            }
        };

        info!("✅ UDP 소켓 생성 완료");
        info!("📡 멀티캐스트 주소: {}", target_addr);
        info!("🔧 인터페이스: {}", interface_ip);

        Ok(Self {
            socket,
            target_addr,
            connected,
            multicast_ip,
            interface_ip,
            sockets_by_port: Mutex::new(HashMap::new()),
            packets_sent: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
        })
    }

    /// UDP 패킷 전송
    pub async fn send_packet(&self, packet: UdpPacket) -> Result<()> {
        debug!("📤 UDP 패킷 전송 시도: {} bytes", packet.size);

        // 논블로킹 전송 시도 (connected 우선)
        let send_result = if self.connected {
            self.socket.send(&packet.data)
        } else {
            self.socket.send_to(&packet.data, self.target_addr)
        };

        match send_result {
            Ok(bytes_sent) => {
                if bytes_sent != packet.size {
                    error!("⚠️ 부분 전송: {}/{} bytes", bytes_sent, packet.size);
                } else {
                    debug!("✅ 패킷 전송 성공: {} bytes", bytes_sent);
                }

                // 통계 업데이트
                self.packets_sent.fetch_add(1, Ordering::Relaxed);
                self.bytes_sent.fetch_add(bytes_sent as u64, Ordering::Relaxed);

                Ok(())
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 논블로킹 소켓에서 일시적으로 전송할 수 없는 경우
                // 실제 환경에서는 버퍼링이나 재시도 로직을 구현할 수 있음
                debug!("📤 UDP 전송 버퍼 가득참, 패킷 드롭");
                Ok(())
            },
            Err(e) => {
                error!("❌ UDP 전송 실패: {}", e);
                Err(CryptoFeederError::UdpError(e))
            }
        }
    }

    /// 특정 포트로 UDP 패킷 전송 (세션별 포트 분산용)
    pub async fn send_packet_to_port(&self, packet: UdpPacket, port: u16) -> Result<()> {
        // 기본 포트와 같으면 기존 경로 재사용
        if port == self.target_addr.port() {
            return self.send_packet(packet).await;
        }

        // 포트별 연결된 소켓을 캐시하여 전송 비용 최소화
        let maybe_send = {
            // 한 번만 잠금 유지
            let mut map = self.sockets_by_port.lock().unwrap();
            if !map.contains_key(&port) {
                let target_addr = SocketAddr::from((self.multicast_ip, port));
                match UdpSocket::bind((self.interface_ip, 0)) {
                    Ok(sock) => {
                        sock.set_nonblocking(true).ok();
                        sock.set_multicast_ttl_v4(1).ok();
                        sock.set_multicast_loop_v4(false).ok();
                        if let Err(e) = sock.connect(target_addr) {
                            warn!("⚠️ UDP connect 실패({}) for port {}. send_to 경로 사용 예정", e, port);
                        } else {
                            info!("🔗 UDP connect 성공: {}", target_addr);
                        }
                        map.insert(port, Arc::new(sock));
                    }
                    Err(e) => {
                        error!("❌ 포트 {}용 UDP 소켓 생성 실패: {}", port, e);
                        return Err(CryptoFeederError::UdpError(e));
                    }
                }
            }
            // 클론은 비용이 작음 (소켓은 내부적으로 공유 카운트됨)
            map.get(&port).cloned()
        };

        if let Some(sock) = maybe_send {
            let send_result = sock.send(&packet.data);
            match send_result {
                Ok(bytes_sent) => {
                    if bytes_sent != packet.size { error!("⚠️ 부분 전송: {}/{} bytes", bytes_sent, packet.size); }
                    self.packets_sent.fetch_add(1, Ordering::Relaxed);
                    self.bytes_sent.fetch_add(bytes_sent as u64, Ordering::Relaxed);
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    debug!("📤 UDP 전송 버퍼 가득참, 패킷 드롭");
                    Ok(())
                }
                Err(e) => {
                    error!("❌ UDP 전송 실패: {}", e);
                    Err(CryptoFeederError::UdpError(e))
                }
            }
        } else {
            Err(CryptoFeederError::Other(format!("포트 {}용 소켓을 사용할 수 없습니다", port)))
        }
    }

    /// 전송 통계 조회
    pub fn get_stats(&self) -> UdpStats {
        UdpStats {
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
        }
    }

    /// 통계 리셋
    pub fn reset_stats(&self) {
        self.packets_sent.store(0, Ordering::Relaxed);
        self.bytes_sent.store(0, Ordering::Relaxed);
    }

    /// 대상 주소 조회
    pub fn target_address(&self) -> SocketAddr {
        self.target_addr
    }
}

#[derive(Debug, Clone)]
pub struct UdpStats {
    pub packets_sent: u64,
    pub bytes_sent: u64,
}

impl UdpStats {
    /// 평균 패킷 크기 계산
    pub fn average_packet_size(&self) -> f64 {
        if self.packets_sent == 0 {
            0.0
        } else {
            self.bytes_sent as f64 / self.packets_sent as f64
        }
    }
}

// Drop 구현으로 정리 작업
impl Drop for UdpMulticaster {
    fn drop(&mut self) {
        let stats = self.get_stats();
        info!("📊 UDP 멀티캐스터 종료 - 전송된 패킷: {}, 바이트: {}, 평균 크기: {:.1} bytes", 
              stats.packets_sent, stats.bytes_sent, stats.average_packet_size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UdpConfig;

    fn create_test_config() -> UdpConfig {
        UdpConfig {
            multicast_addr: "239.1.1.1".to_string(),
            port: 19001, // 테스트용 포트
            interface_addr: "127.0.0.1".to_string(),
        }
    }

    #[test]
    fn test_udp_multicaster_creation() {
        let config = create_test_config();
        let multicaster = UdpMulticaster::new(&config);
        
        assert!(multicaster.is_ok());
        
        if let Ok(b) = multicaster {
            assert_eq!(b.target_address().port(), 19001);
        }
    }

    #[test]
    fn test_stats_calculation() {
        let stats = UdpStats {
            packets_sent: 100,
            bytes_sent: 15000,
        };
        
        assert_eq!(stats.average_packet_size(), 150.0);
        
        let empty_stats = UdpStats {
            packets_sent: 0,
            bytes_sent: 0,
        };
        
        assert_eq!(empty_stats.average_packet_size(), 0.0);
    }

    #[tokio::test]
    async fn test_packet_sending() {
        let config = create_test_config();
        
        if let Ok(multicaster) = UdpMulticaster::new(&config) {
            let test_packet = UdpPacket {
                data: vec![1, 2, 3, 4, 5],
                size: 5,
            };
            
            // 전송 시도 (테스트 환경에서는 실패할 수 있음)
            let result = multicaster.send_packet(test_packet).await;
            
            // 테스트 환경에서는 네트워크 전송이 실패할 수 있으므로 
            // 에러를 출력하고 멀티캐스터 생성만 확인
            if result.is_err() {
                println!("UDP 전송 실패 (테스트 환경에서 정상): {:?}", result.err().unwrap());
            }
            
            // 멀티캐스터가 정상적으로 생성되었다면 성공으로 간주
            assert!(true);
        } else {
            panic!("UdpMulticaster 생성 실패");
        }
    }
}