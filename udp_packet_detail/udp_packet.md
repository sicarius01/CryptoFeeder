# 실시간 금융 데이터 UDP 멀티캐스트 프로토콜 명세서 v1.0

**작성일:** 2025년 8월 1일  
**작성자:** sicarius with Gemini

---

## 1. 개요 (Overview)

본 문서는 암호화폐 거래소 등에서 수신한 실시간 금융 데이터(오더북, 체결 등)를 다수의 클라이언트에게 낮은 지연 시간과 높은 처리량으로 전송하기 위한 UDP 멀티캐스트 프로토콜을 정의합니다.

### 주요 설계 원칙

* **바이너리 포맷**: 문자열 파싱으로 인한 오버헤드를 제거하여 최고 수준의 성능을 지향합니다.
* **MTU 인지**: IP 단편화를 피하기 위해 모든 패킷은 안전한 MTU 크기(1472바이트) 미만으로 설계되었습니다.
* **신뢰성 확보**: 패킷 유실 감지를 위한 시퀀스 번호와 데이터 무결성을 위한 명확한 구조를 포함합니다.
* **확장성**: 프로토콜 버전 명시를 통해 향후 기능 변경 및 확장에 유연하게 대처할 수 있습니다.

---

## 2. 패킷 구조 (Packet Structure)

패킷은 **헤더(Header)**와 **페이로드(Payload)** 두 부분으로 구성됩니다. 모든 멀티-바이트 정수 필드는 **리틀 엔디안(Little Endian)**을 따릅니다.

### 2.1. 전체 레이아웃

```
+--------------------------------+-------------------------------------+
|         헤더 (67 바이트)         |    페이로드 (가변, 최대 1280 바이트)   |
+--------------------------------+-------------------------------------+
| Fixed-size metadata describing | Array of data items (e.g., Order   |
| the packet.                    | Book entries).                      |
+--------------------------------+-------------------------------------+
```

* **최대 아이템 개수**: 80개 (MTU 안전 범위 유지)
* **최대 패킷 크기**: 67 바이트 (헤더) + (80개 * 16 바이트) = 1347 바이트 (MTU 1472 바이트 미만으로 안전)

### 2.2. 헤더 상세 (Header Details - 67 바이트)

| 오프셋(Byte) | 크기(Byte) | 필드명             | 타입     | 바이트 순서 | 설명                                               |
| :----------- | :--------- | :----------------- | :------- | :---------- | :------------------------------------------------- |
| 0            | 1          | `protocol_version`   | `uint8`  | N/A         | 프로토콜 버전. 현재 버전은 `1`                       |
| 1            | 8          | `sequence_number`  | `uint64` | Little Endian  | 패킷의 고유 순서 번호. 1씩 증가.                   |
| 9            | 8          | `exchange_timestamp` | `uint64` | Little Endian  | 거래소에서 이벤트가 발생한 시각 (Unix 나노초)        |
| 17           | 8          | `local_timestamp`  | `uint64` | Little Endian  | 피더에서 패킷을 송신하는 시각 (Unix 나노초)          |
| 25           | 1          | `message_type`     | `uint8`  | N/A         | 메시지 타입 (0~255). 상세 설명은 아래 참조           |
| 26           | 1          | `flags_and_count`  | `uint8`  | N/A         | 플래그 및 아이템 개수 (0~80개, 아래 비트필드 설명 참조) |
| 27           | 20         | `symbol`           | `char[20]` | N/A         | 심볼명 (예: "BTC^USDT"). UTF-8, null로 끝남. 표준화 규칙 참조.      |
| 47           | 20         | `exchange`         | `char[20]` | N/A         | 거래소 이름 (예: "BinanceSpot"). UTF-8, null로 끝남. 표준화 규칙 참조.   |

#### `message_type` 상세 (1 바이트)

* **`0`**: OrderBook 데이터 (depth update)
* **`1`**: TradeTick 데이터 (체결 내역)
* **`2-255`**: 향후 확장을 위해 예약됨

#### `flags_and_count` 비트필드 상세 (1 바이트)

* **비트 7 (최상위 비트): `is_last` 플래그**
    * `1`: 한 웹소켓 메시지에 포함된 데이터의 마지막 패킷임을 의미.
    * `0`: 뒤에 이어서 패킷이 더 있음을 의미.
* **비트 0-6 (하위 7비트): `item_count`**
    * 이 패킷에 포함된 실제 데이터 아이템의 개수 (0~80).
    * 최대 80개로 제한하여 MTU 안전 범위 유지.

### 2.3. 페이로드 상세 (Payload Details)

페이로드 영역에는 `item_count` 개수만큼의 데이터 아이템 구조체가 배열 형태로 연속해서 위치합니다. `message_type` 플래그에 따라 다른 구조체를 사용합니다.

#### OrderBookItem 구조체 (16 바이트) - Depth Update용

| 오프셋(Byte) | 크기(Byte) | 필드명    | 타입    | 바이트 순서 | 설명                                         |
| :----------- | :--------- | :------- | :------ | :---------- | :------------------------------------------- |
| 0            | 8          | `price`    | `int64` | Little Endian  | Scaled Integer 가격 (실제 가격 * 10^8)         |
| 8            | 8          | `quantity_with_flags` | `int64` | Little Endian  | 수량 + 플래그 (아래 비트필드 설명 참조)   |

##### `quantity_with_flags` 비트필드 상세 (OrderBook용)

* **비트 63 (최상위 비트): `is_ask` 플래그**
    * `1`: Ask (매도) 주문
    * `0`: Bid (매수) 주문
* **비트 0-62: `quantity`**
    * Scaled Integer 수량 (실제 수량 * 10^8)
    * 최대값: 2^62 - 1 (약 4.6 * 10^10 원본 수량)

#### TradeTickItem 구조체 (16 바이트) - Trade Tick용

| 오프셋(Byte) | 크기(Byte) | 필드명    | 타입    | 바이트 순서 | 설명                                         |
| :----------- | :--------- | :------- | :------ | :---------- | :------------------------------------------- |
| 0            | 8          | `price`    | `int64` | Little Endian  | Scaled Integer 체결 가격 (실제 가격 * 10^8)    |
| 8            | 8          | `quantity_with_flags` | `int64` | Little Endian  | 체결 수량 + 플래그 (아래 비트필드 설명 참조)   |

##### `quantity_with_flags` 비트필드 상세

* **비트 63 (최상위 비트): `is_buyer_taker` 플래그**
    * `1`: 매수자가 테이커 (시장가 매수)
    * `0`: 매도자가 테이커 (시장가 매도)
* **비트 0-62: `quantity`**
    * Scaled Integer 체결 수량 (실제 수량 * 10^8)
    * 최대값: 2^62 - 1 (약 4.6 * 10^10 원본 수량)

---

## 3. 구현 가이드라인 (Implementation Guidelines)

### 3.1. 데이터 표현

* **Scaled Integer**: 모든 가격/수량 정보는 부동소수점 오차를 없애고 연산 속도를 높이기 위해 실제 값 * $10^8$을 적용한 64비트 정수로 변환하여 다룹니다.
* **문자열**: `symbol`과 `exchange` 필드는 가변 길이 문자열의 복잡성을 피하기 위해 고정 크기(20바이트) 배열로 정의합니다. 실제 문자열 길이가 20바이트보다 짧을 경우, 남은 공간은 널(Null, `\0`)로 채워야 합니다.

### 3.2. 직렬화/역직렬화

* **Endianness**: 모든 `uint64`, `int64` 타입 필드는 리틀 엔디안 순서로 직렬화됩니다. 대부분의 x86/x64 아키텍처에서는 네이티브 바이트 순서와 동일하므로 별도의 변환이 불필요하여 성능상 이점이 있습니다. 다른 아키텍처에서는 적절한 바이트 순서 변환을 수행해야 합니다.
* **메모리 레이아웃**: Rust의 `#[repr(C)]`나 C/C++의 `struct`처럼, 메모리상에 필드가 선언된 순서대로 위치하도록 보장하는 기능을 사용하여 직렬화/역직렬화를 수행해야 합니다.

### 3.3. 데이터 정렬 및 배치 규칙

CryptoFeeder는 패킷 생성 시 다음과 같은 정렬/배치 규칙을 적용합니다:

#### 체결틱 (TradeTick) 정렬:
- **Ask (매수자 테이커)**: 가격 오름차순 정렬
- **Bid (매도자 테이커)**: 가격 내림차순 정렬
- **같은 가격**: 메시지 수신 순서대로 보장
- Ask가 Bid보다 먼저 위치하도록 정렬

#### 오더북 (OrderBook) 정렬:
- **Ask (매도 주문)**: 가격 오름차순 정렬
- **Bid (매수 주문)**: 가격 내림차순 정렬
- Bid가 먼저, Ask가 나중에 배치됩니다

#### 배치 규칙 (중요)
- 한 WebSocket 메시지에 포함된 체결 틱들은 가능한 한 동일한 UDP 패킷으로 묶어 보냅니다.
- `TradeTickItem`은 패킷당 최대 80개까지 담습니다. 80개 초과 시 여러 패킷으로 분할하며, 마지막 패킷에 `is_last=true`를 세팅합니다.
- 오더북(depthUpdate)은 해당 WS 메시지의 bids/asks를 정렬하여 하나의 패킷(또는 80개 단위 분할)로 송신합니다.

### 3.4. 수신 측 로직

1.  패킷 수신 시 가장 먼저 `protocol_version`을 확인하여 자신이 처리할 수 있는 버전인지 검사합니다.
2.  `sequence_number`를 확인하여 이전에 받은 번호보다 1 큰지 검사하고, 아닐 경우 패킷 유실이 발생했음을 인지하고 처리합니다.
3.  `message_type` 필드에서 데이터 타입을 확인합니다.
4.  `flags_and_count` 필드에서 비트 연산을 통해 `is_last` 플래그와 `item_count`를 추출합니다.
5.  `message_type`에 따라 적절한 구조체 타입을 선택합니다:
    - `message_type == 0`: OrderBookItem으로 파싱하고 `is_ask` 플래그 추출
    - `message_type == 1`: TradeTickItem으로 파싱하고 `is_buyer_taker` 플래그 추출
6.  `item_count` 만큼 페이로드를 반복하여 각 데이터 아이템을 파싱합니다.

### 3.5. 멀티캐스트 설정

CryptoFeeder는 config.ini 파일을 통해 멀티캐스트 설정을 관리합니다:

```ini
[UDP]
multicast_addr=239.255.1.1
port=55555
interface_addr=0.0.0.0
```

- **multicast_addr**: 멀티캐스트 IP 주소 (239.0.0.0 ~ 239.255.255.255 범위)
- **port**: UDP 포트 번호
- **interface_addr**: 네트워크 인터페이스 주소 (0.0.0.0은 모든 인터페이스)

---

## 4. Rust 예제 코드

아래는 본 명세서에 따라 작성된 Rust 구조체 예시입니다.

```rust
// C와 동일한 메모리 레이아웃을 보장합니다.
#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct PacketHeader {
    pub protocol_version: u8,      // 1B
    pub sequence_number: u64,      // 8B
    pub exchange_timestamp: u64,   // 8B
    pub local_timestamp: u64,      // 8B
    pub message_type: u8,          // 1B
    pub flags_and_count: u8,       // 1B
    pub symbol: [u8; 20],          // 20B, null-terminated UTF-8
    pub exchange: [u8; 20],        // 20B, null-terminated UTF-8
} // 총 67 바이트

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct OrderBookItem {
    pub price: i64,                // 8B, Scaled by 10^8
    pub quantity_with_flags: i64,  // 8B, quantity + is_ask flag
} // 총 16 바이트

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct TradeTickItem {
    pub price: i64,                // 8B, Scaled by 10^8
    pub quantity_with_flags: i64,  // 8B, quantity + is_buyer_taker flag
} // 총 16 바이트

impl TradeTickItem {
    /// is_buyer_taker 플래그를 추출하는 헬퍼 함수
    pub fn is_buyer_taker(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }

    /// 실제 수량을 추출하는 헬퍼 함수
    pub fn quantity(&self) -> i64 {
        self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF
    }

    /// 수량과 is_buyer_taker 플래그를 설정하는 헬퍼 함수
    pub fn set_quantity_and_flag(&mut self, quantity: i64, is_buyer_taker: bool) {
        let masked_quantity = quantity & 0x7FFF_FFFF_FFFF_FFFF;
        self.quantity_with_flags = if is_buyer_taker {
            masked_quantity | (1i64 << 63)
        } else {
            masked_quantity
        };
    }
}

impl OrderBookItem {
    /// is_ask 플래그를 추출하는 헬퍼 함수
    pub fn is_ask(&self) -> bool {
        (self.quantity_with_flags & (1i64 << 63)) != 0
    }

    /// 실제 수량을 추출하는 헬퍼 함수
    pub fn quantity(&self) -> i64 {
        self.quantity_with_flags & 0x7FFF_FFFF_FFFF_FFFF
    }

    /// 수량과 is_ask 플래그를 설정하는 헬퍼 함수
    pub fn set_quantity_and_flag(&mut self, quantity: i64, is_ask: bool) {
        let masked_quantity = quantity & 0x7FFF_FFFF_FFFF_FFFF;
        self.quantity_with_flags = if is_ask {
            masked_quantity | (1i64 << 63)
        } else {
            masked_quantity
        };
    }
}

impl PacketHeader {
    /// is_last 플래그를 추출하는 헬퍼 함수
    pub fn is_last(&self) -> bool {
        (self.flags_and_count & 0b1000_0000) != 0
    }

    /// item_count를 추출하는 헬퍼 함수
    pub fn item_count(&self) -> u8 {
        self.flags_and_count & 0b0111_1111
    }

    /// message_type이 TradeTick인지 확인하는 헬퍼 함수
    pub fn is_trade_tick(&self) -> bool {
        self.message_type == 1
    }

    /// message_type이 OrderBook인지 확인하는 헬퍼 함수
    pub fn is_order_book(&self) -> bool {
        self.message_type == 0
    }

    /// is_last와 item_count로 flags_and_count 필드를 설정하는 헬퍼 함수
    pub fn set_flags_and_count(&mut self, is_last: bool, count: u8) {
        // count가 최대값(80)을 넘지 않도록 보장 (MTU 안전 범위 유지)
        let item_count = count.min(80) & 0b0111_1111; 
        if is_last {
            self.flags_and_count = item_count | 0b1000_0000;
        } else {
            self.flags_and_count = item_count;
        }
    }

    /// 패킷 정보를 한번에 설정하는 편의 함수
    pub fn set_packet_info(&mut self, message_type: u8, count: u8, is_last: bool) {
        self.message_type = message_type;
        self.set_flags_and_count(is_last, count);
    }
}
```

---

## 5. 심볼 및 거래소 표준화 규칙 (Symbol & Exchange Standardization Rules)

### 5.1. 심볼명 표준화 (Symbol Naming)

CryptoFeeder에서는 다양한 거래소의 심볼 표기법을 통일하기 위해 다음 표준을 적용합니다:

- **형식**: `A^B`
  - `A`: 거래 대상 코인 (예: BTC, ETH, ADA)
  - `B`: 통화 화폐 (Quote Currency, 예: USDT, KRW, BTC)
- **예시**:
  - `BTC^USDT` (비트코인/테더)
  - `ETH^KRW` (이더리움/원화)
  - `ADA^BTC` (에이다/비트코인)

#### 거래소별 변환 예시

| 거래소 | 원본 심볼 | 표준 심볼 |
|--------|----------|----------|
| Binance | BTCUSDT | BTC^USDT |
| Upbit | KRW-BTC | BTC^KRW |
| Bybit | BTCUSDT | BTC^USDT |
| OKX | BTC-USDT | BTC^USDT |
| Coinbase | BTC-USD | BTC^USD |

### 5.2. 거래소명 표준화 (Exchange Naming)

거래소별로 현물/선물/스왑 등의 시장 유형을 명확히 구분하기 위해 다음 명명 규칙을 적용합니다:

#### Binance
- `BinanceSpot`: 바이낸스 현물 시장
- `BinanceFutures`: 바이낸스 선물 시장

#### Bybit
- `BybitSpot`: 바이빗 현물 시장
- `BybitLinear`: 바이빗 무기한 선물 (USDT 기반)
- `BybitInverse`: 바이빗 역방향 선물 (코인 기반)

#### OKX
- `OkxSpot`: OKX 현물 시장
- `OkxSwap`: OKX 무기한 스왑
- `OkxFutures`: OKX 만기 선물

#### 기타 거래소
- `UpbitSpot`: 업비트 현물 시장
- `BithumbSpot`: 빗썸 현물 시장
- `CoinbaseSpot`: 코인베이스 현물 시장

### 5.3. 구현 참고사항

- 모든 심볼과 거래소명은 데이터 파싱 단계에서 자동으로 표준 형식으로 변환됩니다.
- 20바이트 고정 크기 필드에 맞게 적절히 자릅니다.
- UTF-8 인코딩을 사용하며 null 종료 문자를 포함합니다.