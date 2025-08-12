
# Git 커밋 메시지 규칙 (Git Commit Message Convention) 📜

모든 커밋 메시지는 아래의 구조와 규칙을 따릅니다. 이는 커밋 히스토리의 가독성을 높이고, 변경 사항을 쉽게 추적하며, 버전 로그를 자동으로 생성하는 데 큰 도움이 됩니다.

---

## 1. 기본 구조 (Basic Structure)

커밋 메시지는 **헤더(Header)**, **본문(Body)**, **꼬리말(Footer)** 세 부분으로 구성됩니다.

<type>(<scope>): <subject>
<BLANK LINE>

<body>
<BLANK LINE>
<footer>


  - **헤더(Header)**: 필수 항목이며, 커밋의 전체 내용을 요약합니다.
  - **본문(Body)**: 선택 항목이며, 헤더만으로 설명이 부족할 때 상세한 설명을 추가합니다.
  - **꼬리말(Footer)**: 선택 항목이며, 관련된 이슈 번호나 브레이킹 체인지(Breaking Change) 정보를 명시합니다.

-----

## 2\. 헤더(Header) 작성법

헤더는 `타입(스코프): 제목`의 형식을 가집니다.

### A. 타입 (Type) 종류

커밋의 성격을 나타내는 타입입니다. 아래 중 하나를 선택해야 합니다.

  - **`feat`**: 새로운 기능 추가 (예: Bybit 거래소 지원 추가)
  - **`fix`**: 버그 수정 (예: Upbit 체결 데이터 파싱 오류 수정)
  - **`docs`**: 문서 변경 (예: `protocol_spec.md` 파일 업데이트)
  - **`perf`**: 성능을 개선하는 코드 변경 (예: UDP 직렬화 로직 최적화)
  - **`refactor`**: 기능 변경이나 버그 수정이 아닌 코드 구조 개선 (예: `connection_manager` 모듈 내부 리팩토링)
  - **`test`**: 테스트 코드 추가 또는 수정
  - **`style`**: 코드 포맷팅, 세미콜론 누락 등 기능에 영향을 주지 않는 스타일 변경 (`cargo fmt` 실행 등)
  - **`chore`**: 빌드 스크립트, 패키지 매니저 설정 등 기타 유지보수 작업

### B. 스코프 (Scope) 종류

변경 사항이 영향을 미치는 영역을 명시합니다. 프로젝트의 모듈 구조와 일치시킵니다.

  - **`core`**: 여러 모듈에 걸친 핵심 로직
  - **`protocol`**: `udp_packet.md` 등 프로토콜 명세 또는 직렬화/역직렬화 로직
  - **`ws-manager`**: WebSocket 연결 관리자 모듈
  - **`parser`**: JSON 파서 모듈
  - **`udp`**: UDP 브로드캐스터 모듈
  - **`binance`**, **`okx`**, **`upbit`**: 특정 거래소에 한정된 코드
  - **`ci`**: CI/CD 관련 설정
  - **`docs`**: 문서 전반

### C. 제목 (Subject)

  - **50자 이내**로 간결하게 작성합니다.
  - \*\*명령형(Imperative mood)\*\*으로 작성합니다. (예: "Fix bug" (O), "Fixed a bug" (X))
  - 문장의 첫 글자는 **대문자**로 시작합니다.
  - 문장 끝에 마침표를 찍지 않습니다.

-----

## 3\. 커밋 작성 원칙

  - **하나의 커밋에는 하나의 논리적 변경사항만** 포함해야 합니다. (Atomic Commits)
      - 예: 기능 추가와 리팩토링을 한 커밋에 담지 마십시오.
  - **본문(Body)은** 헤더만으로 설명이 부족할 때, **"무엇을"** 그리고 **"왜"** 변경했는지 상세히 서술합니다. "어떻게"는 코드가 설명하므로 생략해도 좋습니다.
  - 본문은 한 줄에 **72자**를 넘지 않도록 줄 바꿈을 해주는 것이 좋습니다.

-----

## 4\. 커밋 예시 (Commit Examples)

**Case 1: 간단한 기능 추가**

> `feat(binance): Add support for Futures market WebSocket`

**Case 2: 특정 스코프의 버그 수정**

> `fix(parser): Correct parsing logic for Upbit's trade quantity`

**Case 3: 문서 업데이트**

> `docs(protocol): Add specification for Heartbeat event packet`

**Case 4: 성능 개선**

> `perf(protocol): Optimize packet serialization using pre-allocation`

**Case 5: 본문과 꼬리말을 포함한 커밋**

> ```
> refactor(ws-manager): Rework reconnection logic to use exponential backoff

> The previous reconnection logic used a fixed delay, which could cause
> server overload during widespread outages.

> This change implements an exponential backoff strategy with jitter
> to distribute reconnection attempts more evenly and improve system
> stability.

> Resolves: #42
> ```

**Case 6: 브레이킹 체인지(Breaking Change)를 포함한 커밋**

> ```
> feat(protocol): Change all timestamp fields from u64 to i64

> BREAKING CHANGE: The `exchange_timestamp` and `local_timestamp` fields
> in the PacketHeader are now `i64` instead of `u64` to allow for
> negative values in specific test scenarios. All downstream consumers
> must update their decoding logic.
> ```