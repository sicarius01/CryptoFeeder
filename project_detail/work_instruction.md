# 상세 작업 지시서 (Detailed Work Instructions)

## Module 1: WebSocket 연결 관리자 (`connection_manager`)

* **기능**: 대상 거래소에 대한 WebSocket 연결 생성, 유지, 모니터링 및 재연결을 담당합니다.
* **요구사항**:
    1.  `architecture.md`에 명시된 연결 및 구독 전략을 정확히 구현해야 합니다.
    1.1. **설정 파일 지원**: 
        * `config/symbol_config.ini`: 거래소별 세션과 심볼 그룹 설정
        * `config/endpoint.ini`: 거래소별 WebSocket 엔드포인트 및 연결 설정
        * 파일이 있으면 해당 설정을 사용하고, 없으면 기본 설정을 사용합니다.
    2.  `tokio-tungstenite` 또는 유사한 비동기 WebSocket 라이브러리를 사용하십시오.
    3.  연결 단절 감지 시, 즉시 재연결 절차를 시작해야 합니다.
    4.  **재연결 로직**:
        * 최초 시도: 1초 후.
        * 이후 시도: 이전 시도 간격의 2배 (2초, 4초, 8초...)로 **지수 백오프(Exponential Backoff)**를 적용합니다.
        * 최대 간격: 60초를 넘지 않도록 설정합니다.
    5.  모든 연결 상태의 변화(최초 연결 성공, 단절, 재연결 시도, 재연결 성공/실패)는 로그로 기록하고, **`protocol_spec.md`**에 따라 정의될 이벤트 패킷으로 생성하여 다음 단계로 전달해야 합니다.

## Module 2: 데이터 파서 (`data_parser`)

* **기능**: WebSocket으로 수신된 JSON 문자열을 Rust 구조체로 변환합니다.
* **요구사항**:
    1.  **반드시 `simd-json` 라이브러리를 사용**하여 파싱 성능을 극대화해야 합니다.
    2.  각 거래소의 오더북, 체결 데이터 포맷에 맞는 개별 파싱 로직을 구현해야 합니다.
    3.  파싱된 결과물은 표준화된 내부 데이터 구조체(예: `StandardizedTrade`, `StandardizedOrderBookUpdate`)로 변환되어야 합니다.
    4.  **데이터 표준화**: 모든 파싱 과정에서 다음 표준을 적용해야 합니다:
        * 심볼명: `A^B` 형식으로 통일 (A=거래코인, B=통화화폐)
        * 거래소명: 시장별 구분 (`BinanceSpot`, `BinanceFutures`, `BybitLinear` 등)
        * 헤더의 `exchange` 표기 원칙: 파서/직렬화 전 과정에서 표준 표시명(예: `BinanceSpot`, `BinanceFutures`)을 유지하며, 소문자 기본명 등으로 임의 덮어쓰지 않는다.
        * 상세 규칙은 `../exchange_protocol/standardization_guide.md` 참조

## Module 3: UDP 패킷 생성기 (`packet_builder`)

* **기능**: 표준화된 내부 데이터 구조체를 최종 UDP 바이너리 패킷으로 직렬화합니다.
* **요구사항**:
    1.  시장 데이터(체결, 오더북)는 **`udp_packet_detail/udp_packet.md`** 명세를 참조하여 직렬화합니다.
    2.  시스템 이벤트(연결 상태 등)는 **`protocol_spec.md`**의 요구사항에 따라 **당신이 직접 설계하고 문서화할** `event_packet.md` 명세에 따라 직렬화합니다.
        - 이벤트 패킷 헤더의 `exchange`에는 연결된 세션의 표시명을 그대로 기록합니다(예: `BinanceFutures`).
    3.  모든 멀티바이트 필드는 **리틀 엔디안(Little Endian)**으로 처리합니다.
    4.  `PacketHeader`의 `sequence_number`는 모든 종류의 패킷을 통틀어 중복 없이 1씩 증가하는 **원자적 카운터(Atomic Counter)**로 구현해야 합니다.
    5.  `exchange_timestamp`와 `local_timestamp`를 나노초 단위로 정확히 기록해야 합니다.

## Module 4: UDP 브로드캐스터 (`udp_broadcaster`)

* **기능**: 생성된 UDP 패킷을 네트워크에 전송합니다.
* **요구사항**:
    1.  Rust의 표준 라이브러리 `std::net::UdpSocket`을 사용합니다.
    2.  설정 파일(Configuration)에서 IP/인터페이스(`config/config.ini`), 세션별 포트(`config/symbol_config.ini`)를 읽어와 **UDP 멀티캐스트**를 수행하도록 구현합니다.

## Module 5: 설정 파일 관리 (`config_management`)

* **기능**: `config/` 폴더 내 설정 파일들을 통해 시스템을 동적으로 구성합니다.

### 5.1 심볼 설정 관리 (`config/symbol_config.ini`)
* **요구사항**:
    1.  **설정 파일 구조**: INI 형식으로 거래소별 섹션을 구분하고, 각 라인은 하나의 WebSocket 세션을 나타냅니다.
    2.  **BTC 세션 규칙**: BTC 관련 심볼은 반드시 독립 세션으로 관리 (한 라인에 하나의 BTC 심볼만)
    3.  **일반 세션 규칙**: 나머지 심볼은 세션당 최대 5개까지 쉼표로 구분하여 그룹화
    4.  **파싱 로직**: 각 라인을 파싱하여 `SymbolSession` 구조체로 변환하고, BTC 세션 여부를 자동 감지

### 5.2 엔드포인트 설정 관리 (`config/endpoint.ini`)
* **요구사항**:
    1.  **거래소별 엔드포인트**: 각 거래소의 WebSocket URL, 타임아웃, 활성화 상태 관리
    2.  **연결 설정**: timeout_ms, ping_interval_ms 등 연결 파라미터 설정
    3.  **활성화 제어**: enabled 플래그로 거래소별 연결 on/off 제어
    4.  **동적 로딩**: `EndpointConfig` 구조체로 파싱하여 런타임에서 사용

### 5.3 네트워크 설정 관리 (`config/config.ini`)
* **요구사항**:
    1.  **UDP 멀티캐스트**: multicast_addr, interface_addr 설정 (포트는 `symbol_config.ini` 세션 라인에서 지정)
    2.  **기본값 지원**: 설정 파일이 없어도 기본값으로 동작

### 설정 파일 예시

#### Symbol Config
```ini
[BinanceSpot]
55559=BTC^USDT
55558=ETH^USDT, ADA^USDT, SOL^USDT, DOT^USDT, MATIC^USDT
```

#### Endpoint Config
```ini
[BinanceSpot]
ws_url_base=wss://stream.binance.com:9443/ws/
timeout_ms=5000
ping_interval_ms=30000
enabled=true
```

#### Network Config
```ini
[UDP]
multicast_addr=239.255.1.1
interface_addr=0.0.0.0
```

### 연동 방식
- `Config::load()` 함수에서 모든 설정 파일을 자동 로드
- `ConnectionManager`에서 설정에 따라 거래소별 세션을 동적으로 생성
- 설정 파일이 없거나 파싱 실패 시 기본 설정으로 폴백
- 시작 시 로드된 설정에 대한 상세 정보를 로그로 출력