# Kythe / Glean 아키텍처의 CodeAtlas 적용 검토 및 개선 플랜

Status:

- Draft (검토 단계, 미착수)

## 1. Objective

Kythe(Google)와 Glean(Meta)의 "컴파일 정보 기반 인덱싱" 아키텍처를 1차 소스(레포 직접 클론)
및 소스 코드 레벨로 검증한 결과를 바탕으로, CodeAtlas에 실제로 적용 가능한 개선을 도출한다.

CodeAtlas의 제약을 최우선으로 둔다:

- 대규모 C++ 프로젝트(30만 파일급)를 **빌드 없이** 합리적 시간 내 인덱싱
- 개발 중 **PC 퍼포먼스 하락 최소화** (백그라운드 증분 + MCP 서비스 동시 운영)
- 증분 인덱싱의 정확도와 비용 균형

따라서 Kythe/Glean의 "정확하지만 무거운" 풀-컴파일 모델을 그대로 이식하지 않는다.
**이미 갖춘 하이브리드(clang USR + tree-sitter) 위에서, 비용 대비 효과가 큰 메커니즘만 선별**한다.

Scope note:

- 본 문서는 검토 + 우선순위 제안이다. 각 항목은 별도 Milestone으로 분리 실행한다.
- 증분 정확성 계약(MS4/MS16)과 DB 스키마는 안전 fallback을 유지한 채로만 확장한다.

---

## 2. 조사 근거 요약 (검증 완료)

Kythe v0.0.75, Glean 최신 레포를 클론하여 docs가 아닌 실제 코드로 14개 주장을 검증
(Kythe 6확인+1부분확인, Glean 7확인). 상세는 `~/.claude/plans/kythe-glean-quiet-conway.md`.

핵심 사실 (CodeAtlas 관련):

| 항목 | Kythe | Glean | CodeAtlas 현황 |
|---|---|---|---|
| 컴파일러 통합 | libclang/LLVM 링크 (`cxx/indexer/cxx/BUILD`) | libclang/libllvm 래핑 (`lang/clang/index.cpp`) | **이미 libclang 사용** (`clang_parser.rs`) |
| 추출/인덱싱 분리 | extractor→`.kzip`(SHA256 CAS)→indexer | clang-index→clang-derive 2단계 | 단일 패스 (clang 직접 파싱) |
| 컴파일러 해소 심볼 | VName | fact ID | **USR** (`clang_parser.rs:563`) |
| 빌드정보 의존 | compile_commands.json (`runextractor/compdb`) | compile_commands.json (cdb) | **compile_commands.json/cpp_context 선택적** |
| 콜그래프 false edge 방지 | `completedby`는 관측시만 방출 | derived fact 2단계 | 17개 휴리스틱 스코어링(`resolver.rs`) |
| 증분 | CAS + set union | unit ownership-set stacking | SHA256 + symbol-level header fanout (MS16) |

결론: CodeAtlas는 이미 Kythe/Glean과 같은 방향(컴파일러 해소 심볼 활용)에 있다.
가장 큰 격차는 **(1) 재파싱 비용을 줄이는 추출 캐시 부재**와
**(2) 휴리스틱 경로와 컴파일러 확정 경로의 신뢰도 구분 부재**다.

---

## 3. 적용 후보 및 우선순위

| # | 인사이트 출처 | 제안 | 목적 적합성 | 비용 | 우선순위 |
|---|---|---|---|---|---|
| A | Kythe `.kzip` CAS | TU 단위 파싱 결과 content-addressable 캐시 | PC부하↓ 증분속도↑ | 중 | **높음** |
| B | Kythe `completedby` 원칙 | 콜 엣지 신뢰도 2계층(compiler-confirmed vs heuristic) | MCP 품질↑ | 저 | **높음** |
| C | Glean clang-index 병렬 | clang TU 파싱 적응형 동시성(부하 인지) | PC부하↓ | 중 | 중 |
| D | Glean ownership-set | derived(propagation) 무효화 정밀화 | 증분 정확도↑ | 고 | 낮음 |
| E | Glean prefix-index 설계 | (해당 없음 — SQLite 인덱스로 충족) | - | - | 제외 |

E 제외 이유: CodeAtlas는 이미 SQLite + 20개 복합 인덱스 + FTS(trigram)로 serving 계층을
분리 구현 중. Glean의 prefix-index 트레이드오프는 RocksDB 특성이라 직접 적용 대상이 아니다.

---

## 4. 제안 A — TU 파싱 결과 Content-Addressable 캐시 (우선순위: 높음)

### 문제

`CppLanguageAdapter::parse_file()`(`indexing.rs:127`)는 매 인덱싱마다 libclang으로 TU를
재파싱한다. 증분에서 헤더 1개가 바뀌면 그 헤더를 include하는 모든 cpp가 재파싱되는데,
대부분의 TU 내용(다른 헤더 + 본문)은 **바이트 단위로 동일**하다. Kythe의 `.kzip`은
정확히 이 재현 비용을 없애기 위해 컴파일 입력을 SHA256으로 content-address한다.

### 설계

Kythe `.kzip` 모델을 경량 차용:

- 캐시 키 = `SHA256(정규화된 컴파일 인자 + 소스 내용 + 직접 include된 헤더들의 내용 해시)`
  - 인자: `indexing.rs:144-157`에서 구성하는 `-I`/`-D` 목록 (정규화 후)
  - Kythe가 `required_input`을 digest에 포함하는 것과 동일 원리
- 캐시 값 = 해당 TU의 `ParseResult`(심볼/raw_calls/references) 직렬화
- 저장: `.codeatlas/parse-cache/` 디렉터리 (CAS, Kythe `files/<sha256>` 레이아웃 차용)
- 적중 시 libclang 호출 자체를 skip → CPU·메모리(CXIndex RSS) 절약

### CodeAtlas 적합성

- **PC 부하↓**: libclang 파싱은 가장 무거운 단계(`acquire_cpp_parse_permit()`로 동시성까지
  제한 중, `indexing.rs:176`). 캐시 적중분은 permit 자체가 불필요.
- **증분 속도↑**: 헤더 fanout 시 시그니처 안 바뀐 TU는 즉시 캐시 적중
- MS16의 symbol-level fanout narrowing과 **상호 보완**: MS16은 "재파싱 대상 파일 수"를 줄이고,
  A는 "재파싱 1건당 비용"을 줄인다.

### 리스크 / 완화

- 캐시 무효화 정확성: 키에 **직접 include 헤더 내용까지** 포함해야 매크로/시그니처 변경 누락
  방지. 전이 include는 비용 문제 → 보수적으로 macro_sensitivity=high 파일은 캐시 우회.
- 디스크 사용: LRU 또는 세대 기반 정리 (`current-db.json` 세대 관리 패턴 재사용)
- 첫 풀 인덱싱엔 이득 없음(미스 100%) → 증분/재시작 시점부터 효과

### 예상 작업 위치

- `indexer/src/indexing.rs` (`CppLanguageAdapter::parse_file` 캐시 게이트)
- 신규 `indexer/src/parse_cache.rs` (CAS 키 계산 + 직렬화 저장/로드)
- `indexer/src/clang_parser.rs` (캐시 키용 직접 include 목록 노출)

---

## 5. 제안 B — 콜 엣지 신뢰도 2계층화 (우선순위: 높음)

### 문제

`resolver.rs`는 USR 기반 fast-path(`resolve_calls` 초입, `pre_resolved_callee_id` 직접 매칭)와
17개 휴리스틱 스코어링(same_parent, parameter_count_match 등)을 **같은 `calls` 테이블에
동일 지위로** 적재한다. MCP 소비자는 어느 엣지가 컴파일러 확정(USR)이고 어느 것이
휴리스틱 추정인지 구분할 수 없다. Kythe는 이 문제를 `completedby`를 **실제 컴파일에서
완성 관계가 관측될 때만** 방출하는 것으로 해결한다(`KytheGraphObserver.cc:957`,
`recordCompletion`). 즉 "확정"과 "추정"을 엣지 종류로 분리한다.

### 설계

CodeAtlas는 이미 `ResolutionStatus`(Resolved/Ambiguous/Unresolved)와 `RawEventSource`를
보유. 이를 MCP 응답까지 일관되게 전파:

- `calls` 행에 `resolution_tier` 추가 (이미 일부 메타 있으면 재사용):
  - `compiler_confirmed`: clang USR로 직접 해소된 엣지 (Kythe `completedby` 등가)
  - `heuristic`: 휴리스틱 스코어링으로 채택 (top_score>0, Ambiguous 포함)
- MCP 도구(`find_callers` 등) 응답에 tier 노출 + 필터 옵션
- AI 에이전트/사용자가 "확정 호출만" 질의 가능 → 잘못된 추정에 의한 오판 방지

### CodeAtlas 적합성

- **MCP 품질↑**: 글로벌 CLAUDE.md의 "MCP 결과는 ground truth 아님, 검증 필요" 원칙과 직접 부합.
  tier가 있으면 에이전트가 어떤 엣지를 추가 검증해야 하는지 스스로 판단.
- 저비용: 데이터는 이미 resolver 내부에 존재. **태깅·전파만** 추가.
- 휴리스틱을 제거하지 않음 — Kythe도 과대추정을 유지하되 다운스트림 필터에 맡기는 철학과 동일.

### 리스크 / 완화

- 스키마 변경 최소화: 컬럼 1개 추가 + 기본값으로 하위호환
- tier 분류 기준의 일관성: USR fast-path 통과 여부를 단일 지점에서 결정

### 예상 작업 위치

- `indexer/src/resolver.rs` (tier 결정 — USR path vs heuristic path 분기점)
- `indexer/src/storage.rs` (`calls` 컬럼 + 쿼리)
- `server/src/` (MCP 응답 필드 + 필터 옵션)

---

## 6. 제안 C — clang 파싱 적응형 동시성 (우선순위: 중)

### 문제

현재 백그라운드 인덱싱 동시성은 정적이다: `main.rs:185`(기본 25% 코어), `indexing.rs:275-311`
(파싱 풀 절반, [4,16] clamp), `acquire_cpp_parse_permit()`로 libclang TU 수 제한. 개발자가
무거운 작업(빌드, 디버깅)을 할 때도 동일 비율을 점유한다.

### 설계

Glean의 work-stealing 병렬 인덱서(`index.cpp:236-278`, 검증됨)는 워커 수 자체는 고정이나,
CodeAtlas는 **부하 인지 적응형**이 목적에 더 맞다:

- 시스템 CPU 사용률 샘플링 → 임계 초과 시 활성 permit 수 동적 축소
- 또는 단순 모델: foreground 활동(파일 저장 빈도/입력) 감지 시 백그라운드 throttle
- `CODEATLAS_BACKGROUND_THREADS` 정적 오버라이드는 유지(하한)

### CodeAtlas 적합성

- 목적의 "PC 부하 최소화"에 직접 기여. 단 A/B보다 효과 측정이 어렵고 OS별 CPU 샘플링 차이 존재.

### 리스크

- 과도한 throttle로 증분 지연 → 하한 보장 필요. 우선 A(절대 비용 감소)를 먼저 적용 후 재평가 권장.

---

## 7. 제안 D — Propagation(derived) 무효화 정밀화 (우선순위: 낮음)

Glean은 derived fact에 ownership-set(`O1 && ... && On`, `uset.h` SetOp::And)을 부여해
증분 시 정확히 무효화한다. CodeAtlas의 `propagation_events`(데이터 흐름 요약)는 증분 시
재계산 범위가 보수적일 수 있다. 다만 ownership-set 전파는 구현 복잡도가 높고(Glean도
"tricky to get right"로 명시), 현재 MS16 fanout으로 대부분 커버된다. **A/B 적용 후
propagation 재계산이 실측 병목으로 확인될 때만** 착수.

---

## 8. 권장 실행 순서

1. **제안 B (신뢰도 2계층)** 먼저 — 저비용·고효과, 스키마 영향 최소, MCP 품질 즉시 개선
2. **제안 A (TU 파싱 캐시)** — 증분/재시작 비용의 구조적 감소, MS16과 상호보완
3. 제안 C — A 적용 후 잔여 부하 측정 결과를 보고 결정
4. 제안 D — 실측 병목 확인 시에만

각 항목은 독립 Milestone(MS20=B, MS21=A 등)으로 분리하고, 기존 관례대로
incremental 결과 == full rebuild 결과 동등성 검증을 acceptance에 포함한다.

---

## 9. 명시적 비채택 (Non-Goals)

- **풀 컴파일/빌드 통합**: Kythe/Glean은 빌드 중 추출하거나 빌드를 선행한다. CodeAtlas의
  "빌드 없이" 제약과 정면 충돌하므로 채택하지 않는다. (compile_commands.json은 인자 메타로만 사용)
- **그래프 DB / RocksDB 전환**: SQLite + 인덱스 + FTS가 serving 요구를 충족. 전환 이득 없음.
- **VName/fact 모델 재설계**: 현 심볼 ID + USR 체계로 동일 목적 달성 중.

---

## 10. 검증 자료 위치

- 조사 보고서: `C:\Users\byulee\.claude\plans\kythe-glean-quiet-conway.md`
- 1차 소스 클론: `%TEMP%\kythe-src`, `%TEMP%\glean-src` (Kythe v0.0.75)
- CodeAtlas 근거 코드:
  - 하이브리드 라우팅: `indexer/src/indexing.rs:127-179`
  - USR 추출: `indexer/src/clang_parser.rs:563-591`
  - USR fast-path: `indexer/src/resolver.rs` (`resolve_calls` 초입)
  - CPU 제어: `indexer/src/main.rs:185`, `indexer/src/indexing.rs:275-311`
  - 증분/fanout: `indexer/src/incremental.rs`, `dev_docs/Milestone16_*.md`
