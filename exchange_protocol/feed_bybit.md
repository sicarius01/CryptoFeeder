# Bybit WebSocket Feed 구조 분석

## 개요
Bybit은 현물(Spot)과 선물(Futures/Linear) 시장에 대해 동일한 WebSocket 프로토콜을 사용하며, V5 API를 통해 통합된 인터페이스를 제공합니다.

## WebSocket 엔드포인트

### 현물 (Spot)
- **Production**: `wss://stream.bybit.com/v5/public/spot`
- **Testnet**: `wss://stream-testnet.bybit.com/v5/public/spot`

### 선물 (Futures/Linear)
- **Production**: `wss://stream.bybit.com/v5/public/linear`
- **Testnet**: `wss://stream-testnet.bybit.com/v5/public/linear`

## 구독 방식

### 구독 메시지 형식
```json
{
  "op": "subscribe",
  "args": [
    "publicTrade.BTCUSDT",
    "orderbook.50.BTCUSDT",
    "publicTrade.ETHUSDT",
    "orderbook.50.ETHUSDT"
  ]
}
```

### 중요 제한사항
- **최대 10개 args per message**: 한 번의 구독 메시지에 최대 10개의 토픽만 포함 가능
- 10개 이상 구독 시 여러 메시지로 나누어 전송 필요
- 메시지 간 100ms 지연 권장 (rate limiting 방지)

## 스트림 타입

### Trade 데이터
- **토픽**: `publicTrade.{symbol}`
- **심볼 형식**: 대문자 (예: BTCUSDT)

### Depth (호가) 데이터
- **토픽**: `orderbook.{depth}.{symbol}`
- **depth 옵션**: 1, 50, 200, 500
- **현재 사용**: `orderbook.50.{symbol}` (상위 50개 호가)

## 메시지 형식

### 구독 응답
```json
{
  "success": true,
  "ret_msg": "",
  "conn_id": "...",
  "op": "subscribe"
}
```

### Trade 메시지
```json
{
  "topic": "publicTrade.SOLUSDT",
  "type": "snapshot",
  "ts": 1753966982387,
  "data": [
    {
      "i": "11102d67-c29f-5a7a-97de-3ba2be6bb91d",  // Trade ID
      "T": 1753966982385,                            // 거래 시간
      "p": "179.17",                                  // 가격
      "v": "8.088",                                   // 수량
      "S": "Buy",                                     // 방향 (Buy/Sell)
      "s": "SOLUSDT",                                 // 심볼
      "BT": false                                     // 블록 거래 여부
    }
  ]
}
```

### Depth 메시지

#### 1. 초기 스냅샷 (type: "snapshot")
```json
{
  "topic": "orderbook.50.NEARUSDT",
  "type": "snapshot",
  "ts": 1753966982357,
  "data": {
    "s": "NEARUSDT",                    // 심볼
    "b": [                              // Bids (매수 호가)
      ["2.651", "2025.88"],            // [가격, 수량]
      ["2.65", "12530.42"],
      ["2.649", "28892.48"]
    ],
    "a": [                              // Asks (매도 호가)
      ["2.653", "10128.9"],            // [가격, 수량]
      ["2.654", "10109.89"],
      ["2.655", "24765.05"]
    ],
    "u": 8203896021959,                 // Update ID
    "seq": 36813765825                  // Cross sequence
  }
}
```

#### 2. 증분 업데이트 (type: "delta")
```json
{
  "topic": "orderbook.50.NEARUSDT",
  "type": "delta",
  "ts": 1753966982404,
  "data": {
    "s": "NEARUSDT",
    "b": [
      ["2.667", "0"],                   // 수량 0 = 해당 가격 레벨 제거
      ["2.666", "15616.1"]              // 새로운 수량으로 업데이트
    ],
    "a": [
      ["2.668", "23646.71"],
      ["2.669", "37499.21"]
    ],
    "u": 8203896022090,
    "seq": 36813765935
  }
}
```

## Depth 데이터 처리 방식

### 스냅샷 + 증분 업데이트 방식
Bybit은 Binance와 달리 **초기 스냅샷을 제공**하고 이후 증분 업데이트를 전송합니다.

1. **초기 스냅샷 (type: "snapshot")**
   - 구독 시작 시 전체 오더북 상태 제공
   - 상위 50개 매수/매도 호가 포함
   - 이를 기반으로 로컬 오더북 구성

2. **증분 업데이트 (type: "delta")**
   - 변경된 가격 레벨만 전송
   - 수량이 "0"인 경우 해당 가격 레벨 제거
   - 수량이 0이 아닌 경우 업데이트 또는 추가

3. **업데이트 순서 검증**
   - `u` (update ID)와 `seq` (cross sequence) 필드로 순서 검증
   - 메시지 누락 시 재구독 필요

### 현재 구현 분석
현재 코드는 스냅샷과 델타를 구분하지 않고 처리하고 있습니다:

```rust
// 현재 구현 - type 필드를 무시하고 모든 메시지를 동일하게 처리
if let Ok(depth) = serde_json::from_value::<BybitDepthSnapshot>(response.data) {
    // 스냅샷과 델타를 구분하지 않음
    let normalized = NormalizedDepth {
        symbol,
        bids,  // 델타의 경우 변경된 호가만 포함
        asks,  // 델타의 경우 변경된 호가만 포함
        timestamp,
        update_id,
        exchange: "Bybit".to_string(),
        market_type,
    };
}
```

### 올바른 구현 방향
1. `type` 필드 확인하여 스냅샷과 델타 구분
2. 스냅샷: 전체 오더북 재구성
3. 델타: 기존 오더북에 변경사항만 적용
4. 수량 "0" 처리로 가격 레벨 제거
5. update_id 순서 검증

## 주요 특징

1. **통합 V5 API**
   - 현물과 선물이 동일한 프로토콜 사용
   - 메시지 형식 통일

2. **구독 제한**
   - 한 메시지당 최대 10개 토픽
   - 청크 단위로 구독 필요

3. **Depth 업데이트 방식**
   - 초기 스냅샷 제공 (Binance와 차이점)
   - 이후 증분 업데이트만 전송
   - 수량 0으로 레벨 제거 표시

4. **심볼 형식**
   - 대문자 사용 (BTCUSDT)
   - 구독 시 대문자로 변환 필요

5. **Ping/Pong**
   - 클라이언트가 "ping" 전송
   - 서버가 "pong" 응답
   - 20초 간격 권장

## 참고사항
- Trade 메시지는 배열로 제공되며, 여러 거래 포함 가능
- Depth는 가격 정렬된 상태로 제공 (Bids 내림차순, Asks 오름차순)
- 모든 가격과 수량은 문자열로 전송
- 타임스탬프는 밀리초 단위