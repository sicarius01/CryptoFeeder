# 시스템 이벤트 UDP 멀티캐스트 프로토콜 명세서 v1.0

**작성일:** 2025년 1월 1일  
**작성자:** CryptoFeeder Team  
**연관 문서:** `udp_packet.md`

---

## 1. 개요 (Overview)

본 문서는 CryptoFeeder 시스템의 상태, 연결, 구독 등의 이벤트를 다수의 클라이언트에게 실시간으로 전송하기 위한 이벤트 프로토콜을 정의합니다. 이 프로토콜은 기존 `udp_packet.md`에서 정의된 기본 패킷 구조를 확장하여 시스템 모니터링과 디버깅을 지원합니다.

### 주요 설계 원칙

* **기존 호환성**: `udp_packet.md`의 `PacketHeader` 구조를 그대로 활용하여 동일한 수신 인프라 사용
* **확장성**: 새로운 이벤트 타입을 쉽게 추가할 수 있는 구조
* **효율성**: 고정 크기 필드를 사용하여 빠른 직렬화/역직렬화 지원
* **모니터링 친화적**: 시스템 상태를 실시간으로 추적할 수 있는 충분한 정보 제공

---

## 2. 메시지 타입 정의 (Message Types)

기존 `udp_packet.md`의 `message_type` 필드에서 사용되지 않는 100번대 값을 할당합니다.

| message_type | 이벤트 명 | 설명 |
|:-------------|:----------|:-----|
| `100` | `SystemHeartbeat` | 피더 생존 신호 |
| `101` | `ConnectionStatus` | 거래소 연결 상태 변경 |
| `102` | `SubscriptionStatus` | 구독 성공/실패 상태 |
| `103` | `SystemStats` | 시스템 성능 통계 |
| `104` | `ErrorEvent` | 시스템 오류 이벤트 |
| `105-199` | 예약됨 | 향후 확장을 위해 예약 |

---

## 3. 이벤트 페이로드 구조 (Event Payload Structures)

### 3.1. SystemHeartbeat (message_type = 100)

피더가 정상 동작 중임을 알리는 주기적 신호입니다.

| 오프셋(Byte) | 크기(Byte) | 필드명 | 타입 | 바이트 순서 | 설명 |
|:-------------|:-----------|:-------|:-----|:------------|:-----|
| 0 | 8 | `uptime_seconds` | `uint64` | Little Endian | 시스템 가동 시간 (초) |
| 8 | 4 | `active_connections` | `uint32` | Little Endian | 활성 WebSocket 연결 수 |
| 12 | 4 | `total_packets_sent` | `uint32` | Little Endian | 총 전송된 패킷 수 |

**총 크기:** 16 바이트

### 3.2. ConnectionStatus (message_type = 101)

거래소 연결 상태 변경 시 전송됩니다.

| 오프셋(Byte) | 크기(Byte) | 필드명 | 타입 | 바이트 순서 | 설명 |
|:-------------|:-----------|:-------|:-----|:------------|:-----|
| 0 | 2 | `exchange_id` | `uint16` | Little Endian | 거래소 ID (아래 표 참조) |
| 2 | 1 | `previous_status` | `uint8` | N/A | 이전 연결 상태 |
| 3 | 1 | `current_status` | `uint8` | N/A | 현재 연결 상태 |
| 4 | 4 | `retry_count` | `uint32` | Little Endian | 재연결 시도 횟수 |
| 8 | 8 | `error_code` | `uint64` | Little Endian | 오류 코드 (연결 실패 시) |

**총 크기:** 16 바이트

#### 거래소 ID 매핑

| exchange_id | 거래소 명 |
|:------------|:----------|
| `1` | Binance |
| `2` | OKX |
| `3` | Bybit |
| `4` | Upbit |
| `5` | Bithumb |
| `6` | Coinbase |

#### 연결 상태 코드

| 상태 코드 | 상태 명 | 설명 |
|:----------|:--------|:-----|
| `0` | `Disconnected` | 연결 끊어짐 |
| `1` | `Connecting` | 연결 시도 중 |
| `2` | `Connected` | 연결 성공 |
| `3` | `Reconnecting` | 재연결 시도 중 |
| `4` | `Failed` | 연결 실패 |

### 3.3. SubscriptionStatus (message_type = 102)

특정 심볼 구독 성공/실패 시 전송됩니다.

| 오프셋(Byte) | 크기(Byte) | 필드명 | 타입 | 바이트 순서 | 설명 |
|:-------------|:-----------|:-------|:-----|:------------|:-----|
| 0 | 2 | `exchange_id` | `uint16` | Little Endian | 거래소 ID |
| 2 | 1 | `subscription_type` | `uint8` | N/A | 구독 타입 (1=OrderBook, 2=Trade) |
| 3 | 1 | `status` | `uint8` | N/A | 구독 상태 (0=실패, 1=성공) |
| 4 | 12 | `symbol_short` | `char[12]` | N/A | 축약된 심볼명 (UTF-8, null 종료) |

**총 크기:** 16 바이트

### 3.4. SystemStats (message_type = 103)

시스템 성능 통계를 주기적으로 전송합니다.

| 오프셋(Byte) | 크기(Byte) | 필드명 | 타입 | 바이트 순서 | 설명 |
|:-------------|:-----------|:-------|:-----|:------------|:-----|
| 0 | 4 | `cpu_usage_percent` | `uint32` | Little Endian | CPU 사용률 * 100 |
| 4 | 4 | `memory_usage_mb` | `uint32` | Little Endian | 메모리 사용량 (MB) |
| 8 | 4 | `packets_per_second` | `uint32` | Little Endian | 초당 패킷 전송 수 |
| 12 | 4 | `bytes_per_second` | `uint32` | Little Endian | 초당 바이트 전송량 |

**총 크기:** 16 바이트

### 3.5. ErrorEvent (message_type = 104)

시스템 오류 발생 시 전송됩니다.

| 오프셋(Byte) | 크기(Byte) | 필드명 | 타입 | 바이트 순서 | 설명 |
|:-------------|:-----------|:-------|:-----|:------------|:-----|
| 0 | 4 | `error_type` | `uint32` | Little Endian | 오류 타입 코드 |
| 4 | 2 | `exchange_id` | `uint16` | Little Endian | 관련 거래소 ID (없으면 0) |
| 6 | 2 | `severity` | `uint16` | Little Endian | 심각도 (1=Info, 2=Warning, 3=Error, 4=Critical) |
| 8 | 8 | `error_details` | `uint64` | Little Endian | 오류 상세 정보 (코드나 참조값) |

**총 크기:** 16 바이트

---

## 4. 패킷 구성 예시 (Packet Examples)

### 4.1. SystemHeartbeat 패킷

```
헤더 (67 바이트):
- protocol_version: 1
- sequence_number: 12345
- exchange_timestamp: 현재시각 (나노초)
- local_timestamp: 현재시각 (나노초)
- message_type: 100
- flags_and_count: 0x81 (is_last=1, item_count=1)
- symbol: "SYSTEM\0\0\0..." (20바이트)
- exchange: "FEEDER\0\0\0..." (20바이트)

페이로드 (16 바이트):
- uptime_seconds: 86400 (1일)
- active_connections: 6
- total_packets_sent: 1234567
```

### 4.2. ConnectionStatus 패킷

```
헤더 (67 바이트):
- message_type: 101
- symbol: "BTC/USDT\0\0..." (20바이트)
- exchange: "binance\0\0..." (20바이트)
- (기타 헤더 필드들...)

페이로드 (16 바이트):
- exchange_id: 1 (Binance)
- previous_status: 0 (Disconnected)
- current_status: 1 (Connecting)
- retry_count: 3
- error_code: 0
```

---

## 5. 구현 가이드라인 (Implementation Guidelines)

### 5.1. 전송 주기

- **SystemHeartbeat**: 30초마다
- **SystemStats**: 60초마다
- **ConnectionStatus**: 상태 변경 시 즉시
- **SubscriptionStatus**: 구독 시도 시 즉시
- **ErrorEvent**: 오류 발생 시 즉시

### 5.2. 패킷 헤더 설정

이벤트 패킷의 헤더는 다음과 같이 설정합니다:

- `symbol`: 관련 심볼이 있으면 해당 심볼 (표준화된 `A^B` 형식), 없으면 "SYSTEM"
- `exchange`: 관련 거래소가 있으면 해당 거래소명 (표준화된 형식, 예: "BinanceSpot"), 없으면 "FEEDER"
- `flags_and_count`: 일반적으로 0x81 (is_last=1, item_count=1)
- `exchange_timestamp`: 이벤트 발생 시각
- `local_timestamp`: 패킷 전송 시각

### 5.3. 수신 측 처리

수신 측에서는 `message_type`이 100번대인 패킷을 이벤트 패킷으로 인식하고, 해당 타입에 맞는 구조체로 페이로드를 파싱해야 합니다.

---

## 6. Rust 구현 예시

```rust
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct SystemHeartbeat {
    pub uptime_seconds: u64,
    pub active_connections: u32,
    pub total_packets_sent: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct ConnectionStatus {
    pub exchange_id: u16,
    pub previous_status: u8,
    pub current_status: u8,
    pub retry_count: u32,
    pub error_code: u64,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct SubscriptionStatus {
    pub exchange_id: u16,
    pub subscription_type: u8,
    pub status: u8,
    pub symbol_short: [u8; 12],
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct SystemStats {
    pub cpu_usage_percent: u32,
    pub memory_usage_mb: u32,
    pub packets_per_second: u32,
    pub bytes_per_second: u32,
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct ErrorEvent {
    pub error_type: u32,
    pub exchange_id: u16,
    pub severity: u16,
    pub error_details: u64,
}
```

---

## 7. 버전 관리 (Version Management)

이벤트 프로토콜은 기본 패킷 프로토콜과 동일한 버전 체계를 따릅니다. 새로운 이벤트 타입 추가는 하위 호환성을 유지하며, 기존 이벤트 구조체 변경 시에는 프로토콜 버전을 업그레이드해야 합니다.

---

**문서 끝**