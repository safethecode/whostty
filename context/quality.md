# 제품 품질 목표와 검증

사용자는 Ghostty 수준이 될 때까지 개발을 계속하고, 모든 변경을 PR로 나눠 커밋·푸시·리뷰·머지하도록 요청했다.
완료 여부는 기능 개수가 아니라 아래 근거로 판단한다. 문서는 context/에만 둔다.
현재 개발 장비는 macOS이며 Windows 데스크톱 조작은 추후 사용자 PC에서 검증한다.

참고: [Ghostty 기능](https://ghostty.org/docs/features), [설정](https://ghostty.org/docs/config/reference),
[xterm 제어 시퀀스](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).
Ghostty의 주요 방향인 네이티브 UX, 탭·분할, 글자 처리, 다양한 터미널 앱 호환성을 비교 기준으로 삼는다.
제품 간 성능 동등성을 측정했다고 표현하지 않는다.

| 영역 | 현재 근거 | 남은 검증 또는 구현 |
|---|---|---|
| 셸·세션 | Windows ConPTY / Mac PTY 통합 테스트, 독립 패널, 비동기 종료 | Windows 장시간 실행·대량 출력·WSL 프로세스 트리 종료 |
| VT·TUI | Alacritty 엔진, ANSI/대체 화면/스크롤백, 키·마우스 회귀 테스트 | 실제 vim/neovim/less/tmux/PowerShell 편집과 앱별 회귀 자료 |
| 입력 | 기본 키, bracketed paste, IME 조합 보류, AltGr 문자 전달 | Windows 한국어 IME·AltGr 실기기, 고급 키보드 프로토콜 |
| 렌더링 | wgpu, 셀 스냅샷, 스타일 face·밑줄 장식·NFC, 폰트 fallback | ligature/복합 shaping, emoji, GPU 측정 |
| 검색 | 백그라운드 취소·revision 검사·결합 부호/soft wrap 매칭 | 긴 단일 논리 행의 256 KiB 제한, Windows 실사용 |
| 사용성 | 탭·중첩 분할·프로필·안전한 설정 저장·실행 옵션·웹 링크·테마·로컬 CWD 계승·키 바인딩 | 명령 탐색, WSL/SSH 경로 계승 |
| 배포 | Windows ZIP·체크섬·고지·정적 CRT·네이티브 PTY/GPU readback 진단 | Windows 설치/업데이트·서명·ARM64, 실기기 설치/재실행 |
| 접근성 | Terminal/TextRun·화면 텍스트·문자 위치·커서·일반 선택·숨김 마스킹 | Narrator/VoiceOver 실사용, 자동 읽기·선택 수정·블록 선택 |

Windows 실기기와 입력 지연·프레임 시간·메모리의 동일 조건 비교가 끝나기 전에는
Ghostty 수준의 완성도를 달성했다고 결론 내릴 수 없다. 현재 통과한 자동 검증과 미검증 항목은 구분해서 보고한다.

## 다음 개선 순서

1. 셀별 글자 배치를 run 단위 shaping과 glyph cache로 개선한다. ligature·결합 부호·emoji·DPI·커서·선택의 좌표 일관성을 함께 검증해야 한다.
2. 고급 키보드 프로토콜에 필요한 원래 키·수정키·누름/해제 정보를 보존한다. 백엔드가 잃는 정보를 확인하기 전에는 프로토콜 지원을 광고하지 않는다.
3. 셸 명령 경계·명령 탐색, WSL/SSH의 작업 디렉터리 계승, Bash/Zsh 연동을 확장한다.
4. Windows PC에서 IME/AltGr·다중 배율·Narrator·실제 TUI·장시간 출력/종료를 검증하고 재현 자료를 남긴다.
5. 동일 장비에서 입력 지연·GPU 프레임·메모리와 설치/업데이트 경로를 측정한다. 자동 테스트 통과와 Ghostty 동등성 판단을 분리한다.

PR #7–#17에서 VT 입력·검색·글꼴 스타일·실행 옵션·링크·접근성·설정 저장·테마·실행 진단·PowerShell CWD·키 바인딩을 개선했다.
Windows 배포 ZIP은 실제 ConPTY/PowerShell 출력과 wgpu 화면 readback을 검사한다. CI의 Microsoft Basic Render Driver/DX12/CPU 결과는
소프트웨어 렌더링 검증이며 하드웨어 성능이나 Windows 실기기 조작 검증을 대신하지 않는다.

Windows 네이티브 진단을 처음 실행하면서 eframe의 기본 기능을 끈 빌드에 DirectX 백엔드가 빠져 Vulkan 드라이버가 없는 Windows에서 시작하지 못하는 문제를 발견했다. Windows 대상에 DirectX 12를 명시적으로 포함하며(Vulkan도 wgpu 기본 Windows 대상으로 포함), 단순 빌드 성공을 실행 성공으로 간주하지 않는다.
