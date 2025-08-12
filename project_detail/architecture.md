# 시스템 아키텍처 (System Architecture)

본 데이터 피더는 4개의 핵심 파이프라인 단계로 구성됩니다.

![Image of a 4-stage data processing pipeline]

1.  **WebSocket 수집 (Ingestion)**: 다수의 거래소에 대해 비동기(Asynchronous) WebSocket 연결을 생성하고 관리합니다.
2.  **SIMD 파싱 (Parsing)**: 수신된 원시 JSON 텍스트를 `simd-json`과 같은 라이브러리를 사용해 CPU 레벨에서 가속하여 Rust 구조체로 변환합니다.
3.  **프로토콜 변환 (Serialization)**: 파싱된 데이터를 `udp_packet.md` 및 `event_packet.md` 명세에 따라 표준 바이너리 포맷으로 직렬화합니다. 데이터 정렬 규칙에 따라 체결틱과 오더북 데이터를 정렬하여 패킷을 생성합니다.
4.  **UDP 전송 (Broadcast)**: 직렬화된 바이너리 패킷을 내부 네트워크에 UDP 멀티캐스트로 전송합니다. `config/config.ini` 파일에서 설정된 멀티캐스트 주소와 포트를 사용합니다.

---

## 연결 및 구독 전략 (Connection & Subscription Strategy)

* **대상 거래소**: Binance (Spot, Futures), OKX, Bybit, Upbit, Bithumb, Coinbase.
* **대상 심볼**: 시가총액 기준 상위 20개.
* **설정 파일 관리**: 
  - `config/symbol_config.ini`: 거래소별 심볼 그룹을 동적으로 설정
  - `config/endpoint.ini`: 거래소별 WebSocket 엔드포인트 및 연결 설정 관리
  - `config/config.ini`: UDP 멀티캐스트 네트워크 설정
  - 각 거래소별로 여러 세션으로 심볼을 분할하여 관리
  - 설정 파일이 없을 경우 기본 설정으로 폴백
* **데이터 표준화**: 
  - 심볼은 `A^B` 형식으로 통일 (A=거래코인, B=통화화폐)
  - 거래소는 시장별로 구분 (`BinanceSpot`, `BinanceFutures` 등)
* **세션 관리 원칙**:
    * **BTC 전용 세션**: `BTC` 관련 심볼(예: BTC/USDT, BTC/KRW)은 거래소별로 다른 모든 심볼과 분리된 **독립 WebSocket 세션**을 사용합니다. 이는 가장 중요한 자산의 데이터 수신을 보장하기 위함입니다.
    * **일반 세션**: 나머지 19개 심볼은 거래소별로 **세션당 5개**씩 묶어 구독을 요청합니다. 이는 단일 연결에 과도한 메시지가 몰리는 것을 방지하고, 특정 심볼 그룹의 문제 발생 시 영향을 최소화하기 위함입니다.
    * **연결 풀링**: 각 거래소에 대한 연결은 효율적으로 관리되어야 하며, 각 연결의 상태는 지속적으로 모니터링됩니다.

---

## 설정 파일 구조 (Configuration Files)

시스템은 `config/` 폴더 내의 여러 설정 파일을 통해 동적으로 구성됩니다:

### 1. 심볼 설정 (`config/symbol_config.ini`)
```ini
[BinanceSpot]
BTC^USDT
ETH^USDT, ADA^USDT, SOL^USDT, DOT^USDT, MATIC^USDT
DOGE^USDT, LINK^USDT, UNI^USDT, AVAX^USDT, ATOM^USDT

[BinanceFutures]
BTC^USDT
ETH^USDT, ADA^USDT, SOL^USDT, DOT^USDT, MATIC^USDT
```

### 2. 엔드포인트 설정 (`config/endpoint.ini`)
```ini
[BinanceSpot]
ws_url_base=wss://stream.binance.com:9443/ws/
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[BinanceFutures]
ws_url_base=wss://fstream.binance.com/ws/
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[OkxSpot]
ws_url_base=wss://ws.okx.com:8443/ws/v5/public
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[BybitSpot]
ws_url_base=wss://stream.bybit.com/v5/public/spot
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[BybitLinear]
ws_url_base=wss://stream.bybit.com/v5/public/linear
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[UpbitSpot]
ws_url_base=wss://api.upbit.com/websocket/v1
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[BithumbSpot]
ws_url_base=wss://pubwss.bithumb.com/pub/ws
timeout_ms=5000
ping_interval_ms=30000
enabled=true

[CoinbaseSpot]
ws_url_base=wss://ws-feed.exchange.coinbase.com
timeout_ms=5000
ping_interval_ms=30000
enabled=true
```

### 3. 네트워크 설정 (`config/config.ini`)
```ini
[UDP]
multicast_addr=239.255.1.1
port=55555
interface_addr=0.0.0.0
```

### 설정 규칙
- **BTC 심볼**: 각 라인에 하나씩 독립 세션으로 관리
- **일반 심볼**: 쉼표로 구분하여 세션당 최대 5개까지 묶음
- **거래소별 구분**: `[거래소명]` 섹션으로 구분
- **심볼 표기**: `A^B` 형식 사용 (예: `BTC^USDT`, `ETH^KRW`)
- **엔드포인트 활성화**: `enabled=true/false`로 거래소별 연결 제어
 - **표시명 보존**: 헤더 `exchange`에는 표준 표시명을 그대로 사용합니다. 예: `BinanceSpot`, `BinanceFutures` (파서/직렬화 전 과정에서 임의 치환 금지)

### 로드 우선순위
1. `config/` 폴더 내 설정 파일들을 우선 로드
2. 파일이 없거나 오류 시 기본 설정으로 폴백
3. 시작 시 로드된 설정에 대한 상세 로그 출력