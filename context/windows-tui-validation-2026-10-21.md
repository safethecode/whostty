# Windows TUI 검증 — 2026-10-20/21

## 환경과 재현

Windows 11 26100, RTX 3080 Ti, 단일 1920×1080 모니터·100% 배율에서 main `cb2defe`의 Windows ZIP을 사용했다.
MSVC 도구 체인이 없는 PC이므로 Rust 빌드·테스트는 GitHub Actions에서 수행한다.
임시 설정과 160행 fixture만 사용하며 사용자 Vim/Neovim 설정은 읽지 않았다.

- 공식 native Neovim v0.12.6 (`-u NONE -i NONE -n`)과 Vim v9.2.1202 (`-u NONE -i NONE -n -N`).
- ASCII, precomposed é, CJK 한글/界 화면은 정상. Neovim에서 분해된 `e`+U+0301이 들어간 행은 첫 화면에서 한 셀씩 밀렸다.
- 새 탭에서도 재현했으며 Ctrl+L 전체 재그리기는 현상을 가렸다. 접근성 그리드도 어긋나므로 GPU shaping 이전의 문제다.
- 동일 Whostty exe(SHA256 `9de7934970c910657041de114484d1eb6cadebe84e74bf641cd978df6c78da32`) 옆에 Microsoft ConPTY 1.24.261001001의 DLL/호스트 쌍만 추가하면 첫 화면이 정상이다. 로드된 모듈의 실제 경로도 확인했다.
- 기본 OS conhost.exe 버전은 10.0.26100.8875. Microsoft는 legacy combining width와 신형 호스트의 차이를 문서화한다.

## 수정

portable-pty 0.9.0의 기존 DLL 로더를 이용하므로 Rust 파서나 입력 처리를 변경하지 않는다.
공식 NuGet archive 및 x64 DLL/호스트의 SHA256을 고정했다. 두 native 파일은 Microsoft Authenticode Valid이며
외부 VCRUNTIME/MSVCP 의존성은 없고 Windows UCRT/API set을 사용한다. ZIP에는 같은 릴리스의 Microsoft MIT 및 전체 upstream NOTICE를 포함한다.
네이티브 AMD64에서 검증했으며 ARM64·에뮬레이션은 검증하지 않았다.

## 실제 조작

- 신형 런타임의 Neovim 첫 화면은 결합 문자 행·번호·상태줄이 정렬된다.
- Neovim 삽입 모드에서 OS 한/영 전환과 `gksrmf`의 OS SendInput VK 입력으로 `한글`을 조합하고 space로 확정했다.
- 저장 후 UTF-8 파일은 원래 160행에 `한글 ` 한 행만 추가된 161행이며, 기존 분해 결합 문자도 보존된다.
- Vim의 한글·CJK·결합 문자가 섞인 첫 화면과 IME 조합·확정·저장도 확인했다. 저장 파일은 같은 한글 한 행만 추가되었고 기존 160행의 코드포인트는 모두 일치한다. 마우스 클릭으로 커서 이동, 휠로 46행 부근 이동, 최대화에 따른 117×26 → 202×38 그리드 변경과 VimResized 로그 및 정렬된 화면도 확인했다.
- 증거는 git-ignored `dist/validation/tui-2026-10-20/`에 둔다. 스크린샷은 해당 fixture가 실제 foreground일 때만 증거로 사용한다.

## 배포본 자동 검사

소스 `2c172e5`의 [Windows 배포 CI](https://github.com/safethecode/whostty/actions/runs/39446506377) ZIP을 다시 받아
체크섬·x64 PE·정적 CRT·모든 고지를 검증했다. 이 ZIP의 exe SHA256은
`ac4f7a47fc3fca503d80a4108bd497fcac9a766d1c4d52c030aeb564c07c5906`이다.
배포본에서 새 창과 새 탭 모두 Neovim 첫 화면의 연속 24행을 접근성 그리드 원문과 정확히 대조했다.
로드된 DLL은 배포 폴더의 `conpty.dll`이었다.

같은 ZIP에서 RTX 3080 Ti Vulkan·DX12 각각 100/125/150/200%의 native PTY UTF-8 왕복·shaped glyph·GPU readback
8회를 통과했다. 실제 monitor native scale은 모두 1.0이었다. 이는 렌더링 배율 override 검사다.
패키지 CI는 추가로 `-RequireBundledConpty`를 사용해 실제 child module 경로를 확인한다. DLL이 없으면 실행 전에,
파일만 있고 로더가 OS ConPTY로 fallback하면 검사 후 실패한다. 잘못된 DLL을 둔 실제 실행으로 fallback 거부도 확인했다.
누락 host·손상 DLL·누락 고지 ZIP은 outer checksum을 다시 계산해도 정적 검증에서 거부된다.

Python 전체 12개 테스트와 actual archive staging을 통과했다. 리뷰에서 Python3.9 미지원 `Path.write_text(newline=...)`를
발견해 `write_bytes`로 수정했다. [Rust CI](https://github.com/safethecode/whostty/actions/runs/39446506130)의
Windows/macOS fmt·Clippy·테스트도 통과했다. 최종 wrapper 및 merge 이후의 상태는 [PR #25](https://github.com/safethecode/whostty/pull/25)에 기록한다.

## 신형 런타임 지속 출력

main cb2defe exe와 신형 ConPTY 쌍을 사용하는 일반 GUI를 초당 600줄로 600초 실행했다.
fixture 360,000줄·source UTF-8/ANSI payload 44,640,694 bytes·일반/대체 화면 전환 59회,
전체 관측 601.281초·602표본, 자식 exit0·앱 생존·completed=true였다.
5초 진행 정지 0건, 최대 관측 진행 간격은 1.016초다. Native 편집·진단 검사를 같은 PC에서 병행했으므로 부하를 격리하지 않았다.

warmup30초 제외 후 앞/뒤 60초 중앙값의 private bytes는 499.715 → 499.879 MiB(+0.164 MiB)였다.
핸들 중앙값788·최대796·종료791, 스레드 중앙값38·최대44, CPU 중앙값20.3125%(논리 코어 하나=100%)다.
진행·프로세스 자원은 대리 지표이며 10분 관측으로 누수 없음·수시간 안정성·매 프레임 정확성을 결론 내리지 않는다.

## 설치·검증 한계

WSL은 설치돼 있지 않아 `wsl --status`가 exit50을 반환했다. 사용자는 설치를 허용했고 재부팅은 금지했다.
공식 WSL 3.0.3 MSI의 해시와 Microsoft 서명을 확인하고 `/norestart REBOOT=ReallySuppress`, DISM `/NoRestart`,
Ubuntu `--no-launch`로만 진행하는 스크립트를 준비했다. 관리자 승인 요청이 OS의 '사용자가 작업을 취소했습니다'로
종료되어 설치는 아직 시작되지 않았다. 승인 재표시 답변을 기다린다. 재부팅은 실행하지 않는다.

다중 모니터·실제 DPI 전환, WSL 내부 TUI, 하드웨어 화면 발광 지연, 수시간·수일 안정성은 남아 있다.
`--check-scale`의 100/125/150/200% readback은 렌더링 배율 검증이며 물리 모니터 DPI 전환을 대신하지 않는다.

## 원문 근거

- [NuGet 1.24.261001001](https://www.nuget.org/packages/Microsoft.Windows.Console.ConPTY/1.24.261001001)
- [동일 릴리스 v1.24.12741.0](https://github.com/microsoft/terminal/releases/tag/v1.24.12741.0)
- [Microsoft combining 문자 폭 변경](https://learn.microsoft.com/en-us/windows/console/console-potential-breaking-changes)
- [portable-pty 0.9.0의 ConPTY 로더](https://docs.rs/crate/portable-pty/0.9.0/source/src/win/psuedocon.rs)

## CI 검사 fixture 경쟁 제거

`79ae067` Windows CI의 기존 `tui_drag_captures_secondary_button_and_release_outside_the_pane`이 링크 열기 결과
`[]` 대 `https://example.com/`로 실패했다. 같은 Rust 소스의 `2c172e5`는 통과했다. 테스트는 직접 VT 상태를
주입하는 동시에 live ConPTY reader가 같은 그리드를 바꾸도록 만들어 resize 재그리기와 경쟁할 수 있었다.
정확히 덮어쓴 native VT 시퀀스는 수집하지 못했으므로 실패의 전체 인과를 확정하지 않는다.

Session-specific resize/terminal 획득을 작은 wrapper에 두고 실제 view 로직은 private generic 함수로 분리했다.
마우스·링크·휠 검사는 독립 `Terminal<VoidListener>`를 사용해 native writer 경쟁을 제거한다.
생산 경로는 같은 위치에서 한 번 resize한 후 lock하며 오류 전달을 보존한다. 기존 전체 gesture 검사를
매번 새 Context·그리드로 20회 반복한다. 변경 전 실패는 위 CI에, 변경 후 Windows/macOS 전체 검사는 PR 리뷰에 기록한다.
