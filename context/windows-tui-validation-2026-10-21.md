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
- Neovim 삽입 모드에서 OS 한/영 전환과 `gksrmf` 물리 VK 입력으로 `한글`을 조합하고 space로 확정했다.
- 저장 후 UTF-8 파일은 원래 160행에 `한글 ` 한 행만 추가된 161행이며, 기존 분해 결합 문자도 보존된다.
- Vim의 한글·CJK·결합 문자가 섞인 첫 화면과 IME 조합·확정도 확인했다. 추가 조작 결과는 검증 종료 때 기록한다.
- 증거는 git-ignored `dist/validation/tui-2026-10-20/`에 둔다. 스크린샷은 해당 fixture가 실제 foreground일 때만 증거로 사용한다.

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
