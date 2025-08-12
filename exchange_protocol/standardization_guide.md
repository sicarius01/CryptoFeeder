# 거래소 및 심볼 표준화 가이드 (Exchange & Symbol Standardization Guide)

**작성일:** 2025년 1월 1일  
**작성자:** CryptoFeeder Team  
**연관 문서:** `../udp_packet_detail/udp_packet.md`, `../event_packet.md`

---

## 1. 개요 (Overview)

CryptoFeeder는 다양한 거래소에서 제공하는 서로 다른 심볼 표기법과 API 구조를 통일된 형식으로 표준화하여 처리합니다. 이를 통해 클라이언트 애플리케이션은 거래소별 차이점을 신경 쓰지 않고 일관된 데이터를 수신할 수 있습니다.

---

## 2. 심볼 표준화 규칙 (Symbol Standardization Rules)

### 2.1. 기본 형식

**표준 형식**: `A^B`
- `A`: 거래 대상 코인 (Base Asset)
- `B`: 통화 화폐 (Quote Currency)

### 2.2. 변환 원칙

1. **Base Asset 우선**: 항상 거래하려는 주요 코인을 앞에 배치
2. **Quote Currency 후순**: 결제에 사용되는 통화를 뒤에 배치
3. **일관성 유지**: 같은 페어는 거래소에 관계없이 동일한 표기

### 2.3. 거래소별 변환 매핑

#### Binance
```
원본 → 표준
BTCUSDT → BTC^USDT
ETHBTC → ETH^BTC
ADAUSDT → ADA^USDT
```

#### Upbit
```
원본 → 표준
KRW-BTC → BTC^KRW
KRW-ETH → ETH^KRW
BTC-ADA → ADA^BTC
```

#### Bybit
```
원본 → 표준
BTCUSDT → BTC^USDT
ETHUSD → ETH^USD
ADAUSDT → ADA^USDT
```

#### OKX
```
원본 → 표준
BTC-USDT → BTC^USDT
ETH-BTC → ETH^BTC
ADA-USDT → ADA^USDT
```

#### Coinbase
```
원본 → 표준
BTC-USD → BTC^USD
ETH-USD → ETH^USD
ADA-USD → ADA^USD
```

#### Bithumb
```
원본 → 표준
BTC_KRW → BTC^KRW
ETH_KRW → ETH^KRW
ADA_KRW → ADA^KRW
```

---

## 3. 거래소명 표준화 규칙 (Exchange Name Standardization Rules)

### 3.1. 명명 규칙

**형식**: `{ExchangeName}{MarketType}`

### 3.2. 거래소별 표준명

#### Binance
- **BinanceSpot**: 바이낸스 현물 거래
- **BinanceFutures**: 바이낸스 선물 거래 (USDT-M)
- **BinanceCoinFutures**: 바이낸스 코인 선물 거래 (COIN-M)

#### Bybit
- **BybitSpot**: 바이빗 현물 거래
- **BybitLinear**: 바이빗 USDT 무기한 선물
- **BybitInverse**: 바이빗 코인 무기한 선물
- **BybitOption**: 바이빗 옵션 거래

#### OKX
- **OkxSpot**: OKX 현물 거래
- **OkxSwap**: OKX 무기한 스왑
- **OkxFutures**: OKX 만기 선물
- **OkxOption**: OKX 옵션 거래

#### 기타 거래소
- **UpbitSpot**: 업비트 현물 거래
- **BithumbSpot**: 빗썸 현물 거래
- **CoinbaseSpot**: 코인베이스 현물 거래

---

## 4. 구현 세부사항 (Implementation Details)

### 4.1. 파싱 단계 적용

모든 표준화는 데이터 파싱 단계(`data_parser.rs`)에서 자동으로 적용됩니다:

1. **WebSocket 원시 데이터 수신**
2. **JSON 파싱** (`simd-json` 사용)
3. **심볼 및 거래소명 표준화** ← 이 단계에서 적용
4. **바이너리 패킷 생성**
5. **UDP 전송**

### 4.2. 20바이트 제한 처리

UDP 패킷의 `symbol`과 `exchange` 필드는 각각 20바이트로 제한됩니다:

- **잘림 처리**: 20바이트를 초과하는 경우 앞에서부터 19바이트만 사용
- **null 종료**: 마지막 바이트는 항상 null('\0')로 설정
- **UTF-8 인코딩**: 모든 문자열은 UTF-8로 인코딩

### 4.3. 코드 예시

```rust
// 심볼 표준화 함수
fn normalize_symbol(exchange: &str, original_symbol: &str) -> String {
    match exchange.to_lowercase().as_str() {
        "binance" => normalize_binance_symbol(original_symbol),
        "upbit" => normalize_upbit_symbol(original_symbol),
        "bybit" => normalize_bybit_symbol(original_symbol),
        // ... 기타 거래소
        _ => original_symbol.to_string(),
    }
}

// 거래소명 표준화 함수
fn normalize_exchange_name(exchange: &str, market_type: &str) -> String {
    match exchange.to_lowercase().as_str() {
        "binance" => format!("Binance{}", capitalize_first(market_type)),
        "bybit" => format!("Bybit{}", capitalize_first(market_type)),
        // ... 기타 거래소
        _ => format!("{}_{}", exchange, market_type),
    }
}
```

---

## 5. 품질 보증 (Quality Assurance)

### 5.1. 테스트 케이스

모든 표준화 함수는 다음 테스트를 통과해야 합니다:

```rust
#[test]
fn test_symbol_standardization() {
    assert_eq!(normalize_binance_symbol("BTCUSDT"), "BTC^USDT");
    assert_eq!(normalize_upbit_symbol("KRW-BTC"), "BTC^KRW");
    assert_eq!(normalize_bybit_symbol("BTCUSDT"), "BTC^USDT");
}

#[test]
fn test_exchange_standardization() {
    assert_eq!(normalize_exchange_name("binance", "spot"), "BinanceSpot");
    assert_eq!(normalize_exchange_name("bybit", "linear"), "BybitLinear");
    assert_eq!(normalize_exchange_name("okx", "swap"), "OkxSwap");
}
```

### 5.2. 검증 도구

`packet-decoder` 유틸리티를 사용하여 전송된 패킷의 심볼과 거래소명이 올바르게 표준화되었는지 확인할 수 있습니다.

---

## 6. 확장성 고려사항 (Scalability Considerations)

### 6.1. 새로운 거래소 추가

새로운 거래소를 추가할 때는 다음을 수행해야 합니다:

1. **심볼 변환 함수 추가**: `normalize_{exchange}_symbol()` 함수 구현
2. **거래소명 매핑 추가**: `normalize_exchange_name()` 함수에 케이스 추가
3. **테스트 케이스 작성**: 새로운 거래소의 표준화 검증
4. **문서 업데이트**: 본 문서의 매핑 테이블에 추가

### 6.2. 새로운 심볼 패턴 지원

기존 패턴과 다른 새로운 심볼 형식이 등장할 경우:

1. **패턴 분석**: 새로운 형식의 구조 파악
2. **변환 로직 확장**: 기존 함수에 새로운 케이스 추가
3. **역호환성 확인**: 기존 심볼들이 여전히 올바르게 변환되는지 검증

---

## 7. 모니터링 및 디버깅 (Monitoring & Debugging)

### 7.1. 로그 출력

표준화 과정에서 다음과 같은 로그가 출력됩니다:

```
[DEBUG] 심볼 표준화: binance BTCUSDT → BTC^USDT
[DEBUG] 거래소 표준화: binance spot → BinanceSpot
[WARN] 알 수 없는 심볼 패턴: UNKNOWN_SYMBOL
```

### 7.2. 메트릭 수집

- 표준화 성공/실패 건수
- 처리 시간 측정
- 알 수 없는 패턴 발견 빈도

---

**문서 끝**