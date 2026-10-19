# Whostty 작업 맥락

- Windows용 독립 네이티브 터미널. Electron이나 웹뷰를 사용하지 않는다.
- Ghostty 수준의 완성도를 장기 목표로 삼지만, 초기 버전이 동등한 기능·성능을 달성했다고 주장하지 않는다.
- Ghostty 코드를 가져오는 포크가 아니다. Rust, Alacritty 터미널 에뮬레이터, portable-pty의 Windows ConPTY, wgpu GPU 렌더러를 사용한다.
- 초기 개발은 macOS에서 진행했다. 2026-10-20부터 Windows 11 / RTX 3080 Ti에서도 배포본의 네이티브 실행 진단을 수행한다. 자동 진단 통과를 IME·WSL·TUI의 실제 조작 검증으로 표현하지 않는다.
- 커밋·푸시하고 작업을 PR 단위로 나눈다. 사용자가 리뷰 후 머지까지 명시적으로 요청했다. PR을 의존 순서로 리뷰하고 CI 통과 후 main에 병합한다.
- 빈 원격 저장소의 PR 기준점으로 main에 파일 없는 초기 커밋 하나를 만들었다. 제품 코드는 전부 PR에 포함한다.
- 문서는 이 `context/` 폴더에만 커밋한다. 별도 README, 설계서 등은 만들지 않는다.
- 라이선스 방향은 MIT. Ghostty 명칭·로고를 사용하지 않는다. 의존 라이브러리 고지는 배포 산출물에 포함한다.

## PR 분할

1. `feat/terminal-core`: 설정, VT 에뮬레이션, 키 인코딩, PTY 수명주기 및 통합 테스트.
2. `feat/desktop-terminal`: GPU 데스크톱 UI, 탭, 프로필, 터미널 입력·선택·스크롤.
3. `build/windows-distribution`: Windows 실행 파일 패키징, DPI/메타데이터, 이벤트 큐 보강, Windows 인계 맥락.

## 제품 경계

현재 범위는 실제 셸 실행, ANSI/truecolor, 대체 화면, 스크롤백, 탭, PowerShell/WSL 프로필,
복사·붙여넣기, 크기 변경, 설정 파일, 분할 창과 검색이다. SSH 관리, GPU 성능 벤치마크,
자동 업데이트, 코드 서명, Ghostty 호환성은 별도 후속 작업이다.

## 검증 현황

- PR #1: macOS 로컬 테스트 8개 및 Clippy 통과. GitHub Actions의 Windows/macOS 테스트도 통과.
- 데스크톱: Rust 네이티브 창 + eframe/wgpu. Windows에서 DX12, macOS에서 Metal을 사용할 수 있다.
- ANSI 16/256색·truecolor, 굵은 글꼴, 밑줄·취소선, wide/combining 문자, 커서 모양을 그린다.
- 탭, 프로필 선택, 설정 재로드, 복사/붙여넣기, 마우스 선택, 스크롤백 및 TUI 마우스 보고를 제공한다.
- 사용자 설정 기본 경로는 Windows `%APPDATA%\Whostty\config.toml`. `WHOSTTY_CONFIG` 환경변수로 재정의할 수 있다.
- 잘못된 설정은 원본을 유지하고 오류를 표시한다. 시작 디렉터리는 기본적으로 홈 디렉터리다.
- Windows 기본 프로필은 PowerShell 5, PowerShell 7, WSL, cmd. 추가 설치가 필요한 프로필은 선택 시 시작 오류를 표시한다.
- OSC52 클립보드 접근은 비활성화했다. 키보드 복사·붙여넣기는 지원한다.
- Windows IME·AltGr·DPI·GPU 드라이버와 PowerShell/WSL/TUI의 실제 UX는 Windows PC 검증이 필요하다.
- macOS 실제 네이티브 창에서 ANSI 색상, 셸 명령 입력/출력, Ctrl+Shift+T 새 탭, 환경설정, 실행 중 세션 닫기 확인창을 수동 확인했다. 임시 테스트 설정/앱 번들은 저장소 밖에 두었다.

## 코드 리뷰

- 그리드의 Tab/방향키/Escape 포커스 이탈을 막았다.
- wide 문자를 그린 뒤 spacer 배경이 덮는 문제를 배경·글자 순서 분리로 수정했다.
- 셀마다 서로 다른 클립을 만들어 GPU 배칭을 깨던 경로를 제거했다.
- 터미널 제목·제어 이벤트 큐를 1024개로 제한하고, 큐가 가득 차도 파서가 그리드 잠금 중 대기하지 않도록 했다. 남은 이벤트는 다음 프레임에서 계속 처리한다.
- 잘못된 사용자 글꼴을 파싱 전에 검증한다. 설정 파싱 오류 때 원본 파일을 덮어쓰지 않는다.
- 포커스 없는 입력과 IME 조합 중 Enter가 셸에 전달되지 않는 것을 실제 PTY를 사용하는 회귀 테스트로 확인했다.
- ConPTY 리사이즈가 출력 파이프를 기다리는 동안 그리드 잠금을 잡지 않도록 수정하고, 출력 중 반복 리사이즈 테스트를 추가했다.
- 첫 Windows ZIP의 PE import table에서 VCRUNTIME140.dll 의존성을 발견했다. 배포 스크립트는 정적 CRT를 사용하며, 패키지 CI가 체크섬·고지·PE x64 GUI·DPI manifest·CRT 독립성을 검사한다.
- 초기 PR #1~#3 검증: 테스트 12개, Clippy 경고 없음, 라이선스 고지 생성 성공. 모두 Windows CI 확인과 리뷰 기록 후 main에 병합했다.

## 2026-10-17 후속 개발

PR #1~#3은 모두 main에 병합됐다. 사용자가 제품 완성도를 높이는 작업을 계속 요청했다.
기존 Rust/ConPTY/wgpu 구조와 테마를 유지하며 분할 창, 스크롤백 검색, 세션 수명주기를 순서대로 개선한다.

- 분할 창: 탭 안에서 독립된 셸을 오른쪽/아래로 중첩 분할한다. 구분선 드래그로 비율을 조절하고 더블 클릭하면 절반으로 복구한다.
- Ctrl+Shift+D/E로 오른쪽/아래 분할, Ctrl+Alt+방향키로 패널 포커스 이동, Ctrl+Shift+W로 활성 패널을 닫는다.
- 탭의 닫기 버튼은 탭 전체를 닫는다. 실행 중 세션은 확인창을 거친다. 패널당 입력·선택·스크롤 상태를 분리한다.
- 탭당 최대 8개 패널이며, 지나치게 작은 패널은 확대 후 분할하도록 안내한다. 새 패널은 활성 패널의 프로필·설정된 시작 디렉터리를 사용한다.
- 활성 패널의 스크롤백을 Ctrl+F로 검색한다. F3/Shift+F3, Enter/Shift+Enter로 다음/이전 결과를 찾고 Escape로 닫는다. 대소문자 구분과 정규식을 지원하며 결과가 끝나면 순환한다.
- 검색 결과 강조는 출력·리사이즈 revision에 묶는다. 출력이 바뀌면 오래된 위치를 표시하지 않고 Enter로 다시 찾도록 안내한다. 후속 검색 구현은 결합 부호를 포함한 문자열도 검색한다.
- 세션 종료는 프로세스를 소유한 백그라운드 작업에서 수행한다. GUI에서 ConPTY 닫기·자식 종료를 기다리지 않고, 재사용된 PID에 신호를 보내지 않는다. 이벤트 큐가 가득 차도 종료 상태를 별도 상태 셀에서 읽는다.
- Windows에서는 프로세스 핸들의 종료 신호를 먼저 확인한다. portable-pty 0.9가 실제 종료 코드 259를 STILL_ACTIVE로 오인할 수 있는 경로를 피하고, Windows 전용 회귀 테스트로 확인한다.
- 렌더러는 재사용 가능한 셀 버퍼로 스냅샷을 만든 뒤 터미널 잠금을 해제한다. 글자 배치와 GPU 도형 생성 중에도 PTY 파서가 진행할 수 있다.
- `cargo run --release --locked -p whostty-core --example terminal_bench`로 파서 처리량, 1만 줄 검색, 4800셀 복사 비용을 측정한다. GPU 프레임 시간·실기기 입력 지연·Ghostty 비교 수치는 별도 검증 대상이다.
- PR #4 분할 창과 #5 검색은 Windows/macOS CI·패키지 검증 후 리뷰 기록과 함께 main에 병합했다. #6은 세션 수명주기·렌더링 스냅샷·성능 기준값을 다룬다.
- 후속 테스트는 macOS 20개와 Windows 전용 1개로 구성한다. 로컬 20개는 통과했다. 구분선 드래그는 패널 위젯이 있는 egui 포인터 이벤트 회귀 테스트로 확인했다. macOS 실제 창에서는 패널별 셸 명령, 검색 강조, 한 패널 닫기 후 다른 셸 유지, 자연 종료와 재시작을 확인했다. 구분선의 실제 마우스 드래그와 Windows GUI 동작은 실기기 확인 항목으로 남긴다.

## 제품 품질 개선 지속

사용자가 Ghostty 수준까지 계속 개발하도록 요청했다. [품질 기준](quality.md)에 검증 근거와 남은 범위를 기록한다.
동기화 출력은 파서의 150ms 기한에 맞춰 UI 재갱신을 예약하고, 기한 초과·출력 EOF에 보류된 출력을 복구한다.
Enter는 터미널의 newline 모드를 따르고, UTF-8 마우스 좌표·우클릭 드래그·패널 밖 버튼 해제를 처리한다.
포커스 보고는 활성 패널/창의 실제 포커스 변화를 따르며 사용자의 선택과 스크롤 위치를 바꾸지 않는다.
PR #7은 macOS 24개/Windows 25개 테스트·Clippy·Windows 패키지 검증 후 자체 리뷰 기록과 함께 병합됐다.

검색은 별도 작업에서 실행한다. 16개 물리 행씩 잠깐 잠가 복사한 뒤 정규식 처리·문자 좌표 매핑은 잠금 밖에서 수행한다.
검색어 수정은 100ms 지연 후 시작하며 새 요청·검색 닫기·패널 제거는 이전 작업을 취소한다.
출력/리사이즈 revision이 달라진 작업은 결과를 적용하지 않는다. 검색은 종료된 패널의 터미널을 계속 소유하지 않는다.
결합 부호와 wide 문자의 셀 좌표, 16행 경계를 가로지르는 soft wrap을 테스트한다.
단일 soft-wrapped 논리 행은 256 KiB까지 검색하며, 넘으면 메모리를 무제한 늘리지 않고 오류를 표시한다.
PR #8은 macOS 28개/Windows 29개 테스트·Clippy·Windows 패키지 검증 후 자체 리뷰와 함께 병합됐다.

글꼴 스타일은 TTC face와 같은 가족의 monospace 파일을 선택한다. 사용자 bold/italic/bold-italic 경로를 지원하고
없는 스타일은 합성한다. macOS는 한글 fallback도 로드하며, 셀 내 결합 문자는 표시할 때 NFC로 정규화한다.
이중·물결·점선·파선 밑줄, 밑줄 색상, wide 문자 너비의 커서를 그린다.
Mac 실제 창에서 네 가지 글꼴 스타일·밑줄 종류·한글 fallback·결합 문자와 동기화 출력 복구를 확인했다.
임시 테스트 앱 번들과 설정은 /tmp에만 두었다. ligature/복합 shaping이나 Windows GUI 검증을 완료한 것은 아니다.

시작 옵션은 `--working-directory/-d`, `--profile/-p`, `--config`, `--command/-e`, `--`를 지원한다.
프로그램 인수의 공백·빈 문자열·셸 메타 문자를 보존하고 자동 평가하지 않는다.
첫 탭에만 적용하고 저장된 프로필은 바꾸지 않는다. 도움말/버전/옵션 오류는 GUI 초기화 전에 처리하며
Windows에서는 상속한 출력 파이프를 보존하고 필요할 때 호출자 콘솔에 붙인다.
실행 파일을 별도 프로세스로 띄우는 통합 테스트로 도움말·버전·오류 코드·stdout/stderr를 확인한다.

웹 링크는 OSC 8 메타데이터와 일반 http/https 문자열을 지원한다. soft wrap은 잇되 hard newline은 넘지 않으며
주변 문장부호를 제외한다. Ctrl hover로 실제 대상을 표시하고 Ctrl+클릭에서만 기본 브라우저에 전달한다.
링크 누름·해제는 TUI 입력과 선택에서 분리하고, 드래그·포커스 상실·해제 시 대상 변경은 열기를 취소한다.
file/임의 URL 프로토콜은 열지 않는다. 토큰 탐색과 URL 길이는 제한해 잠금 중 무한 탐색을 피한다.

PR #9 글꼴은 macOS/Windows 각각 31개 테스트·배포 검증 후 병합했다. 설정 예시의 글꼴 가족 불일치도 수정했다.
PR #10 실행 옵션은 macOS/Windows 각각 35개 테스트와 실제 CLI subprocess 검증·배포 검증 후 병합했다.

터미널 접근성 트리는 Terminal 역할·현재 화면 텍스트·물리 행 TextRun·문자 바이트 길이/위치/너비·커서와
일반 선택 범위를 제공한다. wide/결합 문자는 셀 좌표로 대응시키며 SGR 숨김 문자는 공백으로 가린다.
선택/커서로 필요한 공백을 보존하고 화면 밖 선택은 보이는 범위로 한정한다. 블록 선택은 이 선형 모델로 표현하지 않는다.
기본 접근성 Focus 요청은 활성 패널을 바꾼다. 자동으로 전체 출력을 읽는 live announcement와 접근성 API를 통한
선택 수정은 아직 구현하지 않는다. Mac AX 트리에서 실제 텍스트와 숨김 테스트 문자열 비노출을 확인했다.
Windows Narrator와 VoiceOver의 실제 읽기·탐색·확대기 동작을 완료했다고 주장하지 않는다.

설정 저장은 같은 디렉터리의 임시 파일을 쓰고 sync한 다음 교체한다. 읽기 전용 파일은 수정하지 않고
사용자 심볼릭 링크와 Unix 파일 모드를 보존한다. Windows는 교체 전 임시 파일에 기존 owner/group/DACL과 기본 속성을 적용하고 같은 볼륨에서 MoveFileExW로 교체한다.
ReplaceFileW의 백업-이름 변경 구간에 reader가 NotFound를 관찰한 CI 실패를 수정했다.
암호화/압축/sparse 속성을 임시 파일에 보존할 수 없는 경우 원본을 바꾸지 않고 저장을 거부한다.
전원 차단·모든 네트워크 파일시스템에서의 내구성을 보장하는 것은 아니다. 동시 reader가 완성된 TOML만 읽는지,
잘못된 설정/읽기 전용 저장 실패가 기존 내용을 보존하는지 테스트한다.

테마는 Dark/Light preset 또는 사용자 배경·전경·커서·선택·검색·16색 ANSI 팔레트를 지원한다.
렌더링과 터미널 앱 색상 질의가 같은 resolver를 사용한다. OSC override가 우선이며 리셋하면 현재 테마로 복귀한다.
잘못된 색상/팔레트 길이는 설정 파싱에서 거부한다. Preferences에서 미리 보기와 명시적 저장이 가능하다.
Mac 실제 창에서 라이트 테마와 분할 패널의 텍스트·장식·포커스·커서를 확인했다.

`--check`는 별도 CheckApp으로 실행되어 사용자 설정/프로필을 사용하지 않는다. 공유 터미널 view·기본 시스템 글꼴·
실제 PTY의 truecolor probe를 렌더링하고, eframe Screenshot의 GPU readback에서 해당 색상 픽셀을 검증한다.
PTY 종료 0과 GPU readback이 모두 맞아야 성공 JSON을 내보낸다. 내부 15초/외부 CI 60초 제한을 두고 자동 종료한다.
Windows package는 압축한 exe를 다시 꺼내 이 검사를 실행한다. 사용자 설정 환경변수도 별도의 임시 경로로 두고
설정 파일이 생기지 않았는지 검사한다. JSON에 backend/device type을 기록하므로 software adapter와 실제 하드웨어를 구분할 수 있다.
Mac 실제 실행 결과: Apple M3 Pro/Metal, 1600×1000 readback, probe 색상 3560픽셀, PTY·renderer 성공, 설정 미생성.
이 결과를 실제 입력 지연·성능 비교나 Windows 실기기 조작 검증으로 표현하지 않는다.

현재 작업 디렉터리는 bounded OSC 7/9;9 observer로 추적한다. 로컬 file URI만 허용하고 경로의 제어 문자·네트워크/장치 경로를 거부한다.
새 탭은 명시 시작 디렉터리가 없는 동일 실행 환경에서, 분할은 활성 패널에서 디렉터리를 계승한다. 기본 PowerShell 프로필의 자식 셸
프롬프트를 감싸 기존 프롬프트 출력을 먼저 얻은 뒤 OSC를 출력한다. 사용자 프로필 파일이나 실행 정책은 변경하지 않는다.
기존 설정은 shell_integration=false를 유지하며 opt-in할 수 있다. 직접 명령 실행은 연동을 끈다. WSL/SSH 경로 변환은 후속 작업이다.

Mac 실제 창에서도 `/tmp`로 이동해 OSC 7 보고 후 `+`로 만든 탭의 `pwd`가 `/private/tmp`인 것을 확인했다. 상태 표시줄도 tmp로 갱신됐다.

PR #15는 Windows 네이티브 실행 검증이 찾아낸 DirectX 12 백엔드 누락을 수정한 뒤 병합했다.
최종 ZIP의 진단 결과는 Windows x64 / Microsoft Basic Render Driver / Dx12 / Cpu, 800×500 화면에서 probe 1041픽셀,
PTY·renderer 성공(2125ms)이었다. 소프트웨어 어댑터 검증이며 실기기 GPU 성능 비교가 아니다.

키 바인딩은 `[[keybindings]]`의 key/action으로 기본 조합을 덮어쓰거나 unbind한다. 중복·알 수 없는 조합은 설정 오류로 처리한다.
modifier를 정확하게 비교하며, IME 조합 중에는 단축키를 가로채지 않는다. 새 탭·닫기·분할·붙여넣기는 키 반복을 무시한다.
단축키 뒤의 같은 이벤트 묶음에 든 Text가 셸로 새지 않는 것을 실제 PTY 테스트로 확인한다.

Mac 네이티브 앱에서 설정한 F6로 새 탭을 만들고, 실행 중 F7로 바꿔 Reload config를 누른 뒤 목록과 실제 새 탭 동작이 F7로 바뀌는 것을 확인했다.

TUI 휠 입력은 DEC 1007(Alternate Scroll)을 실제로 확인한다. 이를 끈 앱에는 휠로 화살표를 만들지 않으며,
켰을 때만 대체 화면에서 normal/application cursor 시퀀스를 보낸다. 소수점 휠 누적은 TUI mouse/대체 화면/Shift
대상이 바뀌거나 창 포커스를 잃으면 초기화해 이전 모드의 움직임이 다른 대상으로 새지 않게 한다.
근거: [xterm Wheel mice](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Wheel-mice).

IME 조합 중 창/터미널 포커스를 잃거나 Preferences·검색·닫기 창으로 입력 대상이 바뀌면 보류 중인 preedit를 해제한다.
IME Disabled 이벤트도 GUI 입력 차단 여부와 무관하게 정리해, 돌아온 뒤 이전 조합이 Enter·단축키를 계속 막지 않게 한다.
실제 Windows 한국어 IME 후보창·확정/취소 흐름의 조작 검증은 여전히 인계 항목이다.

사용자의 계속 개발 요청 이후 PR #7–#17은 Windows/macOS 검사와 리뷰 기록을 거쳐 main에 병합했다.
키 바인딩 최종 검증은 Mac 53개/Windows 55개 테스트와 배포 ZIP의 DX12/CPU readback을 통과했다.
후속 TUI 휠·IME 정리는 각각 별도 PR이며, 아직 shaping/고급 키보드 프로토콜이나 Windows 실기기 검증을 완료한 상태는 아니다.
남은 구현·검증 순서는 quality.md를 따른다.

PR #18(DEC 1007/휠 누적)과 #19(IME 포커스·순서)은 main에 병합했다. #19의 최종 검증은 Mac 54개/Windows 56개 테스트와 ZIP 실행 진단을 통과했다.

## OpenType 글자 렌더링

Rustybuzz로 LTR run을 shaping하고 ab_glyph로 OpenType glyph ID를 rasterize한 뒤 공유 GPU atlas에 그린다.
행·색/스타일·블록 커서 경계를 지키고, 글꼴과 문자군을 나눈다. ligature·mark 위치를 원래 VT 셀 범위에 맞춰 배치한다.
검색·복사·접근성은 원래 셀 데이터를 유지한다. 같은 glyph는 다시 업로드하지 않는다. font_ligatures 설정을 즉시 전환할 수 있다.
이전과 같은 em 크기 환산, 빈 advancing placeholder glyph(Fira Code의 contextual ligature), synthetic 스타일·CJK fallback을 처리한다.

레이아웃 캐시는 512개/추정 8 MiB, run은 8 KiB로 제한한다. atlas는 2048² RGBA, glyph metadata는 8192개까지다.
가득 찬 atlas는 다음 egui pass에서만 비워 현재 pass의 UV를 덮지 않는다. 지원하지 않는 RTL/bidi·bitmap/color/SVG glyph와
제한을 넘는 run은 기존 셀 렌더러로 표시한다. 전체 RTL/색상 emoji·복합 문자 호환성 또는 GPU 성능 동등성을 주장하지 않는다.

테스트용 TTF는 직접 만든 도형/GSUB/GPOS fixture이며 scripts/make-shaping-fixture.py(fontTools 4.54.1)로 재생성한다.
폰트 em과 ascender/descender 높이를 다르게 해서 크기 환산 회귀를 잡는다. Fira Code 6.2는 /tmp에만 받아 실제 화면 검증에 사용했고 배포하지 않는다.
Mac 실제 창에서 화살표/비교 연산자 ligature와 옵션 off 전환, 결합 부호·한글 fallback·스타일·밑줄을 확인했다.
--check는 shaped glyph가 실제로 그려진 횟수와 cache entry, 화면 배율도 보고하며 shaping 경로 미사용은 실패로 처리한다.
Mac Metal readback도 shaped text·PTY·화면 픽셀 성공을 확인했다. Windows는 동일한 패키지 진단을 CI에서 실행한다.

Shaped text의 CPU frame/mesh/tessellation 벤치마크는 ignored release 테스트로 분리했다.
80×40 고정 ASCII run, warmup 10회 후 200프레임을 측정하며 GPU 제출·입력 지연을 포함하지 않는 것을 JSON에 명시한다.
Mac 3회 기준값과 포함/제외 범위는 performance.md에 남긴다.

배포 감사에서 cargo-about HTML에 기본 내장 폰트의 일부 attribution이 누락된 것을 확인했다.
잠금된 epaint_default_fonts의 TTF name table(ID 0/13/14), 파일 SHA256, 함께 제공된 고지 원문을 추출해
EMBEDDED_FONT_LICENSES.txt를 ZIP에 포함한다. 별도 폰트 설치나 fontTools 런타임 의존성은 추가하지 않는다.
폰트 목록 변경·원문 누락·잘못된 메타데이터는 패키징을 실패시키고, Windows/macOS CI와 ZIP 검증이 이를 검사한다.
원문 출처: [Hack 고지](https://raw.githubusercontent.com/emilk/egui/0.31.1/crates/epaint_default_fonts/fonts/Hack-Regular.txt),
[Ubuntu Font Licence](https://raw.githubusercontent.com/emilk/egui/0.31.1/crates/epaint_default_fonts/fonts/UFL.txt).
