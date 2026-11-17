# Rusidian 개발 문서

- 셸: Tauri 2 (Rust 백엔드 + OS WebView)
- 프론트엔드: SvelteKit (Svelte 5, SPA 모드)
- 원칙: **원본은 항상 vault 안의 마크다운 파일**. 인덱스/캐시는 부가물일 뿐.

## 실행

```bash
npm install
npm run tauri dev
```

> `tauri dev`가 돌아가는 중에 `npm run build`를 실행하면 `build/`가 바뀌면서 앱 창이 새로고침됩니다.

## 설치 파일 만들기 (릴리스 빌드)

```bash
npm run tauri build
```

`target/release/bundle/` 아래에 두 가지 설치 파일이 생깁니다.

| 파일 | 설명 |
|------|------|
| `nsis/Rusidian_<버전>_x64-setup.exe` | 권장. 관리자 권한 없이 현재 사용자에게 설치(한국어 설치 화면) |
| `msi/Rusidian_<버전>_x64_ko-KR.msi` | 회사 PC 배포처럼 MSI가 필요할 때 |

설치 없이 실행만 하려면 `target/release/rusidian.exe`. WebView2가 없는 PC(구형 Windows 10)에서는
설치 프로그램이 WebView2를 내려받아 설치합니다.

- 버전은 `src-tauri/tauri.conf.json`과 `src-tauri/Cargo.toml`, `package.json`의 `version`을 함께 올립니다
  (`Cargo.lock`, `package-lock.json`은 `cargo check`, `npm install`을 한 번 돌리면 따라 바뀝니다).
- 업데이트 파일에 서명하므로 로컬 빌드에도 서명 키가 필요합니다. 없으면 설치 파일까지 만든 뒤 서명 단계에서 멈춥니다.
  PowerShell: `$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\rusidian-updater.key"; $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""`
- 앱 아이콘은 `app-icon.svg`가 원본입니다. 고친 뒤 `npx tauri icon app-icon.svg`로 `src-tauri/icons`를 다시 만듭니다
  (데스크톱만 쓰므로 생성되는 `android/`, `ios/` 폴더는 지워도 됩니다).
- 설치 파일에 코드 서명은 하지 않았습니다. 처음 실행할 때 Windows SmartScreen이 "알 수 없는 게시자" 경고를 띄울 수 있습니다.
- 설정·탭 구성·알림 기록은 사용자 폴더(`%APPDATA%\com.rusidian.app`, `%LOCALAPPDATA%\com.rusidian.app`)에 저장되고,
  vault 관련 설정(예산, 그래프 색 그룹 등)은 vault의 `.obsidian/` 안에 저장됩니다.

## 새 버전 내기 (자동 업데이트)

설치한 앱은 켤 때 GitHub 릴리스의 `latest.json`을 보고 새 버전을 알려 줍니다(**설치하고 다시 시작**,
또는 설정 → 일반 → **업데이트 확인**). 설치 전에 모든 창의 노트를 저장하고, 설치 파일의 서명이
`tauri.conf.json`의 `pubkey`와 맞을 때만 설치합니다.

1. 위 세 파일의 `version`을 올리고(예: `0.3.0`) 커밋합니다.
2. 같은 버전의 태그를 올립니다: `git tag v0.3.0`, `git push origin main v0.3.0`
3. GitHub Actions의 **Release**(`.github/workflows/release.yml`)가 설치 파일을 빌드·서명해 릴리스에 올립니다.
   태그와 `tauri.conf.json`의 버전이 다르면 멈춥니다.

서명 비밀키는 `%USERPROFILE%\.tauri\rusidian-updater.key`(비밀번호 없음)에 있고, 저장소 Secrets의
`TAURI_SIGNING_PRIVATE_KEY`에도 들어 있습니다. **이 키를 잃어버리면 이미 설치된 앱은 더 이상 업데이트를
받지 못하므로** 따로 백업해 둡니다.

## 구조

```
Cargo.toml   Cargo 워크스페이스 (core + src-tauri, 빌드 결과는 /target)
core/        rusidian-core: UI와 무관한 순수 Rust 로직 (Tauri 의존성 없음)
  src/vault.rs        vault 파일 트리, 경로 안전성 검사, 원자적 저장
  src/frontmatter.rs  YAML frontmatter(속성) 파서 — Obsidian 규칙, 키 순서 보존
  src/bookmarks.rs    Obsidian 북마크(.obsidian/bookmarks.json) 읽기·추가·빼기·이름 바뀜 따라가기
  src/mentions.rs     링크 없는 언급 찾기(링크·코드 밖), 언급을 [[링크]]로 바꾸기
  src/links.rs        [[위키링크]]·마크다운 링크 추출 (코드 블록 제외)
  src/index.rs        메모리 인덱스: 링크 해석(Obsidian 규칙), 백링크, 부분 문자열 검색
  src/watch.rs        vault 폴더 감시(notify) → 인덱스 갱신 + 변경 알림
  src/tasks.rs        할일 파서 + 완료/반복/편집/삭제 (Tasks 플러그인 이모지·Dataview 형식, 시간, 알림)
  src/recur.rs        반복 규칙 해석 + 다음 날짜 계산
  src/templates.rs    템플릿 설정·변수
  src/note_settings.rs 템플릿·데일리 노트 설정 읽기·저장, 미리 보기(오늘의 노트 경로, 없는 폴더·템플릿)
  src/moment.rs       moment.js 날짜·시간 형식 (한국어 이름, 일요일 시작 주·ISO 주, Obsidian과 같은 토큰 나누기)
  src/finance_import.rs 거래 내역 가져오기 (인코딩·구분자·머리글·열 추측, 입출구분·취소 상태, 중복 확인)
  src/phone.rs        휴대폰 알림 파일 읽기, 알림 → 기록 (금액·방향·가게·분류 추측, 중복 확인)
  src/statement.rs    거래 내역 파일 읽기: CSV, 엑셀 xlsx·xls(calamine), 암호 걸린 엑셀(msoffice-crypto), 농협 PDF(pdf-extract)
  src/recent.rs       최근 vault 목록 (같은 폴더 판별, 최대 20개)
  src/calendars.rs    외부 캘린더 구독 목록과 주소 검사
  src/ics.rs          iCalendar 파서 (RRULE·EXDATE·RECURRENCE-ID 펼치기, 시간대는 벽시계 시각+TZID로 넘김)
  src/sync.rs         동기화 설정, 허용 목록 → Git 제외 규칙, 올라갈 파일 미리 보기, 충돌 사본 이름
  src/merge.rs        충돌 사본 이름 → 원래 파일, 같은 부분·다른 부분으로 나누기
  src/gitsync.rs      Git 동기화 실행 (vault 밖 Git 디렉터리, 커밋·받기·합치기·충돌 사본·올리기)
  src/obsidian_config.rs .obsidian/*.json의 키 하나만 읽고 쓰기·지우기 (나머지 키 보존)
  src/hotkeys.rs      바꾼 단축키 (.obsidian/hotkeys.json, Obsidian 형식)
  src/daily.rs        데일리 노트 경로·템플릿 (.obsidian/daily-notes.json)
  src/calendar.rs     캘린더 범위 데이터 (날짜별 데일리 노트 + 범위 안 할일 + 데일리 노트 시간 계획), 알림 목록
  src/attachments.rs  붙여넣은·끌어다 놓은 파일: 첨부 폴더(Obsidian 설정), 안전한 이름, 삽입할 링크
  src/finance.rs      재무 기록 파서 + 줄 생성 (💸/💰 형식, 금액은 ×100 정수, 마이너스 = 환불·차감, 들여쓴 줄 = 정산)
  src/assets.rs       계좌 잔액 기록(🏦) 파서·줄 생성, 저축으로 볼 분류
  src/fixed.rs        고정지출 목록, 그 달 결제 찾기, 매달 나간 결제로 후보 추천(숨긴 후보 제외), 결제일 알림
  src/graph.rs        링크 그래프 (노트/첨부/미해결 노드, 중복 제거된 간선, 연결 수)
  src/date.rs         최소 날짜 계산 (외부 라이브러리 없음)
  src/tags.rs         태그 추출 (#본문 태그 + tags 속성, 하위 태그 /)
  src/fileops.rs      이름 검사, 이름 바꾸기/옮기기 + 링크 다시 쓰기
  src/history.rs      파일 복구: 노트 이전 버전 보관(간격·보관 기간·개수 제한, 옮김 따라가기), 줄 단위 비교
  src/markdown.rs     코드 블록 건너뛰기 등 공용 도우미
src-tauri/   Tauri 셸
  src/lib.rs    Tauri 명령 (open_vault, refresh_tree, read_note, write_note, file_times, create_note,
                last_vault, parse_frontmatter, search_vault, backlinks, resolve_link,
                list_tasks, toggle_task, add_task, calendar_range, open_daily_note,
                finance_range, add_finance, link_graph, list_tags,
                new_note, new_canvas, new_folder, rename_path, move_path, trash_path, reveal_path,
                set_properties, update_task, delete_task, task_reminders,
                list_templates, render_template, note_settings, set_note_settings,
                preview_note_settings, update_finance, delete_finance,
                get_budgets, set_budgets, export_finance_csv, graph_groups, set_graph_groups,
                list_calendars, set_calendars, external_events, recent_vaults, forget_vault,
                import_csv_preview, import_csv_plan, import_csv_commit,
                add_settlement, fixed_expenses, set_fixed_expenses, fixed_reminders, set_fixed_remind, ignore_fixed, save_attachment,
                balances, add_balance, get_savings, set_savings, get_hotkeys, set_hotkey,
                get_bookmarks, toggle_bookmark, remove_bookmark, outgoing_links, unlinked_mentions, link_mention,
                startup_vault, focus_vault_window, open_window,
                get_sync, set_sync, sync_scan, sync_now, list_conflicts, conflict_blocks)
                창마다 vault 하나 (AppState.vaults: 창 이름 → vault), 변경 알림은 그 창에만
  src/subscriptions.rs 외부 캘린더 구독 (ureq로 받기, 메모리·디스크 캐시)
                + "vault-changed" 이벤트
                + vault:// 프로토콜 (열린 vault의 파일을 읽기 전용으로 제공, 숨김 폴더 제외)
  src/phone_inbox.rs   휴대폰 알림 파일 확인·기록 명령 (처리한 알림은 앱 설정 폴더에 기억)
  src/file_history.rs  저장·삭제·옮기기 전에 이전 버전 남기기, 파일 복구 명령 (앱 데이터 폴더 history/)
src/
  lib/api.ts             Rust 명령 타입 래퍼
  lib/FileTree.svelte    재귀 파일 트리
  lib/Editor.svelte      CodeMirror 6 마크다운 에디터 ([[위키링크]] Ctrl+클릭, #태그 강조)
  lib/NoteHeader.svelte  노트 위 제목(이름 바꾸기)과 만든 날·고친 날
  lib/Properties.svelte  속성 패널 (값 종류별 편집 → set_properties로 frontmatter 다시 쓰기)
  lib/CanvasView.svelte  캔버스 보기·편집 (lib/canvas.ts: JSON Canvas 읽기·쓰기, 색, 화살표 곡선)
  lib/Reading.svelte     읽기 보기 (lib/render.ts: marked + DOMPurify, Obsidian 문법 확장, 수식·각주)
  lib/PrintView.svelte   인쇄·PDF용 노트 (화면 밖에서 밝은 색으로 그림, 인쇄할 때 이것만 보임)
  lib/mathMermaid.ts     KaTeX 수식·Mermaid 다이어그램 (필요할 때만 불러옴)
  lib/livePreview.ts     라이브 미리보기 (CodeMirror 장식으로 커서 밖 줄의 기호 숨김, 수식·다이어그램 블록)
  lib/Search.svelte      검색 패널
  lib/Backlinks.svelte   백링크 패널 (+ 링크 없는 언급)
  lib/OutgoingLinks.svelte 나가는 링크 패널
  lib/BookmarksPane.svelte 북마크 탭 (그룹·파일·제목·검색·폴더)
  lib/textStats.ts       단어·글자 수 (상태 줄)
  lib/Outline.svelte     개요 패널 (lib/outline.ts: 제목 파싱, [[#제목]] 찾기)
  lib/TagPane.svelte     태그 패널 (하위 태그 트리, 이름순/노트 수순)
  lib/Palette.svelte     빠른 전환·명령 팔레트 공용 모달 (lib/fuzzy.ts: 퍼지 매칭)
  lib/SettingsDialog.svelte 설정 (lib/settings.svelte.ts: localStorage에 저장), 일반·템플릿·데일리 노트·단축키 탭
  lib/NoteSettings.svelte 템플릿·데일리 노트 설정 탭 (Obsidian 설정 파일에 저장)
  lib/HotkeySettings.svelte 단축키 바꾸기 목록 (lib/hotkeys.svelte.ts: 키 이름·비교·표시, 바꾼 키 불러오기·저장)
  lib/TaskView.svelte    할일 보기 (그룹, 필터, 완료, 빠른 추가)
  lib/CalendarView.svelte 캘린더 보기 (월·주·일, 시간표, 날짜 상세)
  lib/TaskEditor.svelte  할일 편집 대화상자
  lib/workspace.svelte.ts 창·탭 구성 (분할, vault별 저장/복원)
  lib/docs.svelte.ts     열린 노트 (파일당 하나: 내용·자동 저장·속성), 여러 탭이 공유
  lib/LeafView.svelte    탭 하나의 내용 (편집기/읽기/미디어/보기)
  lib/reminders.ts       알림 확인 (할일 30초마다, 놓친 알림 10분까지; 고정지출 15분마다)
  lib/externalEvents.ts  외부 일정을 현지 시각으로 (Intl로 시간대 변환, 일광 절약 시간 포함)
  lib/CalendarsDialog.svelte 외부 캘린더 구독 관리
  lib/SyncDialog.svelte  Git 동기화 설정 (원격, 자동 간격, 올릴 폴더 고르기, 올라갈 파일 미리 보기)
  lib/FileRecovery.svelte 파일 복구 (이전 버전 목록, 바뀐 부분·전체 보기, 되돌리기·되살리기)
  lib/ConflictDialog.svelte 동기화 충돌 합치기 (다른 곳마다 원래·사본·둘 다, 결과 고치기)
  lib/FinanceView.svelte 재무 보기 (요약, 분류별 막대, 6개월 추이 차트, 날짜별 거래 내역, 빠른 기록, 가계부/고정지출 탭)
  lib/TxDetail.svelte    거래 상세 (결제·정산·내 몫, 정산 추가) — lib/txGroups.ts: 결제와 정산 묶기, 내용·계좌 나누기
  lib/AssetsView.svelte  자산 탭 (계좌별 잔액, 합계, 잔액 기록, 저축 분류 설정)
  lib/FixedView.svelte   고정지출 탭 (이번 달 상태, 후보) — lib/FixedEditor.svelte: 추가·편집
  lib/GraphView.svelte   그래프 보기 (Canvas + d3-force, 그래프 화면을 열 때만 로드)
  lib/TaskRow.svelte     할일 한 줄 (할일·캘린더 공용)
  lib/taskToggle.svelte.ts 할일 체크 흐름 (저장 먼저 → 토글 → 새로고침)
  lib/dates.ts           현지 날짜 도우미
  routes/+page.svelte    메인 화면 (사이드바 + 편집기 + 백링크, 자동 저장, 변경 반영)
```

### 검색 방식

`tantivy` 같은 단어 단위 색인 대신 메모리 부분 문자열 검색을 씁니다. 단어 단위 색인은
한국어에서 "회의"로 "회의록을"을 못 찾기 때문입니다(Obsidian 검색도 부분 문자열 방식).
노트 수천 개 규모에서 수 ms이고, 텍스트 사본을 따로 만들지 않습니다. 영문 대소문자는 구분하지 않습니다.

## 테스트

```bash
cargo test -p rusidian-core
```

로직은 `core`에 두고 거기서 테스트합니다. Tauri에 링크된 테스트 바이너리는 Windows에서
GUI 매니페스트가 없어 `STATUS_ENTRYPOINT_NOT_FOUND`로 실행되지 않기 때문입니다.

프론트엔드 로직(퍼지 검색, 탭 구성, 자동 저장, 남은 돈 계산)은 vitest로, 타입은 svelte-check로 확인합니다.

```bash
npm test
npm run check
```

GitHub Actions의 **CI**(`.github/workflows/ci.yml`)가 push와 PR마다 위 검사와 clippy, 프론트엔드 빌드를 돌립니다.

## Windows 빌드 참고

이 환경은 `x86_64-pc-windows-gnu`(MinGW) 툴체인을 씁니다. 그래서 `src-tauri/Cargo.toml`의
`crate-type`에서 모바일 전용 `cdylib`를 뺐습니다(GNU 링커의 DLL export 65535개 한도 초과).
링크 시 나오는 `.rsrc merge failure: multiple non-default manifests` 경고는 실행에 영향이 없습니다.

## 로드맵

- [x] Phase 0 — vault 열기, 파일 트리, 노트 읽기/쓰기 (자동 저장)
- [x] Phase 1 — CodeMirror 6 에디터, frontmatter 파싱
- [x] Phase 2 — 파일 감시(`notify`), 검색(부분 문자열), `[[링크]]` 이동/백링크
- [x] Phase 3 — 할일 (Tasks 플러그인 문법 호환, 완료·반복·빠른 추가)
- [x] Phase 4 — 일정 (월간 캘린더, 날짜별 할일, 데일리 노트 열기/만들기)
- [x] Git 동기화 — 설정·플러그인은 항상, 노트 폴더는 고른 것만, 충돌 사본, 자동 동기화
- [x] 충돌 합치기 — 충돌 사본과 원래 파일을 나란히 비교, 부분마다 골라 합치기
- [x] vault별 창 — 창마다 vault, 새 창(Ctrl+Shift+N), 이미 열린 vault는 그 창으로
- [x] 거래 내역 가져오기 — 은행·카드 CSV·엑셀(암호 포함)·농협 PDF, 열 맞추기, 입출구분·취소 줄, 중복 건너뛰기
- [x] 휴대폰 알림 자동 기록 — MacroDroid + Syncthing 알림 파일, 30초마다 읽어 가계부에 `#자동` 기록
- [x] 여러 vault — 최근 vault 목록, 빠른 전환, vault별 탭·최근 파일
- [x] 외부 캘린더 — ICS/webcal 구독, 반복·시간대 처리, 오프라인 캐시
- [x] 북마크·링크 패널 — Obsidian 북마크 공유, 나가는 링크, 링크 없는 언급(→ 링크로), 단어·글자 수
- [x] 자산·저축 — 계좌 잔액 기록(🏦)과 자산 탭, 저축 분류를 지출과 따로 집계
- [x] 수식·다이어그램·각주 — KaTeX, Mermaid(읽기 보기·라이브 미리보기), 각주
- [x] 이미지 붙여넣기 — 클립보드 이미지·끌어다 놓은 파일을 Obsidian 첨부 설정대로 저장하고 링크 삽입
- [x] 가계부 정리 — 환불·차감(마이너스 금액), 정산(들여쓴 줄)과 거래 상세, 날짜별 거래 내역, 고정지출
- [x] 고정지출 알림 — 결제일 전·후 기록이 없으면 Windows 알림, 후보 숨기기
- [x] 재무·그래프 확장 — 기록 편집/삭제, 월 예산, CSV 내보내기, 그래프 색 그룹(Obsidian 설정 호환)
- [x] 탭·분할·템플릿 — 탭, 좌우 분할(같은 노트 동기화), 탭 구성 기억, 템플릿 삽입
- [x] 할일·일정 확장 — 할일 편집/삭제, 반복 규칙 확장, Dataview 형식, 시간·알림, 주간/일간 시간표
- [x] Phase 5 — 재무 (💸/💰 기록 파싱, 월별·분류별 집계, 6개월 추이, 빠른 기록)
- [x] Phase 6 — 그래프 뷰 (전체/로컬, 필터, 대형 vault 대응)
- [x] 파일 관리 — 새 노트/폴더, 이름 바꾸기, 끌어서 옮기기(링크 자동 수정), 휴지통 삭제
- [x] 파일 복구 — 저장·삭제 전 이전 버전을 컴퓨터에만 보관, 바뀐 부분 비교, 되돌리기·되살리기
- [x] 보기 — 읽기 보기, 라이브 미리보기, 임베드(이미지·노트·PDF·미디어), 속성 편집
- [x] 캔버스 — `.canvas` 보기·편집 (카드·노트·그룹·화살표, Obsidian 형식 그대로)
- [x] PDF로 내보내기 / 인쇄 — 밝은 색, 노트 이름 제목, 수식·다이어그램이 그려진 뒤 인쇄 창
- [x] 탐색 — 빠른 전환, 명령 팔레트, 개요·태그 패널, 검색 연산자, `[[노트#제목]]` 이동, 설정(테마·글자 크기)
- [x] 단축키 바꾸기 — Obsidian `hotkeys.json` 호환(같은 명령 이름), 겹침 표시, 한글 입력 상태에서도 동작
