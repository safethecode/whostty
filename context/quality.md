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
| 사용성 | 탭·중첩 분할·프로필·설정·실행 옵션·웹 링크 | 명령 탐색, 현재 작업 디렉터리 계승, 키 바인딩·테마 |
| 배포 | Windows ZIP·체크섬·고지·정적 CRT·CI | Windows 설치/업데이트·서명·ARM64, 실기기 설치/재실행 |
| 접근성 | 기본 UI AccessKit | 터미널 텍스트·선택·커서의 스크린리더 검증 |

Windows 실기기와 입력 지연·프레임 시간·메모리의 동일 조건 비교가 끝나기 전에는
Ghostty 수준의 완성도를 달성했다고 결론 내릴 수 없다. 현재 통과한 자동 검증과 미검증 항목은 구분해서 보고한다.

## 다음 개선 순서

1. PR #7 완료: 동기화 출력 타임아웃/EOF 복구, 모드별 Enter, TUI 마우스 캡처·포커스 보고.
2. PR #8 완료: 백그라운드 대용량 검색과 오래된 결과 제거.
3. 글꼴 스타일·장식과 렌더링을 검증하고, 이후 shaping 기반을 재검토한다.
4. 일상 사용 기능과 Windows 실행·패키지 품질을 지속적으로 개선한다.
