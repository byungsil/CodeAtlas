# [Specification] Hybrid C++ AST & Multi-Language Incremental Indexer for MCP

## 1. 개요 (Context & Objective)

본 프로젝트는 30만 개 이상의 파일(대규모 C++ 솔루션 및 기타 스크립트 언어)을 빌드 없이 정적 분석하여 SQLite DB에 기호 관계 그래프를 구축하는 시스템이다.
기존 **Tree-sitter 기반 C++ 파싱의 컨텍스트 부재(매크로, 헤더 미해석) 문제를 해결**하기 위해, C++ 엔진만 **Clang AST 기반**으로 전환하고 백그라운드 증분 인덱싱(Incremental Indexing) 효율을 극대화하는 것을 목표로 한다.

---

## 2. 시스템 아키텍처 개요

시스템은 최초 설치 단계에서 `compile_commands.json`을 생성하고 이를 기반으로 전체 인덱싱(Full Index)을 수행한다. 이후 MCP 로드 시 백그라운드에서 변경된 파일 및 의존성이 깨진 파일만 추적하여 증분 인덱싱을 수행한다.

              [ 소스 스캔 및 타임스탬프 비교 ]
                              │
      ┌───────────────────────┴───────────────────────┐
[변경 없음 (99.9%)] [변경 발생] │ │ (Skip) ┌───────────────┴───────────────┐ [C/C++ (.cpp, .h)] [기타 언어] │ │ (Clang AST 파서 엔진) (기존 Tree-sitter) │ ┌─────────────────┴─────────────────┐ [.cpp 변경] [.h 변경] │ │ (해당 파일만 파싱) (DB에서 의존 관계 역추적) │ │ └─────────────────┬─────────────────┘ ▼ [ SQLite 원자적 대치 (WAL) ]

---

## 3. 데이터베이스 스키마 확장 스펙

증분 인덱싱 및 헤더 역추적을 위해 기존 SQLite DB에 다음 2개 테이블을 필수로 생성 및 관리해야 한다.

```sql
-- 1. 파일 메타데이터 관리 (증분 판단용)
CREATE TABLE IF NOT EXISTS file_metadata (
    file_path TEXT PRIMARY KEY,
    last_modified INTEGER, -- OS 파일 최종 수정 시간 (Timestamp)
    file_hash TEXT,        -- 파일 내용의 MD5/SHA256 해시 (선택적 검증용)
    language TEXT          -- 'cpp', 'lua', 'python' 등
);

-- 2. C++ 헤더 의존성 맵 (역추적용)
CREATE TABLE IF NOT EXISTS file_dependencies (
    source_file TEXT,      -- .cpp 파일 경로
    included_header TEXT,  -- 해당 cpp가 include하는 .h 파일 경로
    PRIMARY KEY (source_file, included_header)
);
CREATE INDEX IF NOT EXISTS idx_included_header ON file_dependencies(included_header);
4. 에이전트 구현 작업 지침 (Implementation Tasks)
Task 0: 최초 설치 및 전처리 단계 (Setup & Pre-processing)
설명: 최초 전체 인덱싱을 수행하기 직전, Visual Studio 솔루션(.sln) 및 프로젝트 파일(.vcxproj)을 분석하여 Clang 표준 포맷인 compile_commands.json을 자동으로 생성해야 한다.
요구사항:
최초 설치 단계에서 VS 솔루션 파서 CLI 도구(VS-Compilation-Database CLI 또는 Clang Power Tools CLI)를 트리거하여 compile_commands.json을 프로젝트 루트에 빌드할 것.
Rust 인덱서 진입 시 compile_commands.json 존재 여부를 검증하는 가드 절(Guard Clause)을 두고, 파일이 없다면 명확한 에러 로그와 함께 프로세스를 종료(Exit Code 1)할 것.
솔루션 구조 변경 대응을 위해 .sln 파일의 타임스탬프를 감시하고, 변경 시 이 단계를 재수행할 것.
Task 1: 전처리 단계 - compile_commands.json 파싱
설명: Clang 파서 구동을 위해 생성된 컴파일 데이터베이스를 로드해야 한다.
요구사항:
프로젝트 루트의 compile_commands.json을 읽어 각 .cpp 파일별 컴파일 인자(Include 경로 -I, 프리프로세서 매크로 -D)를 매핑하는 룩업 테이블(Map)을 메모리에 빌드할 것.
Task 2: Rust 파서 엔진 이원화 (Dual-Engine Routing)
설명: 파일 확장자에 따라 정적 분석 엔진을 라우팅한다.
요구사항:
.lua, .py, .ts, .rs 등: 기존 tree-sitter 및 tree-sitter-graph 로직을 그대로 유지.
.cpp, .cxx, .cc, .h: clang-sys 또는 clang 크레이트를 사용하여 Clang AST 파서로 진입.
Task 3: Clang AST 기반 기호(Symbol) 추출 로직 구현
설명: C++ 파일 파싱 시 컴파일 컨텍스트를 주입하고 매크로가 확장된 완전한 AST를 순회한다.
요구사항:
libclang 파싱 세션 생성 시 Task 1에서 얻은 해당 파일의 컴파일 아규먼트를 바인딩할 것.
AST Cursor를 순회하며 다음 기호 데이터를 추출하여 기존 SQLite 기호 테이블 스키마에 맞게 포맷팅할 것:
CursorKind::ClassDecl, CursorKind::StructDecl ➡️ 클래스/구조체 정의
CursorKind::Namespace ➡️ 네임스페이스 스코프 추적
CursorKind::CxxMethod, CursorKind::FunctionDecl ➡️ 메서드/함수 선언 및 정의
CursorKind::CallExpr ➡️ 함수 호출 관계 (Caller-Callee 매핑)
외부 시스템 헤더(예: C:\Program Files\..., MSVC 표준 라이브러리) 영역의 커서 스캔은 성능을 위해 반드시 Skip할 것.
Task 4: 의존성 역추적 기반의 증분 인덱싱 알고리즘 구현
설명: MCP 로드 시 백그라운드에서 자원을 최소화하며 변경 사항을 반영하는 핵심 루프이다.
요구사항:
파일 스캔: 워크스페이스 내 전체 파일을 순회하며 OS 수정 시간을 file_metadata 테이블과 비교한다. 변경된 파일 목록을 추출한다.
헤더 역추적 (C++ 핵심): 변경된 파일 목록 중 .h (헤더) 파일이 존재할 경우, 즉시 아래 쿼리를 실행하여 영향을 받는 cpp 목록을 확보하고 파싱 대기열에 병합한다.
SELECT source_file FROM file_dependencies WHERE included_header = ?;
병렬 제어: MCP 백그라운드 구동 시에는 시스템 자원 독점을 막기 위해 쓰레드 풀(Thread Pool)의 코어 수를 최소한(예: 전체 코어의 25% 이하 또는 단일 쓰레드)으로 제한하여 구동할 것.
Task 5: 원자적 DB 갱신 (Transaction)
설명: 분석 완료된 파일의 데이터를 SQLite에 반영할 때 데이터 무결성을 보장하고 MCP 조회 쿼리와의 충돌을 방지한다.
요구사항:
파일 1개 분석 완료 시마다 독립된 트랜잭션(BEGIN TRANSACTION)을 열고 처리할 것.
정리(Clean): DELETE FROM cxx_symbols WHERE file_path = ?; 및 DELETE FROM file_dependencies WHERE source_file = ?; 실행.
삽입(Insert): Clang/Tree-sitter가 새로 추출한 기호 및 의존성 데이터를 적재.
메타 갱신: file_metadata에 해당 파일의 최신 타임스탬프 기록 후 COMMIT.
성능 튜닝: 대량의 읽기/쓰기 동시성 확보를 위해 SQLite 연결 시 PRAGMA journal_mode=WAL; 및 PRAGMA synchronous=NORMAL;을 활성화할 것.
5. 인수 조건 (Acceptance Criteria)
정확성: 동일한 이름을 가졌으나 네임스페이스나 클래스가 다른 C++ 메서드들을 SQL로 조회했을 때 오진 없이 정확히 분리되어 반환되어야 한다.
증분 성능: 파일 1~2개 수정 후 MCP 재로드 시, 백그라운드 인덱싱이 전체 30만 개를 다시 훑지 않고 변경된 파일과 연관 헤더 기반 파일만 타겟팅하여 수 초 내로 종료되어야 한다.
안정성: 백그라운드 인덱싱이 돌고 있는 와중에도 에이전트가 MCP 툴을 통해 SQL 쿼리(SELECT)를 날렸을 때 Database Locked 에러가 발생하지 않아야 한다.
