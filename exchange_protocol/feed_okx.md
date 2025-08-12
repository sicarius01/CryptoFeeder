# OKX WebSocket Feed 구조 분석

## 개요
OKX는 현물(Spot)과 선물(Futures/Swap) 시장에 대해 통합된 V5 WebSocket API를 제공합니다. 채널 기반 구독 시스템을 사용하며, 심볼 형식이 시장에 따라 다릅니다.

## WebSocket 엔드포인트

### 공통 엔드포인트 (모든 시장)
- **Production**: `wss://ws.okx.com:8443/ws/v5/public`
- **Demo**: `wss://wspap.okx.com:8443/ws/v5/public`

현물과 선물 모두 동일한 WebSocket 엔드포인트를 사용하지만, 심볼 형식으로 구분됩니다.

## 심볼 형식

### 현물 (Spot)
- **형식**: `{BASE}-{QUOTE}`
- **예시**: `BTC-USDT`, `ETH-USDT`, `SOL-USDT`
- **특징**: 하이픈(-)으로 구분, 대문자 사용

### 선물 (Futures/Swap)
- **형식**: `{BASE}-{QUOTE}-SWAP`
- **예시**: `BTC-USDT-SWAP`, `ETH-USDT-SWAP`, `SOL-USDT-SWAP`
- **특징**: 영구 스왑 계약을 나타내는 `-SWAP` 접미사

## 구독 방식

### 구독 메시지 형식
```json
{
  "op": "subscribe",
  "args": [
    {
      "channel": "trades",
      "instId": "BTC-USDT"
    },
    {
      "channel": "books",
      "instId": "BTC-USDT"
    }
  ]
}
```

### 채널 타입
- **trades**: 실시간 거래 데이터
- **books**: 호가 스냅샷 데이터 (기본 400레벨)

## 메시지 형식

### 구독 응답

#### 성공
```json
{
  "event": "subscribe",
  "arg": {
    "channel": "trades",
    "instId": "BTC-USDT"
  },
  "connId": "508c024e"
}
```

#### 실패
```json
{
  "event": "error",
  "msg": "Wrong URL or channel:trades,instId:MATIC-USDT doesn't exist.",
  "code": "60018",
  "connId": "508c024e"
}
```

### Trade 메시지
```json
{
  "arg": {
    "channel": "trades",
    "instId": "SOL-USDT"
  },
  "data": [
    {
      "instId": "SOL-USDT",        // 상품 ID
      "tradeId": "324832665",      // 거래 ID
      "px": "179.02",              // 가격
      "sz": "10.7305",             // 수량
      "side": "buy",               // 방향 (buy/sell)
      "ts": "1753966988683",       // 거래 시간 (밀리초)
      "count": "3",                // 집계된 거래 수
      "seqId": 23783258761         // 시퀀스 ID
    }
  ]
}
```

### Depth (Books) 메시지

#### 초기 스냅샷 (action: "snapshot")
```json
{
  "arg": {
    "channel": "books",
    "instId": "SHIB-USDT"
  },
  "action": "snapshot",
  "data": [
    {
      "asks": [                    // 매도 호가
        ["0.000012915", "150000", "0", "1"],    // [가격, 수량, 청산된 주문 수, 주문 수]
        ["0.000012916", "150000", "0", "1"],
        ["0.000012917", "37442726", "0", "2"]
      ],
      "bids": [                    // 매수 호가
        ["0.000012914", "124260183", "0", "6"],
        ["0.000012913", "33047756", "0", "1"],
        ["0.000012912", "163618866", "0", "6"]
      ],
      "ts": "1753966988360",       // 타임스탬프
      "checksum": 12345678,        // 체크섬 (옵션)
      "prevSeqId": -1,             // 이전 시퀀스 ID
      "seqId": 14577391816         // 현재 시퀀스 ID
    }
  ]
}
```

#### 증분 업데이트 (action: "update")
```json
{
  "arg": {
    "channel": "books",
    "instId": "BTC-USDT"
  },
  "action": "update",
  "data": [
    {
      "asks": [
        ["68000.5", "0.5", "0", "1"],     // 새로운/업데이트된 호가
        ["68001.0", "0", "0", "0"]        // 수량 0 = 해당 레벨 제거
      ],
      "bids": [
        ["67999.5", "1.2", "0", "2"]
      ],
      "ts": "1753966989000",
      "checksum": 87654321,
      "prevSeqId": 14577391816,
      "seqId": 14577391817
    }
  ]
}
```

## Depth 데이터 처리 방식

### 스냅샷 + 증분 업데이트 방식
OKX는 초기 스냅샷을 제공하고 이후 증분 업데이트를 전송합니다.

1. **초기 스냅샷 (action: "snapshot")**
   - 구독 시작 시 전체 오더북 상태 제공
   - 최대 400개 레벨 (설정에 따라 다름)
   - 이를 기반으로 로컬 오더북 구성

2. **증분 업데이트 (action: "update")**
   - 변경된 가격 레벨만 전송
   - 수량이 "0"인 경우 해당 가격 레벨 제거
   - 수량이 0이 아닌 경우 업데이트 또는 추가

3. **순서 검증**
   - `seqId`로 메시지 순서 검증
   - `prevSeqId`가 현재 오더북의 `seqId`와 일치해야 함
   - 불일치 시 재구독 필요

4. **체크섬 검증** (옵션)
   - 오더북 무결성 확인용
   - 상위 25개 레벨로 계산

### 호가 데이터 필드
- **[0]**: 가격 (문자열)
- **[1]**: 수량 (문자열)
- **[2]**: 청산된 주문 수 (문자열, 일반적으로 "0")
- **[3]**: 해당 가격의 주문 수 (문자열)

## 주요 특징

1. **통합 API**
   - 현물과 선물이 동일한 WebSocket 엔드포인트 사용
   - 심볼 형식으로 시장 구분

2. **채널 기반 구독**
   - 각 데이터 타입(trades, books)별로 별도 채널
   - 심볼별로 개별 구독 필요

3. **Depth 업데이트 방식**
   - 초기 스냅샷 제공
   - 이후 증분 업데이트
   - 시퀀스 ID로 순서 보장

4. **심볼 형식 차이**
   - 현물: `BTC-USDT`
   - 선물: `BTC-USDT-SWAP`

5. **에러 처리**
   - 존재하지 않는 심볼 구독 시 명확한 에러 메시지
   - 예: MATIC-USDT는 POL-USDT로 변경됨

## Ping/Pong
- 클라이언트가 "ping" 전송
- 서버가 "pong" 응답
- 30초 이내 응답 필요 (연결 유지)

## 참고사항
- 모든 가격과 수량은 문자열로 전송
- 타임스탬프는 밀리초 단위
- Trade 데이터에서 `count`는 집계된 거래 수를 나타냄
- Books 데이터는 가격순 정렬 (Bids 내림차순, Asks 오름차순)
- 일부 심볼은 리브랜딩으로 변경될 수 있음 (예: MATIC → POL)