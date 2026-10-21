# 메모리 원인 분석과 Ghostty 비교 — 2026-10-21

기본 상태의 메모리 비용이 크다는 근거를 Windows와 Mac에서 확보했다. 출력 큐나 스크롤백만으로 약 500MiB를 설명할 수 없다.
Windows에서는 wgpu의 큰 기본 GPU 풀과 디스크립터 힙을, Mac에서는 그래픽 리소스와 전체 TTC 폰트 버퍼 중복을 우선 조사한다.
아래 결과는 설정을 바꾸기 전 기준값과 명시적으로 구분한다. 이번 변경은 진단과 비교 도구이며 기본 GPU 설정은 유지한다.

## Ghostty와 동일 장비 비교

Apple M3 Pro / macOS 26.5.2 / arm64. Whostty 제품 소스 `bb9ef13`의 `cargo build --release --locked` 산출물과
설치된 Ghostty 1.2.3 ReleaseFast/Metal을 비교했다. Ghostty를 업데이트하거나 사용자 설정을 바꾸지 않았다.
두 앱 모두 일회용 설정, Menlo 15pt, 121열×29행, Python 3의 같은 프로그램을 사용한다. 셸 프로필·셸 연동은 사용하지 않는다.
기본 창의 폰트 메트릭과 앱 UI는 달라 전체 창 크기는 Whostty 1120×752pt, Ghostty 1158×548pt였다.
따라서 같은 픽셀 수의 순수 GPU 부하 비교는 아니다. 모니터 환경과 창 크기는 원본 JSON에 기록했다.

UI 자동화 도구에서 Ghostty 앱 제어가 차단되어, 두 앱을 모두 비활성 백그라운드 창으로 맞췄다.
창이 화면에 존재하고 측정 전후 다른 앱이 전면에 있는지 검사했다. 중간의 일시적 포커스 변경·창 가림 정도를
매 프레임 추적하지는 않는다. 사용자 데스크톱의 부하·메모리 압력은 격리하지 않았다.

비교 순서는 Whostty/Ghostty, Ghostty/Whostty, Whostty/Ghostty다. 새 프로세스마다 5초 대기 후 측정한다.
스크롤백은 두 앱 모두 0으로 설정해 보존 용량 차이를 없앴다. 색상·한글·CJK·분해 결합 문자·이모지가 들어간
18,000행을 30행씩 50ms 간격으로 출력한다. OS 대기로 실제 지속 시간은 약 32초였다.
이어서 정확히 같은 20,400,000바이트/200,000행을 출력하고 DSR 응답까지 잰다.
각 실행의 입력 SHA256은 `4a3d36a2ef4bf0eadc0da4d28d208ce808b0ac7e9b6d279746163368520952ed`로 같았다.

| 지표 | Whostty, 3회 중앙값 (범위) | Ghostty, 3회 중앙값 (범위) |
|---|---:|---:|
| 출력 전 physical footprint | 392.27MiB (377.27–392.69) | 79.05MiB (78.75–79.11) |
| 지속 출력 후 physical footprint | 426.25MiB (413.95–426.50) | 80.24MiB (79.94–80.55) |
| 지속 출력 후 RSS 중앙값 | 236.73MiB | 131.06MiB |
| 지속 출력 중 앱 CPU, 논리 코어 하나=100% | 1.317% (1.172–1.413) | 0.152% (0.148–0.156) |
| burst 쓰기→파서 DSR 처리량 | 119.00MiB/s (41.05–128.26) | 72.49MiB/s (69.35–73.67) |
| 작은 PTY 출력→DSR 응답 p50의 중앙값 | 0.218ms | 0.227ms |
| 같은 응답 p95의 중앙값 | 0.539ms | 0.461ms |

CPU는 자식 Python과 WindowServer를 제외한 앱 프로세스의 누적 CPU 시간 차이다. 각 실행의 p50/p95는
10회 워밍업을 뺀 100표본이며, 세 실행의 백분위수를 다시 중앙값으로 정리했다.
DSR은 출력이 터미널 파서에 도달했다는 확인이다. 중간 프레임 전부의 렌더링, GPU 표시 완료, 키보드 입력 지연을 뜻하지 않는다.
Whostty의 burst 편차가 크므로 중앙값만으로 처리량 우위를 주장하지 않는다. 백그라운드 CPU 비율을 활성 창의 체감 속도로 일반화하지 않는다.

재현:

```sh
cargo build --release --locked --package whostty
python3 scripts/terminal-performance.py --output dist/performance/new-comparison --repeat 3 --seconds 30 --burst-lines 200000 --capture
```

기본은 백그라운드 상태 검사다. `--window-state foreground`는 각 전용 창을 직접 활성화한 뒤 측정할 수 있다.
설정·상태·샘플·vmmap·heap 원본은 `dist/performance/background-zero-history/`에 있다.
공개 가능한 측정 요약은 [JSON](performance-comparison-2026-10-21.json)에 보관한다.

## Mac 메모리 기여

첫 비교 실행의 `vmmap`에서 출력 전 footprint는 377.3MiB, 출력 후는 413.8MiB였다.

| 분류 | 출력 전 | 출력 후 | 근거와 해석 |
|---|---:|---:|---|
| owned unmapped (graphics), dirty | 180.8MiB | 199.3MiB | Metal/드라이버 소유 리소스. 개별 텍스처의 논리 바이트와 동일하지 않다. |
| IOSurface, resident | 37.8MiB | 37.8MiB | 창/표시 표면. 다른 범주의 바이트와 무조건 합산하지 않는다. |
| MALLOC_LARGE, dirty | 122.1MiB | 122.1MiB | 큰 폰트 원본 및 복제 버퍼가 대부분이다. |
| MALLOC_LARGE (empty), dirty | 없음 | 16.0MiB | 해제 뒤 malloc 영역에 남아 있는 페이지. 살아 있는 폰트 객체나 누수로 간주하지 않는다. |
| MALLOC_SMALL, dirty | 17.1MiB | 19.1MiB | 작은 객체·레이아웃·문자 데이터 등. |

별도 `MallocStackLogging=1` 실행의 live allocation 스택에서 다음을 확인했다. 이 실행은 계측 오버헤드가 있어
위 성능 수치에 합치지 않는다. `heap --addresses=all --noContent`와 `malloc_history -allBySize`를 사용했다.

- 약 64MiB: `configure_fonts → read_font → alloc::raw_vec::finish_grow`. 한글 fallback인 AppleSDGothicNeo.ttc
  55,373,848바이트를 `read_to_end`로 읽으면서 유지한 Vec capacity다.
- 약 52.8MiB: 같은 TTC를 egui/epaint의 `FontsImpl::new`가 다시 복제한 버퍼다.
  `FontData`의 Owned 데이터를 `FontVec::try_from_vec_and_index(bytes.clone(), index)`로 바꾸는 경로와 일치한다.
- Menlo.ttc도 네 스타일별 약 2.06MiB씩 원본과 egui 복제본을 유지한다. `Arc`로 shaping 쪽과 원본을 공유해도
  egui 내부 복제까지 없애는 구조는 아니다.
- shaped glyph atlas는 첫 사용에 2048² RGBA, 16MiB를 준비한다. 출력 후 GPU 리소스 증가와 일시적 CPU 이미지
  페이지 잔류는 이 경로와 부합한다. 전체 GPU/드라이버 사용량을 이 atlas 하나로 설명하지 않는다.

AppleSDGothicNeo TTC에는 18개 face가 있고, 현재 고르는 face 0의 SFNT table 길이 합은 약 3.03MiB다.
전체 52.8MiB collection을 보존하는 대신 선택한 face만 안전하게 분리하는 것이 유력한 개선 후보다.
SFNT 재구성·폰트 체크섬·모든 글리프와 스타일 보존을 검증해야 하며, 이번 분석에서 추출 코드를 제품에 적용하지는 않았다.

같은 장비에서 Whostty 10,000행 / Ghostty 10,000,000바이트의 제품별 기본 기록량도 한 번씩 측정했다.
출력 전→후 footprint는 Whostty 377.20→446.67MiB, Ghostty 78.45→88.41MiB였다.
서로 다른 기록 단위를 동일한 기록량으로 취급하지 않는다. Whostty의 첫 zero-history 실행 대비 증가분 차이는 약 32.8MiB였고,
heap에서 늘어난 약 10,000개의 3KiB 행 할당과 부합했다. 이것이 전체 기본 메모리 비용의 주원인은 아니다.

## Windows에서의 재현과 대조

[초기 Windows CI](https://github.com/safethecode/whostty/actions/runs/39591461683)는 동일한 배포 실행 파일을
DX12 / Microsoft Basic Render Driver에서 8초 대기→초당 600행·20초 출력으로 실행했다.
실제 RTX 3080 Ti와 다른 소프트웨어 어댑터다. 아래 PrivateUsage와 Mac physical footprint는 서로 다른 지표다.

| 스크롤백 | 대기 구간 PrivateUsage 중앙값 | 출력 구간 PrivateUsage 중앙값 | 출력 후 committed MEM_PRIVATE |
|---|---:|---:|---:|
| 0행 | 428.35MiB | 431.86MiB | 428.99MiB |
| 10,000행 | 428.27MiB | 451.42MiB | 457.79MiB |

두 실행 모두 12,000행·종료 0·GUI 생존·완료 판정을 통과했다. 대기 중앙값은 시작 2초 이후 표본이다.
출력 중앙값은 기록이 채워지는 구간까지 포함한다. committed 영역은 `VirtualQueryEx`의 스냅샷으로,
PrivateUsage나 물리 상주량·GPU VRAM과 같지 않다. 가장 큰 private 영역은 대기부터 256MiB, 64MiB, 61.04MiB,
16MiB, 12.84MiB였다. 출력 전부터 약 428MiB라는 점이 핵심이다.

고정된 wgpu-hal 24.0.4의 DX12 구현은 기본 Performance 힌트에서 gpu-allocator의 256MiB device / 64MiB host
블록을 사용한다. MemoryUsage 힌트에서는 8MiB / 4MiB로 바뀐다. egui는 기본 device descriptor를 그대로 요청한다.
또 `max_non_sampler_bindings`를 descriptor heap 크기로 사용한다. 큰 기본값은 이 터미널이 사용하는 텍스처 수에 비해 크다.
수치가 같다는 것만으로 OS 영역을 GPU 객체에 확정 대응시키지 않고, 다음 opt-in 설정 대조와 allocator 보고로 확인한다.

[allocator 보고를 추가한 Windows CI](https://github.com/safethecode/whostty/actions/runs/39592383265)의 일반 GUI에서도
초기 GPU live allocation은 0.8125MiB인데 reserved block은 256+64=320MiB였다. 20초 출력 뒤 live allocation은
18.4375MiB, reserved는 여전히 320MiB였다. 폰트 원본 길이는 14.53MiB/capacity 17.70MiB,
논리 텍스처는 egui 1MiB와 shaped glyph atlas 16MiB, 레이아웃 캐시 추정치는 약 0.56MiB였다.
큰 GPU 풀은 이 실행의 고정 비용이라는 근거이며, 전체 private bytes를 폰트나 스크롤백에 돌릴 수 없다.

### GPU 설정을 바꾼 인과 대조

[동일 빌드 대조 CI](https://github.com/safethecode/whostty/actions/runs/39592731811)는 같은 배포본·DX12 소프트웨어
어댑터·스크롤백 0·8초 대기·20초/12,000행 출력에서 다음을 확인했다. 각 설정 한 번의 진단이다.

| GPU 설정 | 대기 PrivateUsage 중앙값 | 출력 PrivateUsage 중앙값 | 출력 후 GPU live / reserved |
|---|---:|---:|---:|
| 기본값 | 428.91MiB | 431.34MiB | 18.44 / 320MiB |
| MemoryUsage만 | 119.66MiB | 138.92MiB | 18.50 / 28MiB |
| MemoryUsage + descriptor 1024개 | 59.19MiB | 78.32MiB | 18.44 / 28MiB |

대기 중 GPU 예약은 기본 320MiB에서 12MiB로 줄었고, 출력에 필요한 16MiB atlas를 만들면 28MiB가 됐다.
기본값 대비 출력 중 private 약 353MiB 감소는 풀 예약 약 292MiB 감소와 약 61MiB descriptor 영역 제거로 설명된다.
VirtualQueryEx에서도 256/64MiB 영역이 사라지고, compact에서 61.04MiB 영역이 사라졌다.
따라서 적어도 이 Windows DX12 재현의 큰 기본 메모리 비용은 이 두 설정에 인과적으로 연결된다.

세 설정의 지속 출력이 모두 완료됐고, memory/compact 각각의 유니코드 입력 왕복·shaped glyph·GPU readback 및
10표본 latency smoke도 통과했다. 이 smoke를 입력 지연 개선 또는 RTX GPU의 성능 회귀 없음으로 해석하지 않는다.
두 설정을 제품 기본값으로 적용하는 작업은 별도다. 실제 RTX/Vulkan 장비와 탭·분할·대량 glyph·DPI 부하에서
작은 GPU 풀이 재할당/지연을 늘리지 않는지 확인할 필요가 있다.

```powershell
# 새 배포 ZIP에서, 원래 RTX 장비에도 같은 명령으로 재현할 수 있다.
$env:WGPU_BACKEND = 'dx12'
python scripts/windows-soak.py --exe .\whostty.exe --output .\memory-default --idle-seconds 8 --duration 60 --scrollback 0 --alternate-interval 0 --memory-map --close-on-success
$env:WHOSTTY_GPU_MEMORY_MODE = 'memory' # MemoryUsage만
python scripts/windows-soak.py --exe .\whostty.exe --output .\memory-pools --idle-seconds 8 --duration 60 --scrollback 0 --alternate-interval 0 --memory-map --close-on-success
$env:WHOSTTY_GPU_MEMORY_MODE = 'compact' # MemoryUsage + descriptor 1024개
python scripts/windows-soak.py --exe .\whostty.exe --output .\memory-compact --idle-seconds 8 --duration 60 --scrollback 0 --alternate-interval 0 --memory-map --close-on-success
Remove-Item Env:WHOSTTY_GPU_MEMORY_MODE
Remove-Item Env:WGPU_BACKEND
```

`--memory-map`은 해당 실행에만 WHOSTTY_MEMORY_LOG를 지정한다. `allocator.jsonl`에 폰트 원본 길이/capacity,
레이아웃 캐시 추정치, 논리 텍스처 바이트, 지원되는 GPU backend의 live allocation/reserved block을 기록한다.
Metal처럼 allocator 보고를 지원하지 않는 backend는 null을 기록한다. GPU reserved bytes를 OS private bytes에 더하지 않는다.
기존 진단 파일을 덮어쓰지 않으며 진단을 켠 실행에만 최대 1초 간격의 파일 출력과 repaint가 추가된다.

## 남은 해석 범위

원래 RTX 장비의 Vulkan/DX12 500MiB를 이 CI·Mac 수치와 동일시하지 않는다. 해당 장비에서 새 진단을 재실행하면
GPU reservation, 디스크립터, CPU 폰트/기록 비용을 구분할 근거가 더 생긴다. 일반 부하의 수시간 누수 여부,
순수 GPU 프레임 시간과 물리 키보드→화면 발광 비교는 이번 결과에 포함하지 않는다.

근거: [wgpu DX12 suballocation](https://github.com/gfx-rs/wgpu/blob/v24.0.4/wgpu-hal/src/dx12/suballocation.rs),
[wgpu DX12 device](https://github.com/gfx-rs/wgpu/blob/v24.0.4/wgpu-hal/src/dx12/device.rs),
[epaint 0.31.1 font loading](https://docs.rs/epaint/0.31.1/src/epaint/text/fonts.rs.html),
[VirtualQueryEx](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualqueryex),
[Ghostty scrollback configuration](https://ghostty.org/docs/config/reference#scrollback-limit).
