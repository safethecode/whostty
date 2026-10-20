# ConPTY 배포 수정 계획

> 실행: 현재 `codex/windows-tui-validation`에서 직접 구현하고 별도 리뷰를 받는다.

**Goal:** 결합 문자가 들어간 Neovim 첫 화면의 행 밀림을 Windows 배포본에서 해결한다.
**Architecture:** portable-pty의 기존 conpty.dll 로더를 사용한다. Microsoft NuGet 1.24.261001001의 x64 DLL과 OpenConsole.exe를 한 쌍으로 포함하고 다운로드·최종 ZIP을 해시로 검사한다.
**Tech Stack:** Python 표준 라이브러리, PowerShell 패키징, 기존 Rust/portable-pty.
**Spec:** `context/project.md`의 Windows 실기기 품질 검증. 같은 cb2defe 실행 파일에서 기본 ConPTY는 e+U+0301 행을 밀고 공식 DLL 쌍은 정상임을 네이티브 Neovim으로 확인했다.

## 제약 및 리뷰 초점

- 사용자 설정과 시스템 콘솔을 변경하지 않는다. WSL 설치에도 재부팅하지 않는다.
- 공식 HTTPS 배포를 고정 SHA256으로 확인한 뒤 필요한 두 파일만 꺼낸다.
- DLL·호스트의 버전 불일치, 잘못된 아키텍처, 외부 VC++ runtime 의존성, 고지 누락을 최종 ZIP 검사로 차단한다.
- native source 실행은 개발자가 런타임을 exe 옆에 배치해야 한다. OS fallback의 한계를 문서에 남긴다.
- 실제 편집·저장, 한글 IME, 재시작 후 첫 화면을 확인한다. 자동 검사와 실기기 범위를 구분한다.

## Task 1: 검증한 런타임 쌍 배포

- [x] `scripts/test_conpty_runtime.py`: checksum mismatch와 누락된 pair를 먼저 실패로 확인한다. 정상 archive 추출도 검사한다.
- [x] `scripts/prepare_conpty_runtime.py`: 고정 archive hash, 명시한 두 member만 추출, 고지 생성. `scripts/build-windows.ps1`에 연결한다.
- [x] `scripts/verify_windows_package.py`: pair hash, x64 PE, 정적 VC++ runtime, 고지를 검사한다. 손상·누락 ZIP을 거부하는 검증을 실행한다.
- [x] Python 전체 테스트와 패키징 helper를 실제 공식 archive로 실행한다. Windows/macOS CI와 최종 ZIP native GPU·PTY 진단을 확인한다.
- [x] `context/`에 근거·검증 한계를 기록하고 커밋·PR을 만든다.

통합 후 리뷰·머지·main CI·새 브랜치 결과는 [PR #25](https://github.com/safethecode/whostty/pull/25)에 기록한다.
