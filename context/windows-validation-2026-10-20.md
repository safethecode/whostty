# Windows 실기기 검증 — 2026-10-20

Windows 11 Pro 10.0.26100, NVIDIA GeForce RTX 3080 Ti, x86_64에서 수행했다.
실제 모니터 배율은 100%다. 200% 항목은 egui 렌더링 배율 오버라이드이며 모니터 이동 검사가 아니다.
일반 데스크톱의 CPU/GPU 부하는 격리하지 않았다. Ghostty와 비교하지 않았다.

## 실제 한국어 IME

Microsoft 한국어 IME에 VK_HANGUL과 영문 물리 키에 해당하는 가상 키를 SendInput으로 전달했다.
완성된 한글 붙여넣기는 사용하지 않았다. 화면의 조합 밑줄·한자 후보창과, PTY 안에서 입력을
문자열로만 기록하는 Python fixture의 UTF-8 코드포인트 로그를 함께 확인했다.
입력은 소유한 전용 창의 HWND/PID·foreground·수정키 상태를 확인한 뒤 전송하고,
포커스가 바뀌면 중단했다. 입력 문자열을 셸 명령으로 평가하지 않았다.

main `327e45b` 배포본에서 다음을 확인했다.

| 조작 | PTY가 받은 결과 |
|---|---|
| 한글 모드 `rks` 조합 중 | 화면에 `간` preedit, PTY에는 아직 미전달 |
| 위 상태에서 Enter | `간` U+AC04 한 줄, 중복 없음 |
| `rks`, Backspace, Enter | `가` U+AC00 |
| `rks`, Escape, Enter | 빈 줄, 취소한 조합 미전달 |
| `gks`, 한자 키, 후보 1, Enter | `韓` U+97D3 |
| `gks`, 한/영 전환, `x`, Enter | `한x`, 조합 확정 후 영문 전달 |

한자 후보창이 커서와 멀리 떨어진 터미널 패널 오른쪽 아래에 뜨는 문제를 발견했다.
egui-winit 0.31이 네이티브 IME exclusion area에 사용하는 `IMEOutput.rect`에 패널 전체를
전달하고 있었다. 이를 패널 안으로 제한한 커서 셀 크기로 바꿨다. 커서 이동과 패널 경계를
검사하는 테스트 및 실제 `view::show`의 출력 영역을 검사하는 회귀 테스트를 추가했다.

수정 제품 소스 `b178e40`의 새 ZIP에서 같은 조합·수정·취소·한자 확정·영문 전환을 다시 확인했다.
한자 후보창은 커서 옆으로 이동했다. 출력으로 스크롤한 화면 하단에서도, 영문 접두어 뒤로
열을 옮긴 커서 근처 위쪽에 열렸다. 후보창만 Escape로 닫은 뒤 Enter를 누르면 `xxxxx한`이
정확히 전달됐다. 실제 모니터 DPI 변경, 다른 IME, AltGr, WSL/TUI 안에서의 조합은 미검증이다.

## 네이티브 키 이벤트 → PTY 수신

main `327e45b`의 위 전용 fixture에서, ASCII marker 9글자와 Enter의 key-down/up 이벤트를
한 SendInput 호출로 보내기 직전과 fixture가 한 줄을 수신한 직후를 Windows 공통 QPC
(`time.perf_counter_ns`)로 측정했다. 10회 warmup 후 100표본, 표본 간 100ms 휴지,
백그라운드의 별도 Whostty는 초당 600줄을 출력했다. 모든 marker가 정확히 일치했다.

| 제품 소스 | 최소 | p50 | p95 | p99 | 최대 |
|---|---:|---:|---:|---:|---:|
| main `327e45b` | 19.2124 ms | 27.0386 ms | 32.8170 ms | 35.7406 ms | 36.1564 ms |
| 수정 `b178e40` | 8.0693 ms | 26.7491 ms | 31.2229 ms | 35.6774 ms | 36.9200 ms |

nearest-rank 백분위수다. OS 이벤트·Whostty 입력 처리·PTY·Python 줄 디코딩/처리를 포함한다.
한 글자의 물리 타이핑 지연, IME 성능, 화면 표시 완료를 측정한 값은 아니다.
수정 빌드도 같은 방법으로 별도 100표본을 측정했다. 두 실행의 작은 차이를 성능 개선으로 주장하지 않는다.
사용한 Python 3.12의 clock 정보는 `QueryPerformanceCounter()`, monotonic, 해상도 100ns였다.
프로세스 간 같은 counter를 사용한다는 근거는 [Python time 문서](https://docs.python.org/3.12/library/time.html#time.perf_counter)다.

## PTY 쓰기 → GPU readback

제품 소스 `54e908e`의 배포 ZIP을 사용했다. `--check-latency 200`은 매 실행 20회 warmup을
제외한다. 각 요청의 고유 응답 토큰은 터미널 텍스트에서 확인하고, 해당 screenshot은 교대하는
glyph 색상 픽셀과 이전 색상의 부재로 검증해 회수까지 측정한다. screenshot 문자 인식은 하지 않는다.
응답 관측 수치에도 GUI polling/스케줄링이 포함된다.
진단 앱은 PTY 변경 시 repaint를 요청하고 30ms의 fallback repaint도 예약한다. 이 진단 스케줄링과
screenshot 회수 경로까지 포함한 수치이므로 일반 창의 화면 표시 지연과 동일하게 취급하지 않는다.

Vulkan/DX12 × 100/200% × 3회, 총 12회·2,400표본을 측정했다. 측정 도중 별도 Whostty
프로세스가 초당 600줄을 출력했다. 같은 PTY 안에서 대량 출력과 입력을 경쟁시킨 실험은 아니다.
각 실행은 실제 discrete GPU, 요청 배율, 정확한 표본 수와 종료 코드 0을 확인했다.
아래는 실행별 백분위수를 구한 뒤 3회의 중앙값이다. 합친 600표본의 백분위수가 아니다.

| 백엔드 | 배율 | 응답 관측 p50 | 응답 관측 p95 | readback p50 | readback p95 | readback p99 |
|---|---:|---:|---:|---:|---:|---:|
| Vulkan | 100% | 33.2645 ms | 50.3288 ms | 132.9250 ms | 136.4735 ms | 153.8744 ms |
| Vulkan | 200% | 33.6674 ms | 54.6629 ms | 132.9189 ms | 135.9544 ms | 151.6739 ms |
| DX12 | 100% | 31.5355 ms | 41.1275 ms | 82.0292 ms | 83.8073 ms | 86.0841 ms |
| DX12 | 200% | 31.2855 ms | 41.3817 ms | 82.0776 ms | 84.1684 ms | 90.7675 ms |

모든 12회에서 오래된 screenshot으로 무시한 표본은 0개였다. 수정 `b178e40` 배포본도 이 PC의
Vulkan/DX12 각각에서 10표본·20 warmup의 입력 왕복·GPU 검사에 통과했다. 이 짧은 검사는
수정 빌드 실행 확인이며 위 3회 기준값을 대체하지 않는다.

소프트웨어 readback은 GPU 큐·화면 회수와 드라이버·VSync 영향을 받는다. 이 값만으로 백엔드의
물리 타이핑 체감 우열을 결론 내리지 않는다. 물리 키보드→화면 발광, 순수 GPU 프레임 시간,
Ghostty 동등성은 측정하지 않았다.

## 지속 출력과 자원

main `327e45b`의 일반 GUI를 1,800초 동안 실행했다. 초당 600줄, ANSI·한글·CJK·결합 문자·이모지,
10초 간격 일반/대체 화면 전환, 10,000줄 scrollback을 사용했다. 다른 진단·IME 조작을 같은 PC에서
병행한 시간도 있으므로 부하는 일정하지 않다. 렌더링 배율은 기본값이며 강제하지 않았다.

- fixture 1,800.0초, 1,080,000줄, source UTF-8/ANSI payload 133,922,074 bytes, 화면 전환 179회.
- 전체 모니터 1,801.141초·1,802표본, 실제 자식 종료 코드 0, GUI 생존, `completed=true`.
- 5초 이상 출력 진행 정지 0건, 최대 관측 진행 간격 2.0초. 종료 후 GUI의 세션 종료 코드 0도 확인했다.

처음 30초를 제외한 메모리에서 앞/뒤 각 60초의 중앙값을 비교했다.

| 앱 자원 | 앞 구간 | 뒤 구간 | 차이 | warmup 이후 최대 |
|---|---:|---:|---:|---:|
| Private bytes | 498.97 MiB | 501.32 MiB | +2.36 MiB | 501.46 MiB |
| RSS | 414.72 MiB | 422.89 MiB | +8.17 MiB | 423.17 MiB |

전체 핸들 중앙값 800개·최대 818개·종료 시 794개, 스레드 중앙값 38개·최대 44개였다.
앱 CPU 중앙값은 논리 코어 하나를 100%로 세었을 때 23.44%다. 16코어 전체 사용률이 아니다.
출력 진행·프로세스 자원은 대리 지표이며 매 프레임의 정확성을 검사하지 않는다. 메모리는
큰 초기 사용량과 소폭 증가가 관측됐으며, 30분만으로 누수가 없다고 결론 내리지 않는다.
수시간·수일 부하, 실제 작업 혼합, GPU 메모리와 약 500MiB private 사용량의 원인 분석은 남아 있다.

이 실행 파일의 SHA256은 `1abff49d51fb22986f9c08baff52593bcb91766f787aa3d37baf74e1367bff99`다.
검사를 시작한 뒤 harness에 빌드 해시·처리량 메타데이터를 추가했다. 원래 실행의 report는 그대로
보관하고, 실제 측정 파일의 빌드 식별은 로컬 `provenance.json`에 덧붙였다.

수정 `b178e40` 빌드는 별도로 60초·36,000줄·4,464,073 source bytes·5회 화면 전환을 완료했다.
실제 자식 종료 코드 0, GUI 생존, 5초 이상 출력 진행 정지 0건이었다. 이를 수정 빌드의 30분
검사를 완료했다고 표현하지 않는다.

## 코드·배포 검증

제품 소스 `b178e40`의 [Check CI](https://github.com/safethecode/whostty/actions/runs/39344758245)는
Rust Windows 72개·macOS 70개와 fmt/Clippy에 통과했다. 명시 실행용 CPU 벤치마크는 플랫폼마다
1개씩 제외한다. Python 검사는 Windows 9개 통과, Mac은 Windows 프로세스 API 1개를 제외하고 통과했다.
[Windows package CI](https://github.com/safethecode/whostty/actions/runs/39344758367)의 배포·네 배율
실행 검사·10표본 latency smoke도 통과했다. 실기기에서 ZIP 체크섬·PE·DPI manifest·정적 CRT·고지를
다시 검증했다. 수정 실행 파일 SHA256은
`0f1a0394cc951d41bcfd256d8211001bed1dba20192831592d307f63cf951f6a`다.

원본 JSON·JSONL·IME 후보창 전후 캡처는 로컬 `dist/validation/windows-2026-10-20/`에 보관한다.
실행 방법은 [Windows 인계](windows-handoff.md)를 따른다.
