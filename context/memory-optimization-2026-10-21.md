# 메모리 최적화 적용 — 2026-10-21

[원인 분석](memory-and-ghostty-2026-10-21.md) 후속 변경이다. 스크롤백 기본 10,000행과 글자·스타일·검색·선택 기능을 유지한다.
측정 숫자는 [요약 JSON](memory-optimization-2026-10-21.json)과 각 CI artifact의 원본 report/allocator 로그로 확인한다.
제품 코드 기준 커밋은 `6553c25`이며, 문서만 바뀐 커밋과 구분한다.

## 적용한 변경

- wgpu `MemoryHints::MemoryUsage`와 `max_non_sampler_bindings=1024`를 기본으로 사용한다. 터미널의 텍스처 수보다 훨씬 큰
  일반 목적 DX12 풀/descriptor 예약을 줄인다. `WHOSTTY_GPU_MEMORY_MODE=performance`로 이전 GPU 설정을 비교할 수 있다.
  `memory`는 작은 풀만 사용하는 진단 모드이고 `compact`는 현재 기본값이다.
- TTC 전체를 face마다 복제하는 대신 선택한 face의 테이블로 SFNT를 재구성한다. 글리프 subset은 만들지 않는다.
  glyph ID, OpenType GSUB/GPOS, CJK cmap, 스타일을 보존하며 offset·정렬·checksum을 다시 계산한다.
  컨테이너 변경으로 유효하지 않게 되는 DSIG는 제외한다. 추출 검증 실패 시 기존 컨테이너로 되돌아간다.
  형식 근거는 [Microsoft OpenType 파일 규격](https://learn.microsoft.com/en-us/typography/opentype/spec/otff)이다.
- CJK collection은 필요한 face의 테이블만 파일에서 읽는다. 큰 전체 TTC를 읽고 해제해도 macOS malloc에 남던 임시 dirty 페이지를 줄인다.
  다른 폰트도 실제 파일 크기에 맞춰 읽고 Vec의 여유 capacity를 보관하지 않는다. 입력과 추출 결과 모두 64MiB 이내로 제한한다.
- shaped glyph atlas는 최초 사용 시 512² RGBA 페이지(1MiB)를 만든다. 필요할 때 페이지를 추가하며 전체 상한은 기존과 같은 16MiB다.
  큰 글자는 최대 2048² 페이지를 사용할 수 있다. 예전 pass에서만 쓴 페이지를 LRU로 교체하며, 이번 pass에서 사용한 페이지는
  다른 run·패널에서 참조할 수 있으므로 교체하지 않는다. 페이지가 바뀌면 mesh도 나눠 glyph 순서를 유지한다.

## 실측 결과

Windows CI의 DX12/Microsoft Basic Render Driver(소프트웨어 어댑터)에서 같은 8초 대기→20초·초당 600행 fixture를 사용했다.
PrivateUsage의 마지막 구간 중앙값을 같은 방식으로 비교한다. 12,000행이 출력되며 10,000행 기록이 채워지는 조건이다.
이는 원래 RTX 장비에서 관측한 약 500MiB의 재측정 결과가 아니다.

| Windows 지표 | 이전 기본값 | 최적화 기본값 |
|---|---:|---:|
| 스크롤백 0, late PrivateUsage | 431.35MiB | 58.40MiB |
| 스크롤백 10,000행, late PrivateUsage | 461.73MiB | 87.06MiB |
| 출력 후 GPU allocator 예약 | 320MiB | 12MiB |
| 출력 후 GPU live allocation | 18.44MiB | 3.44MiB |
| shaped glyph atlas 논리 크기 | 16MiB | 1MiB |

10,000행 조건에서는 약 81.1% 감소, 약 5.3배 축소다. 50MiB/10배 목표는 아직 달성하지 않았다.
[이전 실행](https://github.com/safethecode/whostty/actions/runs/39592731811)과
[최종 제품 코드 실행](https://github.com/safethecode/whostty/actions/runs/39600558205)은 각각 동일 fixture 조건의 독립 CI VM이다.
동일 물리 장비의 무작위 교차 실험은 아니므로 작은 수 MiB 차이는 일반화하지 않는다.
최종 실행의 같은 binary에서도 `memory` 진단 모드는 118.66MiB, `performance`는 427.96MiB(0행 late PrivateUsage)였다.
따라서 큰 예약 비용의 감소는 단순히 CI 장비 차이만으로 설명되지 않는다.

Mac은 이전과 같은 Apple M3 Pro/Metal, Menlo 15pt, 121×29 그리드와 비활성 화면 창 조건이다.
같은 Unicode 18,000행·약 32초 지속 출력과 20,400,000바이트 burst를 사용했다. 최종 binary와 fixture의 SHA256을 확인해
각 제품 3회의 유효 실행을 모았다. 창이 화면에서 사라진 시작 시도 1회는 측정 전 중단하고 같은 binary로 재시작했다.
출력의 SHA256은 이전과 같은 `4a3d36a2ef4bf0eadc0da4d28d208ce808b0ac7e9b6d279746163368520952ed`다.

| Mac 지표, 각 3회 중앙값 | 이전 Whostty | 최적화 Whostty | 같은 시기 Ghostty 1.2.3 |
|---|---:|---:|---:|
| 스크롤백 0, 출력 후 physical footprint | 426.25MiB | 271.53MiB | 79.36MiB |
| 출력 후 RSS | 236.73MiB | 111.97MiB | 130.27MiB |
| 지속 출력 앱 CPU, 논리 코어 하나=100% | 1.317% | 1.350% | 0.186% |
| burst 쓰기→파서 DSR | 119.00MiB/s | 125.18MiB/s | 77.77MiB/s |
| 작은 PTY 출력→DSR p50 | 0.218ms | 0.224ms | 0.265ms |
| 작은 PTY 출력→DSR p95 | 0.539ms | 0.297ms | 0.355ms |

Mac 출력 후 footprint는 약 36.3% 줄었지만 CPU 비용의 개선은 관측하지 못했다. 새 Whostty footprint 범위는 271.38–273.92MiB,
burst는 122.45–126.15MiB/s였다. 이전 burst 편차와 데스크톱 부하를 감안하면 속도 우위를 단정할 수 없다.
DSR은 파서 응답이며 표시 완료/물리 키 입력 지연이 아니다. CPU는 자식 프로그램·WindowServer를 제외한다.

폰트 원본 길이는 65,405,744→6,744,728바이트, capacity는 77,140,760→6,744,728바이트다. egui 내부의 별도 복제는 이 수치에 포함하지 않는다.
첫 최종 실행의 vmmap에서 MALLOC_LARGE dirty는 약 122.1→6.1MiB로 줄었다. 중간 구현에서 전체 collection을 읽고 해제하던
약 56.9MiB의 MALLOC_LARGE(empty)는 선택 테이블 읽기 적용 후 약 4.1MiB였다. 두 경우 모두 해제된 페이지를 살아 있는 누수로 해석하지 않는다.
Metal/드라이버의 owned unmapped graphics 약 184MiB와 표시 표면 비용은 남아 있다.

release CPU 제출 benchmark 5회는 cold frame 중앙값 2.867ms, warm p50 중앙값 0.163ms/p95 0.176ms였다.
기존 기록은 cold 8.674ms, warm p50 0.176ms/p95 0.181ms다. 이 benchmark는 GPU 제출·표시나 입력 지연을 포함하지 않는다.

기본 기록량도 각 1회 측정했다. Whostty 10,000행의 출력 후 footprint는 446.67→303.92MiB였다.
Ghostty 10,000,000바이트는 최종 비교에서 89.28MiB였다. 두 제품의 기록 단위/보존량은 같지 않다.
최종 실행 원본은 `dist/performance/optimized-final-zero-history/`, `optimized-final-default-history/`,
`optimized-final-runtime/`, `windows-optimized-final/`에 있다. 중간 구현/실패한 시작 시도의 수치는 최종 중앙값에 합치지 않는다.

## 검증

- Rust workspace 테스트, Clippy 경고 없음, rustfmt, Python 검사 통과. Mac은 Rust 71개 통과/benchmark 1개 ignore,
  Python 14개 통과/Windows 전용 1개 skip이다. 별도로 release CPU benchmark를 실행했다.
- 합자·mark·CJK·SVG fallback과 같은 glyph의 재업로드 방지 검사를 유지했다. collection 추출 전후의 모든 비-head/DSIG 테이블,
  SFNT 전체 checksum, 실제 Menlo 네 스타일과 parser 호환성을 검사한다. 파일 읽기량 검사로 사용하지 않는 collection 데이터 읽기를 막는다.
- atlas를 여러 페이지로 채우고 같은 pass의 기존 UV가 유지되는지, 다음 pass에서 다른 페이지를 교체해도 먼저 사용한 페이지가 살아 있는지 검사한다.
- 최종 Mac 앱의 `--check --check-scale` 1/1.25/1.5/2/4 각각에 실제 PTY Unicode·GPU readback·10회 입력 경로 검사를 실행해 통과했다.
  Windows 배포본도 bundled ConPTY, 1/1.25/1.5/2 배율과 10회 latency smoke를 검사한다.
- CI는 기본 설정의 0/10,000행 프로필 각각에 대해 완료·12,000행 출력·GUI 생존을 먼저 확인한다.
  고정 fixture에서 late PrivateUsage ≤128MiB, GPU 예약 ≤64MiB, shaped atlas ≤4MiB를 검사한다.
  이 수치는 회귀 검출용 여유 상한이며 50MiB 제품 목표 달성을 뜻하지 않는다. 이전 compact 실행의 16MiB atlas는 새 검사를 통과하지 못한다.

## 남은 비용과 50MiB 목표

스크롤백을 끄거나 기본 10,000행을 줄여 목표에 맞춘 결과가 아니다. Windows에서 10,000행의 기존 Cell/Row 저장은 여전히
약 29~31MiB를 더 쓴다. egui/epaint의 owned 폰트 복제도 남아 있으며 Arc를 shaping과 공유하는 것만으로 그 복제까지 없어지지 않는다.
50MiB에 가까워지려면 오래된 행의 압축 저장과 폰트 parser/rasterizer 사이의 소유권 공유가 추가로 필요하다.
현재 Alacritty grid는 행을 연속 Cell 배열로 보유하므로 단순한 외부 캐시 추가로 압축 문제를 해결할 수 없다.
검색·복사·resize/reflow·wide/combining 문자·hyperlink를 보존하는 저장 계층 변경과 별도 회귀 검증이 필요하다.

Mac은 Metal/드라이버 소유 리소스와 표시 표면도 크게 남는다. Windows PrivateUsage와 Mac physical footprint를 합치거나
서로 같은 지표로 비교하지 않는다. 이번 결과는 전체 1/10 달성이나 Ghostty와의 성능 동등성 달성을 뜻하지 않는다.
원래 Windows 11/RTX 3080 Ti 장비의 같은 설정 재측정, 활성 창의 GPU 표시/물리 입력 지연, 장시간·다중 패널 부하는 계속 확인해야 한다.

재현:

```sh
cargo build --release --locked
python3 scripts/terminal-performance.py --output dist/performance/new-optimized --repeat 3 --seconds 30 --capture
python3 scripts/terminal-performance.py --output dist/performance/new-optimized-history --repeat 1 --seconds 30 --scrollback 10000 --ghostty-scrollback-bytes 10000000 --capture
```

Windows의 배포·진단·메모리 검사는 `.github/workflows/windows-package.yml`에서 같은 패키지 실행 파일로 수행한다.
