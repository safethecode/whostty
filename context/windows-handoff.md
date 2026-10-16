# Windows PC 인계

## 현재 상태

Whostty는 Ghostty 소스 포크가 아닌 독립 Rust 터미널이다. Windows에서 portable-pty가 ConPTY를 열고,
Alacritty가 VT 상태를 처리하며, eframe/wgpu가 네이티브 창에 그린다. Electron·브라우저 런타임은 없다.

개발은 macOS에서 진행했다. 실제 셸 PTY 테스트와 Mac 네이티브 창 테스트를 수행했고,
Windows GitHub Actions에서 컴파일·PTY 통합 테스트를 수행한다. **Windows 데스크톱에서의 실제 조작은 아직 검증하지 않았다.**
Ghostty와의 성능·호환성 동등성은 검증하지 않았다.

## 실행 파일 받기

GitHub Actions의 **Windows package** 워크플로에서 main의 성공한 실행을 열고
`whostty-windows-x64` 아티팩트를 다운로드한다. 아티팩트 안의 `whostty-windows-x64.zip`을 다시 풀고
`whostty.exe`를 실행한다. ZIP에는 Whostty MIT 라이선스와 의존성 라이선스 고지가 동봉된다.
코드 서명과 자동 업데이트는 아직 없다. 대상은 Windows x64이며 ARM64 패키지는 만들지 않는다.
아티팩트 보존 기간은 14일이다. 만료되면 워크플로의 Run workflow로 main에서 새로 빌드한다.
Windows package는 ZIP의 실행 파일을 꺼내 `--check`로 네이티브 PTY·화면 GPU readback을 검사하며
결과 JSON도 아티팩트에 포함한다. CI 가상 장비의 검사를 실제 PC의 IME·DPI·WSL·입력 지연 검증으로 해석하지 않는다.

GitHub CLI를 사용하는 경우:

```powershell
gh run list --repo safethecode/Whostty --workflow windows-package.yml --branch main
gh run download <성공한-run-id> --repo safethecode/Whostty --name whostty-windows-x64
Get-FileHash .\whostty-windows-x64.zip -Algorithm SHA256
```

## 소스에서 실행

필요한 도구: Git, Rust MSVC 도구 체인, Visual Studio Build Tools의 **Desktop development with C++** 및 Windows SDK.
저장소의 `rust-toolchain.toml`이 Rust 1.88.0을 선택한다.

```powershell
git clone https://github.com/safethecode/Whostty.git
cd Whostty
git switch main
cargo run --locked --package whostty
```

특정 디렉터리·프로필로 시작하거나 프로그램을 직접 실행할 수 있다:

```powershell
.\whostty.exe --working-directory 'C:\dev' --profile 'PowerShell 7'
.\whostty.exe --config 'C:\dev\whostty.toml' -e pwsh.exe -NoLogo
.\whostty.exe --help
.\whostty.exe --check
```

`-e` 또는 `--` 뒤의 인수는 Whostty 옵션으로 해석하지 않고 프로그램에 그대로 전달한다.
셸 문자열 평가를 자동으로 추가하지 않는다. 실행 옵션은 첫 탭에만 적용되며 설정 파일의 프로필을 바꾸지 않는다.
도움말·버전·잘못된 옵션은 GUI를 만들지 않는다. Windows의 기존 콘솔과 출력 리다이렉션을 사용하고,
콘솔 없이 직접 실행한 경우에는 메시지 창을 표시한다.

`--check`는 사용자 설정을 읽거나 저장하지 않고 고정된 일회용 셸을 실행한다. 정상 종료와 표시된 색상 픽셀을
실제 GPU 화면 readback으로 확인한 뒤 JSON을 출력하고 닫는다. 실패하면 종료 코드가 0이 아니다.
다른 실행 오버라이드와 함께 사용할 수 없다. 기본 프로필·사용자 글꼴·IME나 전체 GUI 기능을 검사하는 모드는 아니다.

CI와 같은 휴대용 ZIP을 만들려면 PowerShell 7에서 실행한다:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo install cargo-about --version 0.9.2 --locked --features cli
./scripts/build-windows.ps1
```

산출물은 `dist/whostty-windows-x64.zip` 및 SHA256 파일이다. 라이선스 고지 생성이 실패하면 패키징도 실패한다.
배포 스크립트는 VC++ 런타임을 정적으로 링크한다. CI는 생성된 PE 파일의 import table을 검사해 VC++ 재배포 패키지 의존성이 없는지 확인한다.
빌드/테스트만 할 때는 cargo-about이나 PowerShell 7이 필요하지 않다.

## 프로필 및 설정

기본 설정: `%APPDATA%\Whostty\config.toml`. 첫 실행에 기본값을 생성한다.
`WHOSTTY_CONFIG` 환경변수로 다른 파일을 지정할 수 있다. 메뉴 `...`에서 설정 파일 열기·재로드·저장이 가능하다.
잘못된 설정은 덮어쓰지 않고 오류를 표시하며 기본값으로 실행한다. 설정 재로드는 기존 셸을 종료하지 않는다.

기본 프로필: Windows PowerShell (`powershell.exe`), PowerShell 7 (`pwsh.exe`), WSL (`wsl.exe --cd ~`), cmd.
PowerShell 7과 WSL은 해당 프로그램이 설치돼 있어야 한다. 기본 프로필은 PowerShell 5이며, 시작 디렉터리는 홈이다.
프로필은 실행 파일과 인수 배열을 직접 전달한다. 셸 문자열을 자동으로 평가하지 않는다.

```toml
font_size = 15.0
theme = "dark" # or "light"
scrollback = 10000
default_profile = 0
inherit_working_directory = true
# font_path = 'C:\Windows\Fonts\consola.ttf'
# font_bold_path = 'C:\Windows\Fonts\consolab.ttf'
# font_italic_path = 'C:\Windows\Fonts\consolai.ttf'
# font_bold_italic_path = 'C:\Windows\Fonts\consolaz.ttf'

[[profiles]]
name = "PowerShell"
program = "powershell.exe"
args = ["-NoLogo"]
shell_integration = true
# working_directory = 'C:\dev'

[[profiles]]
name = "Ubuntu"
program = "wsl.exe"
args = ["-d", "Ubuntu", "--cd", "~"]
```

새 기본 PowerShell 프로필은 `shell_integration = true`다. 기존 설정에서는 기본값이 false이므로 위 값을 직접 추가한 후 재로드하면 새 셸부터 적용한다.
기존 PowerShell 프로필·프롬프트를 먼저 로드한 뒤 자식 셸 안에서 경로 보고만 감싼다. 프로필 파일·실행 정책을 수정하지 않는다.
`-NoLogo`, `-NoProfile`, `-NoExit`만 있는 대화형 PowerShell 프로필에서 지원한다. `-e` 직접 실행에서는 자동으로 끈다.

셸이 OSC 7 또는 OSC 9;9로 알린 로컬 디렉터리는 상태 표시줄에 나타난다. `+`/Ctrl+Shift+T와 분할은 같은 프로그램·인수의
활성 셸 디렉터리를 계승한다. 새 탭 프로필에 명시한 `working_directory`와 메뉴에서 직접 선택한 프로필은 그 설정을 사용한다.
Preferences에서 계승을 끌 수 있다. 보고가 없으면 시작 디렉터리를 사용하며, 없어진 디렉터리는 계승하지 않는다.
원격 host의 file URI·네트워크/장치 경로·제어 문자는 계승하지 않는다. WSL/SSH 경로 변환과 Bash/Zsh 자동 주입은 아직 제공하지 않는다.
WSL 기본 프로필은 Linux 홈에서 시작한다.

기본 글꼴은 Cascadia Mono 또는 Consolas, 한글 대체 글꼴은 맑은 고딕을 로드한다.
일반/굵게/기울임/굵은 기울임은 같은 글꼴 가족의 face를 선택하며 없으면 합성 스타일을 쓴다.
사용자 스타일 파일은 해당 스타일의 같은 monospace 가족이어야 한다. 개별 파일은 최대 64 MiB다.
글꼴 크기와 프로필의 저장은 Preferences의 Save preferences로 명시적으로 수행한다.

Preferences의 Terminal colors에서 Dark/Light를 선택하면 즉시 미리 보고 Save preferences로 저장한다.
사용자 색상은 위 `theme = ...` 줄 대신 별도의 테이블로 설정한다. 생략한 색상은 기본값을 사용한다.

```toml
[theme]
background = "#101620"
foreground = "#dce4ee"
cursor = "#9ee8b2"
selection = "#2a4a54"
search = "#6b4f2a"
# ansi = [정확히 16개의 "#RRGGBB" 색상]
```

색상은 #RRGGBB 형식이다. ANSI 0–15는 테마를 따르며 16–255와 truecolor도 지원한다.
앱이 OSC로 바꾼 색은 우선 적용되고 OSC 리셋 시 선택된 테마 색으로 돌아간다.

## Windows 실기기 검증

1. PowerShell에서 `Get-Location`, `Get-ChildItem`, 한글 입력·삭제·복사·붙여넣기, Ctrl+C 프로세스 중단을 확인한다.
2. Ctrl+Shift+T, Ctrl+Tab/Shift+Ctrl+Tab으로 탭 생성·이동을 확인한다. Ctrl+Shift+D/E로 중첩 분할하고 Ctrl+Alt+방향키로 포커스를 이동한다. Ctrl+Shift+W는 활성 패널을 닫고, 탭의 닫기 버튼은 탭 전체를 닫는다. 닫기 취소와 패널 구분선 드래그·더블 클릭을 확인한다.
3. Ctrl+Shift+C/V 또는 Ctrl+V로 복사·붙여넣기를 확인한다. 선택이 없을 때 Ctrl+C는 셸에 중단 문자를 보낸다.
4. 터미널 안에서 `& C:\path\to\Whostty\scripts\windows-smoke.ps1`을 실행해 색·한글·대체 화면·스크롤백을 확인한다.
5. 창을 계속 리사이즈하고 100/150/200% 배율 모니터 사이로 옮겨 그리드와 글꼴이 맞는지 확인한다.
6. PowerShell 7, cmd, WSL 각각에서 실행·입력·크기 변경·종료를 확인한다. 설치하지 않은 프로필은 명확한 오류를 표시해야 한다.
7. WSL에서 vim/less/top 같은 대체 화면 프로그램, 마우스 보고, bracketed paste를 확인한다. Shift+드래그는 TUI 마우스 모드에서도 선택한다.
8. 한국어 IME 후보창 위치와 조합 확정 시 중복 입력 여부, AltGr 키보드 레이아웃을 확인한다.
9. `exit` 후 종료 코드와 Restart session 버튼을 확인한다. 모든 탭을 닫은 뒤 새 터미널을 다시 열 수 있어야 한다.
10. 대량 출력을 실행한 패널을 닫는 동안 다른 패널에 입력하고 창을 이동한다. UI가 자식 종료·ConPTY 정리를 기다리며 멈추지 않아야 한다. 세션 오류의 Restart session은 실행 중인 프로세스를 끝내기 전에 확인창을 표시해야 한다.
11. 동기화 출력(2026) 종료 마커가 없는 앱도 약 150ms 기한 뒤 화면을 갱신해야 한다. TUI에서 좌/중/우 버튼 드래그 후 패널 밖에서 놓고, 창 포커스를 바꿨을 때 마우스가 눌린 상태로 남지 않는지 확인한다.
12. 웹 URL 또는 OSC 8 링크 위에서 Ctrl을 누르면 실제 대상과 밑줄을 표시한다. Ctrl+클릭은 기본 브라우저로 열고 드래그·일반 클릭은 링크를 열지 않는다. file/임의 프로토콜 링크는 열지 않는다. TUI의 마우스 모드에서 Ctrl+클릭이 앱에 중복 전달되지 않는지 확인한다.
13. Narrator에서 터미널 영역을 찾아 화면 텍스트·한글·결합 문자·커서/선택 위치를 읽는지 확인한다. 숨김(SGR 8) 텍스트는 접근성 값에도 나타나지 않아야 한다. 접근성 포커스로 다른 패널을 선택한 뒤 키 입력이 그 셸로 가는지 확인한다.
14. PowerShell에서 한글·공백·따옴표가 있는 디렉터리로 이동한 뒤 상태 표시줄, 새 탭과 분할의 `Get-Location`을 확인한다. 기존 프롬프트의 오류 표시와 `$LASTEXITCODE`, 파일 시스템 외 provider, 계승 끄기, 명시 시작 경로를 확인한다.
15. Dark/Light/사용자 색상을 재로드·저장한 뒤 재실행한다. 분할 패널·선택·검색·커서·OSC 색 변경/리셋의 대비를 확인한다.

문제 보고에는 Windows 빌드 번호, GPU/드라이버, 화면 배율, 사용 프로필, 재현 명령, 현상·예상 결과를 기록한다.
설정 파일이나 화면을 공유할 때 토큰·개인 경로 등은 제거한다.

## 개발 검증 및 남은 범위

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

현재 구현은 SSH 관리, 세션 복구, 이미지 프로토콜, ligature/복합 문자 shaping,
완전한 접근성, 자동 업데이트를 제공하지 않는다. 이중·물결·점선·파선 및 밑줄 색상을 지원한다.
셀 내 문자열은 NFC로 정규화해 합성 가능한 결합 문자를 표시하지만 ligature/복합 shaping은 아직 지원하지 않는다.
ANSI·대체 화면 엔진 지원과 각 TUI의 전체 호환성은 구분한다. macOS의 파서·검색·셀 복사 기준값은
`context/performance.md`에 기록했다. Windows 실기기의 입력 지연·GPU 프레임 시간은 아직 측정하지 않았다.

분할 창은 탭당 최대 8개를 지원한다. Ctrl+F로 활성 패널의 스크롤백을 검색하고 F3/Shift+F3 또는
Enter/Shift+Enter로 결과를 이동한다. 대소문자 구분과 정규식을 선택할 수 있고 Escape로 검색을 닫는다.
새 출력·리사이즈로 결과 좌표가 달라지면 Enter로 다시 검색한다. 검색은 취소 가능한 백그라운드 작업이며
결합 부호 문자열도 매칭한다. 한 soft-wrapped 논리 행이 256 KiB를 넘으면 명시적인 검색 제한 오류를 표시한다.
서로 다른 hard newline을 가로지르는 다중 행 검색은 지원하지 않는다.
