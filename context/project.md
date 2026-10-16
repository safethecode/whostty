# Whostty 작업 맥락

- Windows용 독립 네이티브 터미널. Electron이나 웹뷰를 사용하지 않는다.
- Ghostty 수준의 완성도를 장기 목표로 삼지만, 초기 버전이 동등한 기능·성능을 달성했다고 주장하지 않는다.
- Ghostty 코드를 가져오는 포크가 아니다. Rust, Alacritty 터미널 에뮬레이터, portable-pty의 Windows ConPTY, wgpu GPU 렌더러를 사용한다.
- 현재 개발 장비는 macOS. **실제 Windows PC 실행 검증은 사용자가 추후 진행한다.** Mac의 테스트 통과를 Windows 동작 보장으로 표현하지 않는다.
- 커밋·푸시하고 작업을 PR 단위로 나눈다. PR은 검토 가능한 의존 순서로 쌓고 임의로 병합하지 않는다.
- 빈 원격 저장소의 PR 기준점으로 main에 파일 없는 초기 커밋 하나를 만들었다. 제품 코드는 전부 PR에 포함한다.
- 문서는 이 `context/` 폴더에만 커밋한다. 별도 README, 설계서 등은 만들지 않는다.
- 라이선스 방향은 MIT. Ghostty 명칭·로고를 사용하지 않는다. 의존 라이브러리 고지는 배포 산출물에 포함한다.

## PR 분할

1. `feat/terminal-core`: 설정, VT 에뮬레이션, 키 인코딩, PTY 수명주기 및 통합 테스트.
2. `feat/desktop-terminal`: GPU 데스크톱 UI, 탭, 프로필, 터미널 입력·선택·스크롤.
3. `build/windows-distribution`: Windows CI, 실행 파일 패키징, Windows 인계 맥락.

## 제품 경계

초기 범위는 실제 셸 실행, ANSI/truecolor, 대체 화면, 스크롤백, 탭, PowerShell/WSL 프로필,
복사·붙여넣기, 크기 변경, 설정 파일이다. 분할 창, SSH 관리, GPU 성능 벤치마크,
자동 업데이트, 코드 서명, Ghostty 호환성은 별도 후속 작업이다.
