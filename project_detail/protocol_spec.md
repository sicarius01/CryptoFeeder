# 프로토콜 명세 지침 (Protocol Specification Guidelines)

## 기본 시장 데이터 프로토콜

* 모든 시장 데이터(오더북, 체결)의 패킷 구조는 **`../udp_packet_detail/udp_packet.md`** 파일에 정의되어 있습니다.
* 이 파일을 **반드시 숙지하고 그대로 구현**해야 합니다. `PacketHeader`, `OrderBookItem`, `TradeTickItem` 구조체의 필드, 크기, 바이트 순서(리틀 엔디안)를 정확히 준수하십시오.

### 헤더의 `exchange` 표기 원칙 (중요)
- 헤더 필드 `exchange`에는 항상 표준화된 "표시용" 이름을 그대로 기록합니다.
  - 예시: `BinanceSpot`, `BinanceFutures`, `BybitSpot`, `BybitLinear`, `OkxSwap` 등
- 파서 단계나 후속 단계에서 `exchange` 문자열을 임의로 소문자 기본명(예: `binance`)으로 덮어쓰지 않습니다.
- 이벤트 패킷(100번대 message_type)에서도 동일 규칙을 적용합니다. 세션 기준 표시명(예: `BinanceFutures`)을 그대로 헤더에 기록해 현물/선물 등 시장을 명확히 구분합니다.

## 데이터 표준화 규칙

* **심볼 표준화**: 모든 거래소의 심볼은 `A^B` 형식으로 통일됩니다 (A=거래코인, B=통화화폐).
  - 예시: `BTC^USDT`, `ETH^KRW`, `ADA^BTC`
* **거래소명 표준화**: 거래소는 시장 유형별로 구분됩니다.
  - 현물: `BinanceSpot`, `UpbitSpot`, `BybitSpot` 등
  - 선물: `BinanceFutures`, `BybitLinear`, `OkxSwap` 등
  - 주의: 헤더의 `exchange`에는 반드시 위 표준 표시명을 사용합니다(임의 치환/축약 금지).
* **데이터 정렬 규칙**: 패킷 생성 시 다음 정렬 규칙을 적용합니다:
  - 체결틱: Ask(매수자 테이커) 오름차순, Bid 내림차순
  - 오더북: Ask 오름차순, Bid 내림차순
  - 같은 가격일 때는 메시지 순서 보장
* **설정 관리**: 
  - `config/config.ini`: UDP 멀티캐스트 IP/인터페이스 설정
  - `config/symbol_config.ini`: 거래소별 심볼 그룹 및 세션별 전송 포트 설정 (예: `55557=DOGE^USDT, XRP^USDT, SOL^USDT`)
  - `config/endpoint.ini`: 거래소별 WebSocket 엔드포인트 및 연결 설정
* 상세한 표준화 규칙은 `../udp_packet_detail/udp_packet.md`의 5장을 참조하십시오.

## 이벤트 프로토콜 확장 요구사항

현재 프로토콜은 시스템의 상태를 알릴 방법이 없습니다. 따라서 당신의 핵심 과업 중 하나는 **시스템 이벤트를 위한 프로토콜을 확장 설계하고 문서화**하는 것입니다.

1.  **새로운 `message_type` 정의**:
    * `udp_packet.md`의 `message_type`에서 사용되지 않는 값(예: `100`번대)을 할당하여 아래 이벤트들을 구분하십시오.
        * `100`: `SystemHeartbeat` (피더 생존 신호)
        * `101`: `ConnectionStatus` (연결 상태 변경)
        * `102`: `SubscriptionStatus` (구독 성공/실패)
        * 기타 필요하다고 판단되는 이벤트 타입 추가 가능

2.  **이벤트 페이로드 설계**:
    * 각 이벤트에 맞는 페이로드(Payload) 구조체를 설계하십시오.
    * 예를 들어, `ConnectionStatus`는 `(거래소 ID: u16, 이전 상태: u8, 현재 상태: u8)` 와 같은 필드를 가질 수 있습니다. 상태는 `(Disconnected, Connecting, Connected)` 등으로 정의할 수 있습니다.
    * 기존 설계 원칙에 따라 필드는 고정 크기를 가져야 합니다.

3.  **산출물**:
### 이벤트 패킷의 `exchange` 표기 정책
- 이벤트 패킷 생성 시 헤더 `exchange`에는 해당 연결(세션)의 표시명을 그대로 기입합니다.
- 예: `BinanceFutures` 연결의 재시도/성공/실패 이벤트는 헤더 `exchange=BinanceFutures`로 송출합니다.
    * 위 요구사항에 따라 설계한 새로운 이벤트 패킷 명세를 **`./event_packet.md`** 라는 이름의 새 파일로 생성하여 제출하십시오.
    * 이 파일은 기존 `udp_packet.md`와 같이 상세한 테이블 형식으로 필드, 크기, 타입, 설명을 포함해야 합니다.