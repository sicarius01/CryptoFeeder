# Binance WebSocket Feed 구조 분석

## 개요
Binance는 현물(Spot)과 선물(Futures) 시장에 대해 서로 다른 WebSocket 엔드포인트와 메시지 형식을 제공합니다.

## WebSocket 엔드포인트

### 현물 (Spot)
- **Production**: `wss://stream.binance.com:9443/ws`
- **Testnet**: `wss://testnet.binance.vision/ws`
- **연결 형식**: `wss://stream.binance.com:9443/ws/{stream1}/{stream2}/...`
- **예시**: `wss://stream.binance.com:9443/ws/btcusdt@trade/btcusdt@depth/ethusdt@trade/ethusdt@depth`

### 선물 (Futures)
- **Production**: `wss://fstream.binance.com/ws`
- **Testnet**: `wss://testnet.binancefuture.com`
- **연결 형식**: `wss://fstream.binance.com/stream?streams={stream1}/{stream2}/...`
- **예시**: `wss://fstream.binance.com/stream?streams=btcusdt@aggTrade/btcusdt@depth/ethusdt@aggTrade/ethusdt@depth`

## 스트림 타입

### Trade 데이터
- **현물**: `{symbol}@trade` (개별 거래)
- **선물**: `{symbol}@aggTrade` (집계된 거래)

### Depth (호가) 데이터
- **현물/선물 동일**: `{symbol}@depth`
- **업데이트 방식**: 증분 업데이트 (Incremental Updates)

## 메시지 형식

### 현물 Trade 메시지
```json
{
  "e": "trade",          // 이벤트 타입
  "E": 1753966988114,    // 이벤트 시간
  "s": "SOLUSDT",        // 심볼
  "t": 1436308964,       // Trade ID
  "p": "179.02000000",   // 가격
  "q": "1.54400000",     // 수량
  "T": 1753966988114,    // 거래 시간
  "m": false,            // Is buyer the market maker?
  "M": true              // Ignore
}
```

### 선물 Trade 메시지 (aggTrade)
```json
{
  "stream": "solusdt@aggTrade",
  "data": {
    "e": "aggTrade",       // 이벤트 타입
    "E": 1753966988030,    // 이벤트 시간
    "a": 934054024,        // Aggregate trade ID
    "s": "SOLUSDT",        // 심볼
    "p": "178.9500",       // 가격
    "q": "8.00",           // 수량
    "f": 2516917476,       // First trade ID
    "l": 2516917479,       // Last trade ID
    "T": 1753966987875,    // 거래 시간
    "m": false             // Is buyer the market maker?
  }
}
```

### Depth 메시지 (현물/선물 동일)
```json
{
  "e": "depthUpdate",      // 이벤트 타입
  "E": 1753966988116,      // 이벤트 시간
  "s": "NEARUSDT",         // 심볼
  "U": 7892277646,         // First update ID in event
  "u": 7892277668,         // Final update ID in event
  "b": [                   // Bids to update
    ["2.66700000", "7622.70000000"],
    ["2.66600000", "7393.50000000"]
  ],
  "a": [                   // Asks to update
    ["2.66800000", "10112.90000000"],
    ["2.66900000", "12783.70000000"]
  ]
}
```

선물의 경우 추가 필드:
```json
{
  "pu": 8203885932051,     // Previous final update ID
  "T": 1753966988029       // Transaction time
}
```

## Depth 데이터 처리 방식

### 증분 업데이트 (Incremental Update) 방식
Binance는 초기 스냅샷을 제공하지 않고, **변경된 호가 정보만** 전송합니다.

1. **초기 스냅샷 필요**
   - WebSocket에서는 스냅샷을 제공하지 않음
   - REST API로 초기 오더북 상태를 가져와야 함
   - `/api/v3/depth` (현물) 또는 `/fapi/v1/depth` (선물)

2. **업데이트 적용 규칙**
   - 각 업데이트는 `U` (첫 번째 업데이트 ID)와 `u` (마지막 업데이트 ID)를 포함
   - 수량이 "0.00000000"인 경우 해당 가격 레벨 제거
   - 수량이 0이 아닌 경우 해당 가격 레벨 업데이트 또는 추가

3. **업데이트 순서 검증**
   - 이전 업데이트의 `u` + 1 = 현재 업데이트의 `U`여야 함
   - 순서가 맞지 않으면 REST API로 다시 스냅샷을 가져와야 함

### 현재 구현의 문제점
현재 코드는 각 depth 업데이트를 완전한 스냅샷으로 처리하고 있어, 실제로는 증분 업데이트만 포함된 데이터를 전체 오더북으로 잘못 해석하고 있습니다.

```rust
// 현재 구현 (잘못됨)
let normalized = NormalizedDepth {
    symbol,
    bids,  // 변경된 호가만 포함
    asks,  // 변경된 호가만 포함
    timestamp,
    update_id,
    exchange: "binance".to_string(),
    market_type,
};
```

### 올바른 구현 방향
1. 초기 REST API 호출로 전체 오더북 스냅샷 획득
2. WebSocket 증분 업데이트를 받아 오더북 상태 유지
3. update_id 순서 검증으로 데이터 무결성 확인
4. 수량 0인 경우 해당 가격 레벨 제거

## 주요 특징

1. **스트림 형식 차이**
   - 현물: 개별 trade 이벤트 (`@trade`)
   - 선물: 집계된 trade 이벤트 (`@aggTrade`)

2. **URL 구조 차이**
   - 현물: 직접 스트림 경로 사용
   - 선물: 쿼리 파라미터로 스트림 지정

3. **메시지 래핑**
   - 현물: 직접 메시지 형식
   - 선물: `stream`과 `data` 필드로 래핑

4. **Depth 업데이트**
   - 증분 업데이트만 제공 (초기 스냅샷 없음)
   - 클라이언트가 오더북 상태를 직접 관리해야 함
   - 수량 0은 해당 가격 레벨 제거를 의미

## 참고사항
- 심볼은 소문자로 변환하여 사용 (예: BTCUSDT → btcusdt)
- 모든 가격과 수량은 문자열로 전송됨
- 타임스탬프는 밀리초 단위
- Ping/Pong 메커니즘으로 연결 상태 유지