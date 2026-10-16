# Whostty 작업 맥락

- Windows용 독립 네이티브 터미널. Electron이나 웹뷰를 사용하지 않는다.
- Ghostty 수준의 완성도를 장기 목표로 삼지만, 초기 버전이 동등한 기능·성능을 달성했다고 주장하지 않는다.
- Ghostty 코드를 가져오는 포크가 아니다. Rust, Alacritty 터미널 에뮬레이터, portable-pty의 Windows ConPTY, wgpu GPU 렌더러를 사용한다.
- 현재 개발 장비는 macOS. **실제 Windows PC 실행 검증은 사용자가 추후 진행한다.** Mac의 테스트 통과를 Windows 동작 보장으로 표현하지 않는다.
- 커밋·푸시하고 작업을 PR 단위로 나눈다. 사용자가 리뷰 후 머지까지 명시적으로 요청했다. PR을 의존 순서로 리뷰하고 CI 통과 후 main에 병합한다.
- 빈 원격 저장소의 PR 기준점으로 main에 파일 없는 초기 커밋 하나를 만들었다. 제품 코드는 전부 PR에 포함한다.
- 문서는 이 `context/` 폴더에만 커밋한다. 별도 README, 설계서 등은 만들지 않는다.
- 라이선스 방향은 MIT. Ghostty 명칭·로고를 사용하지 않는다. 의존 라이브러리 고지는 배포 산출물에 포함한다.

## PR 분할

1. `feat/terminal-core`: 설정, VT 에뮬레이션, 키 인코딩, PTY 수명주기 및 통합 테스트.
2. `feat/desktop-terminal`: GPU 데스크톱 UI, 탭, 프로필, 터미널 입력·선택·스크롤.
3. `build/windows-distribution`: Windows 실행 파일 패키징, DPI/메타데이터, 이벤트 큐 보강, Windows 인계 맥락.

## 제품 경계

초기 범위는 실제 셸 실행, ANSI/truecolor, 대체 화면, 스크롤백, 탭, PowerShell/WSL 프로필,
복사·붙여넣기, 크기 변경, 설정 파일이다. 분할 창, SSH 관리, GPU 성능 벤치마크,
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
- 최종 로컬 검증: 테스트 12개, Clippy 경고 없음, 라이선스 고지 생성 성공. PR #1은 리뷰 기록 후 main에 병합했다. PR #2/#3은 Windows CI 확인 후 순서대로 병합한다.
