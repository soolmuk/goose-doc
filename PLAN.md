# goose-doc — 오프라인 문서 호스팅 도구 계획

goose 공식 문서(goose-docs.ai에 배포되는 것과 동일한 docs root)를 **사내 서버에서 직접 호스팅**해서,
`goose-doc-guide` 스킬이 네트워크 없이(또는 인터넷 없이) 동작하도록 만드는 프로젝트.

- 저장소 위치(이 계획의 루트): `/Users/ihoseong/Documents/goose-doc`
- 대상 OS: **Windows / macOS / Linux(Ubuntu)**
- 실행 형태: 서버(헤드리스 가능) + 더블클릭 실행 시 뜨는 **간단한 패널 UI**

---

## 1. 목표와 범위

| # | 요구사항 | 설계 대응 |
|---|---|---|
| 1 | Windows / macOS / Ubuntu 지원 | GitHub Actions hosted runner에서 **각 OS 네이티브 빌드**(크로스 컴파일 없음) |
| 2 | goose 새 릴리즈 시 GitHub Actions가 각 OS에서 "단순 실행"으로 문서 호스팅 가능 | goose 릴리즈를 감지해 docs 번들 생성 → 3개 OS용 실행 파일 빌드 + 스모크 테스트 |
| 3 | `goose-doc(.exe)` 실행 → 패널 UI → IP/포트 설정 후 시작/취소 | **egui/eframe 네이티브 창** 패널 + 같은 바이너리의 `--headless` 모드. 구현 방식은 §5.3 참고 |
| 4 | 저장소 CI/CD 최소화 (goose 저장소 CI/CD 불필요) | 워크플로 3개(번들/앱 빌드/릴리스)만. **goose 저장소에 어떤 트리거도 걸지 않음**(읽기만) |
| 5 | 서버에서 실행할 용도 | `--headless`, systemd/launchd/Windows 서비스 설치, **기본 바인드 `0.0.0.0`(모든 인터페이스)** — 서버가 다른 PC에 호스팅하려면 필수. `--local-only`로 이 머신 전용 |

**비목표(이번 범위 제외)**
- goose 문서 내용 자체를 수정/번역하는 것 (원문 그대로 호스팅)
- goose CLI 바이너리를 함께 배포하는 것 (호스팅만 담당)
- goose 저장소에 PR/워크플로를 추가하는 것 (요구사항 4에 따라 불가)

---

## 2. 근거: 확인된 사실 (설계 전제)

계획의 모든 전제는 아래에서 직접 확인했다.

| # | 사실 | 확인 방법 |
|---|---|---|
| F1 | 스킬 이름은 `goose-doc`, 실제 등록명은 **`goose-doc-guide`** (goose 내장 builtin skill, `builtin://skills/goose-doc-guide`) | `goose skills list` 출력 |
| F2 | 스킬은 `{{GOOSE_DOCS_ROOT}}` 플레이스홀더를 사용하고, goose가 이를 config/환경변수 값으로 치환. 미설정 시 `https://goose-docs.ai` | `crates/goose/src/skills/mod.rs`(`resolve_docs_root_placeholder`, `DEFAULT_GOOSE_DOCS_ROOT`), `crates/goose/src/config/base.rs`(`get_goose_docs_root`) |
| F3 | `GOOSE_DOCS_ROOT`는 **로컬 경로 또는 HTTP(S) URL** 모두 허용 | `documentation/docs/guides/offline-docs.md` |
| F4 | docs root 레이아웃은 `<root>/goose-docs-map.md` + `<root>/docs/**` | `documentation/docs/guides/offline-docs.md` |
| F5 | 문서 빌드 산출물 `documentation/build/`가 **그대로 docs root**가 됨 (`goose-docs-map.md` + `docs/` 포함) | `documentation/docs/guides/offline-docs.md`, `documentation/scripts/verify-build.sh` |
| F6 | 공식 빌드 절차: goose 체크아웃(바이너리와 **같은 버전**) → `cd documentation` → `npm run build` | `documentation/docs/guides/offline-docs.md` |
| F7 | `npm run build` = `node scripts/generate-docs-map.js && docusaurus build`. 산출: `static/goose-docs-map.md`(→ build 루트로 복사) + `build/docs/*.md` export | `documentation/package.json`, `scripts/generate-docs-map.js`, `plugins/markdown-export.cjs` |
| F8 | map은 `getting-started/*` + `guides/**`만 포함 → **60개 문서**. 실서비스 `goose-docs.ai/goose-docs-map.md`와 경로 집합이 일치 | 실측(`paths.txt` 60개) vs `generate-docs-map.js`의 sections |
| F9 | `documentation/build`(전체 사이트)는 `.md` export까지 포함하므로 **스킬용 + 사람용 브라우징 동시 충족** | `plugins/markdown-export.cjs` (postBuild에서 `outDir/docs`로 md 복사) |
| F10 | goose 릴리즈 자산명이 OS별로 존재하고 태그가 `v1.*` (`v1.52.0` 확인) | `gh release view v1.52.0` |
| F11 | goose 문서 라이선스는 Apache-2.0 → 재배포 시 고지 필요 | `gh api repos/aaif-goose/goose/license` |
| F12 | `GOOSE_DOCS_ROOT` 로컬 경로 지정 시 오프라인으로 스킬이 실제 동작함 (파일 도구만 사용, 네트워크 0회) | `GOOSE_DOCS_ROOT=/tmp/goose-docs-offline goose run …` 실행 검증 완료 |
| F13 | goose 저장소 릴리스 워크플로는 `documentation/**` 변경을 무시(`paths-ignore`) → 우리 작업이 goose CI에 영향 없음 | `.github/workflows/release.yml`, `canary.yml` |
| F14 | 문서 배포는 goose 저장소의 `deploy-docs-and-extensions.yml`이 담당 (Pages). 우리는 이 경로를 건드리지 않음 | `.github/workflows/deploy-docs-and-extensions.yml` |
| **F15** | `documentation/docs/gdk/acp/reference.md`는 **커밋되지 않은 생성물**이고 `docs/gdk/acp/index.md`가 이를 링크한다. 없으면 `onBrokenLinks: throw`로 **빌드 실패** | v1.52.0 클론에서 `npm run build` 실패 재현 → 생성 후 성공 |
| **F16** | ACP 생성 스크립트는 `documentation/scripts/generate-acp-docs.js`이고 `schema meta output [version]` 인자를 받는다. 스키마/메타는 **태그 안에 존재**한다(`crates/goose/acp-{schema,meta}.json`) | `generate-acp-docs.js`의 `process.argv` 처리, `git show v1.52.0:...` |
| **F17** | 업스트림 헬퍼 `generate-released-acp-docs.sh`는 버전을 **`releases/latest`로 해석**한다 → 특정 태그를 빌드할 때 문서 버전이 어긋날 수 있다 | 같은 스크립트 6번째 줄 |
| **F18** | `npm run build`는 ACP 단계를 **포함하지 않는다**. CI가 별도 스텝으로 먼저 실행한다 | `documentation/package.json`의 `build` 스크립트 |
| **F19** | `npm run build` 성공 시 `build/`는 완전한 docs root가 된다. 실측: 맵 항목 **61개 누락 0**, md export **175개** | 실측(v1.52.0, P1) |
| **F20** | site 번들은 실측 **344.4MB**. 주 원인은 블로그 이미지(`assets/images` 191MB)와 `videos/`(71MB)이고, 스킬에 필요한 부분은 map+`docs/*.md` = **1.8MB** | 실측(P1) |
| **F21** | `npm ci`는 v1.52.0 기준 **3초**, `npm run build`는 약 **30초** | 실측(P1, 이 머신) |

### 2.1 중요한 결론 3가지

1. **번들 소스는 goose 정식 릴리즈 태그**를 쓴다. goose 저장소 CI/CD는 전혀 필요 없고, 우리 저장소의 워크플로가 `git checkout vX.Y.Z` + `npm ci` + `npm run build`를 수행한다. (F6, F7, F13)
2. **`documentation/build` 전체를 배포하면 끝난다.** 별도 변환 도구를 만들 필요가 없다. HTML 사이트(사람용) + `goose-docs-map.md` + `docs/*.md`(스킬용)가 한 디렉터리에 있다. (F5, F7, F9)
3. **호스팅 서버의 루트 = docs root**로 설계하면 goose CLI는 `GOOSE_DOCS_ROOT=http://<host>:<port>` 한 줄로 붙는다. (F3, F4)

---

## 3. 아키텍처

```
[aaif-goose/goose]  (읽기 전용, 릴리즈 태그만 사용)
        │  git checkout vX.Y.Z
        ▼
┌──────────────────────────────┐
│ goose-doc/tools/             │   docs 번들 생성 (Node는 goose 저장소의 것을 사용)
│  build-docs-bundle.sh        │   → documentation/build  →  goose-docs-<ver>.tar.gz
└──────────────────────────────┘
        │
        ▼  goose-doc 저장소 릴리즈 자산
┌──────────────────────────────┐
│ goose-docs-<ver>.tar.gz      │   goose-docs-map.md + docs/**  (+ HTML 사이트)
│ + manifest.json (sha256)     │
└──────────────────────────────┘
        │  (런타임 다운로드 또는 --docs-dir)
        ▼
┌──────────────────────────────┐
│ goose-doc 실행 파일           │   Rust 단일 바이너리
│  ├─ 패널 UI (egui 창)        │   IP/포트 설정, 시작/중지, 상태
│  ├─ 정적 서버 (axum)         │   docs root 서빙
│  └─ --headless               │   서버/컨테이너용
└──────────────────────────────┘
        │  http://host:port
        ▼
[goose CLI / Desktop]  GOOSE_DOCS_ROOT=http://host:port  →  goose-doc-guide 스킬 오프라인 동작
```

### 3.1 저장소 레이아웃

```
goose-doc/
├── PLAN.md                     # 이 문서
├── README.md
├── AGENTS.md
├── Cargo.toml                  # 단일 크레이트(초기), 필요 시 워크스페이스로 분리
├── src/
│   ├── main.rs                 # 진입점: 인자 파싱 → 패널 또는 headless
│   ├── cli.rs                  # clap 정의
│   ├── config.rs               # 설정 영속화(IP/포트/문서버전)
│   ├── docs.rs                 # 번들 해석/다운로드/검증/압축해제
│   ├── server.rs               # axum 정적 서버 + 컨트롤 API
│   ├── panel/                  # egui/eframe 패널 UI (창)
│   │   ├── mod.rs              # 패널 상태 모델(GUI 비의존, 테스트 대상)
│   │   └── app.rs              # eframe::App 구현
│   └── fonts.rs                # (선택) CJK 폰트 등록
├── tools/
│   ├── build-docs-bundle.sh    # goose 태그 → tarball + manifest
│   └── smoke-test.sh           # OS별 실행 검증(CI에서 호출)
├── tests/
│   └── serve_test.rs           # 서버 통합 테스트(fixture docs root 사용)
├── fixtures/
│   └── docs-root/              # 최소 docs root (map 1개 + docs/ 2개) — CI 스모크용
└── .github/workflows/
    ├── docs-bundle.yml         # goose 릴리즈 → docs 번들 생성/배포
    ├── build-app.yml           # 3 OS 빌드 + 스모크 테스트 (재사용 워크플로)
    └── release.yml             # 3 OS 릴리스 자산 업로드
```

### 3.2 docs root 계약 (반드시 지킬 것)

```
<docs-root>/                      ← 서버가 서빙하는 루트
├── goose-docs-map.md             ← 스킬이 가장 먼저 읽는 인덱스
├── docs/
│   ├── getting-started/*.md
│   └── guides/**/*.md
└── (선택) *.html, assets/ ...    ← 사람이 브라우저로 보는 정적 사이트
```

- `goose-docs-map.md`가 루트에 있어야 한다. (F4, F5)
- map에 적힌 경로와 실제 파일 경로가 정확히 일치해야 한다 (스킬이 EXACT path만 읽음). (F8)
- 정적 서버는 **SPA 폴백을 하지 않는다**. 파일이 없으면 404. (`docusaurus serve`가 아닌 `serve-static.js` 의미론)

---

## 4. 컴포넌트 A — docs 번들 생성 (`tools/build-docs-bundle.sh`)

### 4.1 입력/출력

| 항목 | 값 |
|---|---|
| 입력 | goose 버전(태그 `vX.Y.Z`) |
| 소스 | `git clone --depth 1` 후 태그 체크아웃 (`https://github.com/aaif-goose/goose`) |
| 빌드 전 | `node documentation/scripts/generate-acp-docs.js … vX.Y.Z` (**F15~F17**) |
| 빌드 | `cd documentation && npm ci && npm run build` (F6, F7) |
| 검증 | `./scripts/verify-build.sh` + **맵 항목 ↔ 실제 파일 대조** |
| 출력 | `goose-docs-<version>.tar.gz`, `goose-docs-<version>.tar.gz.manifest.json` |

`manifest.json` 스키마 (**P1 구현·실측값**):

```json
{
  "goose_version": "1.52.0",
  "tag": "v1.52.0",
  "commit": "302b60806639ea9f0ae8f053f49f8bf0e88b26f4",
  "generated_at": "2026-09-25T23:56:09Z",
  "variant": "site",
  "bundle": "goose-docs-1.52.0.tar.gz",
  "bytes": 361118442,
  "sha256": "d158413e…",
  "map_sha256": "b844f916…",
  "entries": 61
}
```

### 4.1.1 ACP 단계가 필수인 이유 (F15~F18)

- `docs/gdk/acp/index.md`가 `reference.md`를 링크하는데, `reference.md`는 **생성물이라 저장소에 없다**. 없으면 docusaurus가 `onBrokenLinks: throw`로 빌드를 중단한다.
- `npm run build`에는 이 생성이 포함되지 않으므로 CI와 동일하게 **빌드 전에** 실행해야 한다.
- 업스트림 헬퍼는 버전을 `releases/latest`로 해석하므로, **과거 태그를 재빌드하면 문서 버전이 어긋난다**. 우리 스크립트는 체크아웃한 태그 값을 명시적으로 넘겨 이 문제를 제거했다:
  ```
  node documentation/scripts/generate-acp-docs.js \
    crates/goose/acp-schema.json crates/goose/acp-meta.json \
    documentation/docs/gdk/acp/reference.md <tag>
  ```
- 스키마/메타는 태그 안에 존재하므로 별도 네트워크 호출이 필요 없다.

### 4.1.2 실측 결과 (v1.52.0, P1)

| 항목 | 실측값 |
|---|---|
| `npm ci` | 3초 |
| `npm run build` | 약 30초 |
| 맵 항목 | 61개 (누락 0) |
| md export | 175개 |
| **site 번들 크기** | **344.4 MB** |
| 스킬 필수분(map + `docs/*.md`) | 1.8 MB |

> ⚠️ **344MB는 서버 배포·에어갭 반입에 부담이 크다.** 원인은 스킬과 무관한 콘텐츠다:
> `assets/images` 191MB(대부분 블로그 썸네일) + `videos/` 71MB.
> P1 종료 시 사용자 결정으로 **site 전체를 그대로 유지**했으나, 다음 대안이 측정되어 있다(§10.2).
> 1. `videos/` + `assets/medias/` 제외 → **232MB**
> 2. `videos/`만 제외 → 276MB
> 3. 문서+블로그만(이미지 없이) → 6.8MB
> 4. `lean`(맵 + 61개 md) → **195KB**

### 4.2 두 가지 번들 변형

| 변형 | 내용 | 크기(예상) | 용도 |
|---|---|---|---|
| `site` (**확정, 기본**) | `documentation/build` 전체 (HTML + md + map) | 수십 MB | 사람 브라우징 + 스킬 (F9) |
| `lean` (미채택, 참고) | map + map에 나열된 60개 `.md`만 | ~1MB | 스킬 전용 최소 배포 |

- **`site` 전체를 기본이자 유일한 배포물로 확정한다(P0 결정).** HTML 브라우징과 스킬용 md가 같은 산출물에 있어 배포 경로가 하나로 끝난다.
- `lean`은 이번 범위에서 **만들지 않는다.** 필요해지면 워크플로 입력 플래그로 추가하되, 그때도 **map은 공식 `generate-docs-map.js`를 그대로 사용**해 경로 정합성을 보장한다(자체 생성기 금지).

### 4.3 주의사항 (리스크 선반영)

- `onBrokenLinks: "throw"` 이므로 링크가 깨지면 빌드 실패 → goose CI와 **동일한 순서**로 빌드한다.
- goose CI는 빌드 전 `documentation/scripts/generate-released-acp-docs.sh`(GH_TOKEN 필요)를 실행한다 → 동일 스텝 포함.
- `documentation/build`는 빌드마다 생성되며 저장소에 커밋되지 않는다(upstream `.gitignore`). 우리도 커밋하지 않고 **릴리즈 자산**으로만 배포.

**남은 위험 (P1에서 발견)**

**G1. site 번들 344MB는 서버 배포·에어갭 반입에 부담** (§4.1.2 실측)
- 원인은 스킬과 무관한 콘텐츠(`assets/images` 191MB, `videos/` 71MB).
- 결정: P1에서는 **site 전체 유지**(사용자 결정). 아래 대안이 측정되어 있으니 필요 시 전환.
- 측정된 대안: `videos/`+`assets/medias/` 제외 **232MB** / `videos/`만 제외 276MB / 문서+블로그 6.8MB / lean **195KB**.
- 전환 비용: `build-docs-bundle.sh`의 `tar --exclude` 한 줄.

**G2. 업스트림 문서 빌드가 깨지면 번들이 발행되지 않는다** (F15)
- goose 릴리즈 태그에서 `npm run build`가 실패하면 그 버전은 자동 배포되지 않는다.
- 완화: 실패 시 워크플로가 실패로 끝나고 **이전 버전 릴리즈는 그대로 남는다**(서비스 영향 없음). 수동으로 `workflow_dispatch` 재시도 가능.
- 업스트림이 ACP 스키마 파일명을 바꾸면 `build-docs-bundle.sh`의 하드코딩 경로가 깨진다 → 실패 시 그 경로만 갱신.

**G3. 깨진 앵커 경고** (치명적이지 않음)
- v1.52.0에서 `/docs/guides/remote-goose-server#3-find-the-certificate-fingerprint` 앵커 경고 1건. 빌드는 성공하며 스킬 동작에 영향 없음. 업스트림 문서 문제이므로 우리가 고치지 않는다.

---

## 5. 컴포넌트 B — `goose-doc` 실행 파일 (Rust)

### 5.0 P2 구현 현황 (완료)

```
src/
├── main.rs     # 진입점 + fetch/doctor/serve 분기
├── cli.rs      # clap 정의 (단위 테스트 포함)
├── config.rs   # 기본값, 캐시 경로, 버전 정렬
├── docs.rs     # 번들 검증/해석/다운로드/압축해제
└── server.rs   # axum 정적 서버
tests/serve_test.rs   # 실제 서버를 띄우는 HTTP 통합 테스트
fixtures/docs-root/   # CI·테스트용 최소 docs root (맵 2항목 + index.html)
tools/smoke-test.sh   # 바이너리 실행 스모크 테스트
```

- 규모: 약 1,300줄(테스트 포함), 테스트 **37개 통과**(단위 32 + 통합 5)
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 통과

**P2에서 확정·검증된 동작**

| 동작 | 결과 |
|---|---|
| 실제 번들(61페이지) 서빙 + 맵 항목 전부 200 | 통과 |
| `GOOSE_DOCS_ROOT=http://<LAN IP>:10650 goose run` → 스킬이 HTTP로 문서 읽음 | 통과 (스킬이 받아온 map이 서버 응답과 바이트 동일) |
| `.md`은 `text/plain; charset=utf-8` | 통과 |
| 없는 경로 404, HTML 폴백 없음 | 통과 |
| 경로 이탈(`/../`, `/docs/../../`, `%2e%2e`) 403 | 통과 |
| `--port 0` → 실제 포트를 출력·URL에 반영 | 통과 |
| 플래그 없이 기본 바인드로 기동 → **LAN IP로 접속 가능**(192.168.45.42에서 200) | 통과 |
| 광고되는 URL이 wildcard가 아닌 실제 IP | 통과 |
| 정지 시 포트 해제(재바인드 가능) | 통과 |

**P2에서 의도적으로 하지 않은 것**

- 관리 API를 문서 포트에 노출하지 않음 — 패널(P3)이 서버를 **프로세스 내에서** 직접 제어하므로 HTTP 관리 엔드포인트가 필요 없다. §5.4의 "별도 로컬 관리 포트" 계획은 이 결정으로 불필요해졌다(공격면 제거).
- 인증·검색·`--smoke-gui`는 P3/P6.

### 5.1 CLI 표면

```
goose-doc [OPTIONS]                 # 인자 없으면 패널 UI 실행
  --bind <IP>                       # 기본 0.0.0.0 (모든 인터페이스)
  --port <PORT>                     # 기본 10650, 0이면 임의 포트
  --docs-dir <PATH>                 # 로컬 docs root 직접 지정(에어갭)
  --docs-version <X.Y.Z>            # 특정 버전 번들 사용
  --cache-dir <PATH>                # 번들 캐시 위치
  --headless                        # 패널 없이 서빙(서버/컨테이너용)
  --open                            # 시작 후 기본 브라우저 열기
  --local-only                      # 127.0.0.1에만 바인드(이 머신 전용)
  --auth-user/--auth-pass           # (선택) Basic 인증
  --smoke-gui                       # (테스트) 창 생성 후 1프레임 렌더하고 종료

goose-doc fetch <X.Y.Z>             # 번들만 내려받아 캐시(에어갭용 사전 준비)
goose-doc service <install|uninstall|status>   # systemd / launchd / Windows 서비스
goose-doc doctor                    # 환경 점검(포트, 캐시, 번들 무결성, 디스플레이 유무)
```

### 5.2 패널 UI (요구사항 3)

패널에 반드시 포함할 요소:

1. **문서 버전** 표시 + 새 버전 확인/다운로드 버튼
2. **IP 주소** 입력(드롭다운: `0.0.0.0`(모든 인터페이스, 기본), 감지된 로컬 IP들, `127.0.0.1`, 직접 입력)
3. **포트** 입력(숫자 검증, 이미 사용 중이면 오류 표시)
4. **시작 / 중지 버튼** — 중지는 graceful shutdown, 상태는 `중지됨 / 실행 중`
5. **상태 영역**: `http://<ip>:<port>/`, docs root 경로, 문서 버전, 업타임, 요청 수
6. **goose 연결 안내**: 복사 가능한 `GOOSE_DOCS_ROOT=<url>` 문자열
7. **폴더 열기 / 브라우저로 보기** 버튼
8. 잘못된 입력은 시작 전에 차단(포트 범위, 바인드 권한, docs root 존재/무결성)

### 5.3 패널 방식 — **결정: 네이티브 창 (egui/eframe)**

> ⚠️ **전제 정정**: goose에는 GUI 크레이트가 **없다**. `crates/**/Cargo.toml`과 `*.rs` 전체를 확인한 결과 `eframe`/`egui`/`iced`/`slint`/`tauri`/`winit` 참조가 0건이다. goose 데스크톱 앱은 별도 저장소의 Electron(TypeScript)이고, goose CLI는 터미널 UI다. 따라서 egui는 **goose에서 오는 것이 아니라 우리 프로젝트가 새로 도입하는 의존성**이다.

**확정 사항**
- 패널은 **egui/eframe 네이티브 창**으로 구현한다. `goose-doc(.exe)` 더블클릭 → 창이 뜬다.
- GUI와 헤드리스는 **하나의 바이너리**로 낸다. `--headless` 플래그로 창 없이 서빙한다. → 릴리스 자산은 OS별 1개씩 유지되어 §6의 CI 최소화 원칙과 충돌하지 않는다.
- 서버(디스플레이 없는 Ubuntu)에서는 `--headless`로 실행한다. 이 경로가 곧 systemd 서비스의 기본 실행 형태다.

**버전/특성 (확인값)**

| 항목 | 값 | 출처 |
|---|---|---|
| 최신 eframe | `0.36.2` (MSRV **1.95**) | crates.io API |
| 라이선스 | `MIT OR Apache-2.0` | crates.io API |
| **기본 렌더러** | **`wgpu`** (0.36 기준) | eframe 0.36.2 features의 `default` |
| glow 렌더러 | opt-in (`glow` feature) | eframe README, features |
| `default` features | `accesskit, default_fonts, links, wayland, web_screen_reader, wgpu, winit/default, x11` | crates.io API |
| Linux 빌드 의존 패키지 | `libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev` (+ `libssl-dev`) | eframe README |
| 로컬 참고 구현 | `calcu/`(eframe 0.28.1) — `eframe::run_native` + `NativeOptions.viewport` 패턴 | 이 머신의 기존 프로젝트 |

**설계 규칙**
- 렌더러는 **`glow`** 를 명시적으로 선택한다(`default-features = false` + `glow` + `default_fonts` + `wayland` + `x11`). 이유: Linux 서버/VM의 소프트웨어 GL 및 원격 데스크톱 호환성이 wgpu보다 예측 가능하고, `wgpu`가 끌어오는 의존성 트리가 더 크다. wgpu가 필요하면 피처로 전환한다(§10).
- GUI 모듈은 피처 게이트 없이 **항상 컴파일**한다(단일 바이너리 원칙). 대신 Linux CI에서 필요한 시스템 패키지를 설치한다(§6).
- egui 기본 폰트에는 CJK 글리프가 없다. 패널 UI 문구는 **영어**로 작성한다. 한국어 라벨이 필요하면 §5.8 참고.

### 5.3.1 무디스플레이 Linux에서의 동작 (중요)

- `--headless` 없이 GUI 실행을 시도하면 winit이 디스플레이를 찾지 못해 **실패한다.** 이는 버그가 아니라 설계 의도이며, 서버 배포 문서에 명시한다.
- 판별 로직: `DISPLAY`/`WAYLAND_DISPLAY`가 없으면 **자동으로 헤드리스 폴백**하고 로그로 알린다(서비스 등록 시 사고 방지).
- CI에서 실제 창 생성까지 확인하려면 `xvfb-run`을 쓴다(§6).

### 5.4 서버 동작 (P2 구현 확정)

- **자체 핸들러**로 docs root 정적 서빙(`tower-http::ServeDir` 미사용). 이유: 경로 이탈 거부와 404 정책을 명시적으로 통제하기 위함.
  - `.md` → `text/plain; charset=utf-8` (goose 스킬이 원문을 읽어야 함)
  - `.html` → `text/html; charset=utf-8`
  - **SPA 폴백 없음** → 없는 파일은 404 + 짧은 평문 본문(HTML 셸 없음)
  - 경로 이탈: 절대경로/`..` 컴포넌트를 **거부(403)**. URL 퍼센트 디코딩 후에도 동일하게 검사하며, 잘못된 인코딩은 400
  - 디렉터리는 `index.html`로 해석
- 헬스체크: `GET /healthz` → `200 "ok"` (관리 API 아님)
- **HTTP 관리 API를 두지 않는다(P2 결정).** 패널(P3)이 같은 프로세스에서 `server::start`/`RunningServer::stop`을 직접 호출하므로 원격 start/stop 엔드포인트가 불필요하다. §5.4 초안의 "별도 로컬 관리 포트"는 이 결정으로 폐기했다 — 문서 포트에 인증 없이 상태 변경 경로가 존재하지 않는다.
- **기본 바인드 `0.0.0.0`(모든 인터페이스).** 서버에 배포해 다른 PC가 문서를 받는 것이 이 도구의 목적이므로, 기본이 루프백이면 무의미하다. `--local-only`로 이 머신 전용으로 제한한다.
- `0.0.0.0`은 클라이언트가 접속할 수 없는 주소이므로 **광고되는 URL(`GOOSE_DOCS_ROOT` 값)에는 감지된 LAN IP를 대신 넣는다**(LAN IP가 없으면 루프백). 접속 불가능한 주소를 그대로 노출하지 않기 위함이다.
- 인터페이스 IP는 `if-addrs`로 열거한다. `addr::rank`가 사설망(192.168/10/172.16) → CGNAT(100.64/10) → 공인 → 링크로컬 순으로 정렬해 **LAN 주소가 먼저** 오게 한다.
- 서버는 **메인 스레드가 아닌 tokio 런타임**에서 돌고, GUI(P3)는 `RunningServer` 핸들을 통해 시작/중지를 지시한다. 핸들은 `uptime()`·`request_count()`·`is_running()`을 제공하므로 패널 상태 표시에 그대로 쓴다.

### 5.4.1 구현된 CLI (P2)

```
goose-doc [OPTIONS]                    # 인자 없으면 서빙(패널은 P3)
  --bind <IP>        기본 0.0.0.0 (모든 인터페이스)
  --port <PORT>      기본 10650, 0이면 임의 포트
  --docs-dir <PATH>  로컬 docs root 직접 지정
  --docs-version <V> 캐시된 특정 버전 사용
  --cache-dir <DIR>  번들 캐시 위치
  --headless         패널 없이 서빙
  --open             시작 후 브라우저 열기
  --local-only       127.0.0.1에만 바인드

goose-doc fetch <VERSION> [--base-url URL] [--cache-dir DIR]
goose-doc doctor  [--docs-dir PATH] [--docs-version V] [--cache-dir DIR]
```

- `doctor`: docs root 검증 결과·페이지 수·`GOOSE_DOCS_ROOT` 출력. 실패 시 종료코드 1.
- `fetch`: goose 버전 태그 릴리즈에서 번들+manifest를 받아 **sha256 검증 후** 캐시에 압축 해제. 비공개 저장소는 GitHub API로 asset을 해석하고 `GH_TOKEN`(또는 `--token`)을 쓴다. 압축 해제 시 절대경로·`..` 항목은 건너뛴다.
- 기본 캐시: OS 캐시 디렉터리 `goose-doc/bundles/<version>`.
- 캐시가 비어 있고 `--docs-dir`도 없으면 **명확한 에러**로 안내한다(§5.1 초안의 "최신 캐시 자동 선택"은 캐시가 있을 때만 동작).

### 5.5 설정 영속화

- 표준 config 디렉터리(`dirs` crate) 사용: `goose-doc/config.yaml`
- 저장 항목: `bind`, `port`, `docs_version`, `last_started_at` — **비밀값은 저장하지 않음**
- 시작 시 저장된 설정을 prefill

### 5.6 런타임 의존성 최소화

- `reqwest`는 `rustls-tls` 사용(Windows에서 OpenSSL 불필요)
- 압축 해제는 `tar` + `flate2`(순수 Rust)
- egui/eframe은 **단일 바이너리 원칙**에 따라 항상 포함한다(피처 게이트 없음). 대신 렌더러를 `glow`로 지정해 `wgpu` 의존성 트리를 회피한다:

```toml
[dependencies]
eframe = { version = "0.36", default-features = false, features = [
  "glow", "default_fonts", "wayland", "x11"
] }
```

- 프로파일에서 크기 최적화: `[profile.release] lto = true`, `strip = true`, `codegen-units = 1`
- eframe 0.36의 MSRV는 **1.95**이므로 `rust-toolchain.toml`은 1.95 이상으로 고정한다(goose 저장소의 1.96.1과 동일하게 맞춰도 무방)
- GUI 툴킷은 goose에서 오는 것이 아니라 우리가 선택하는 의존성이다(§5.3 전제 정정 참고)

### 5.7 egui 채택에 따른 작업 범위 (수용한 비용)

egui는 goose에 없으므로 우리가 책임지는 항목이 늘어난다. 아래는 결정에 포함된 비용이다.

| 비용 | 내용 | 완화 |
|---|---|---|
| Linux 시스템 패키지 | 빌드 시 `libxcb-*-dev`, `libxkbcommon-dev` 필요 | §6 워크플로 B에 설치 스텝 1줄. 서버 **런타임**은 `--headless`면 GL 라이브러리 불필요 |
| 바이너리 크기 | GUI 포함으로 증가 | `glow` 선택으로 wgpu 트리 회피, `lto = true` + `strip = true` (§5.6) |
| 창 열림 검증 | 헤드리스 CI에서 창을 못 띄움 | `xvfb-run ./goose-doc --smoke-gui` 스텝 추가(§6) |
| 폰트 | egui 기본 폰트에 CJK 없음 | UI 문구는 영어. 한국어 필요 시 §5.8 |
| 창 생성 스레드 | winit 이벤트 루프는 메인 스레드 고정 | 서버는 별도 tokio 런타임 스레드에서 기동, 종료는 채널로 전달 |

### 5.8 폰트 / 패널 UI 언어 — **결정: 영어**

- **패널 UI 문구는 영어로 확정한다(P0 결정).** egui 기본 폰트에는 CJK 글리프가 없어 한국어는 폰트를 별도로 넣어야 한다.
- 한국어가 필요해지면 `egui::FontDefinitions`에 시스템 CJK 폰트를 등록한다. 이 머신의 `calcu/`가 이미 폴백 패턴을 쓴다: macOS `AppleSDGothicNeo.ttc`, Linux `NotoSansCJK-Regular.ttc`, Windows `C:\Windows\Fonts\malgun.ttf`.
- 서버 이미지에는 폰트가 없을 수 있으므로 도입 시 **폰트 파일을 번들에 포함**(라이선스 확인 필요)해야 한다. 지금은 하지 않는다.
- 폰트를 찾지 못하면 기본 폰트로 폴백하고 크래시하지 않는다(글자는 깨질 수 있음).

### 5.9 GUI ↔ 서버 상호작용

- 서버는 GUI가 소유한 별도 스레드에서 실행된다. `start`/`stop`은 채널(예: `tokio::sync::mpsc`)로 전달한다.
- GUI는 `ctx.request_repaint_after(Duration::from_millis(250))`로 상태를 폴링해 표시한다.
- 포트를 `0`으로 지정하면 바인드 후 실제 포트를 `local_addr()`로 얻어 화면에 표시한다.
- 종료 순서: `stop` 요청 → axum graceful shutdown → 스레드 join → 창 종료. 창을 닫아도 서버는 함께 정지한다(고아 프로세스 방지).

---

## 6. 컴포넌트 C — GitHub Actions (CI/CD 최소화)

**원칙**
- 워크플로 **2개**. 그 이상 늘리지 않는다.
- goose 저장소에 트리거/PR/워크플로 추가 금지 → **우리 워크플로가 goose를 읽기만** 한다. (요구사항 4, F13)
- 행렬 확장, 야간 빌드, 자동 머지, 의존성 자동 업데이트는 도입하지 않는다.

### 워크플로 A — `release.yml` (번들 + 앱 통합)

**릴리즈 단위 = goose 버전.** 하나의 태그에 문서 번들과 3개 플랫폼 실행 파일이 함께 있다.

```yaml
on:
  workflow_dispatch:            # 입력: goose_version (생략 시 latest)
  schedule:
    - cron: "0 0,4,8,12,16,20 * * *"   # 1일 6회(4시간 간격, UTC)
permissions: { contents: write, actions: read }
concurrency: { group: release, cancel-in-progress: false }
jobs:
  resolve:   # 버전 확정 + 릴리즈가 이미 있으면 skip=true
  bundle:    # ubuntu 1회: docs 번들 생성 → artifact (skip이면 실행 안 함)
  app:       # build-app.yml 재사용 (win / linux / macos-arm64)
  publish:   # 번들 + 실행파일 + SHA256SUMS → 릴리즈 생성
```

- **릴리즈 태그 = `<goose_version>`** (예: `v1.52.0`). 이전의 `docs-v*`/`app-v*` 분리는 폐기했다.
- **멱등성**: `resolve`가 릴리즈 존재를 확인하고 `skip=true`를 내면 `bundle`/`app`/`publish`가 모두 건너뛴다. 1일 6회 실행해도 **344MB 번들을 매번 다시 만들지 않는다.**
- 감지 방식은 폴링만 쓴다. `repository_dispatch`는 외부에서 트리거를 걸어야 하므로 "goose 저장소에 아무것도 걸지 않는다"는 요구사항 4와 충돌한다.
- (참고: GitHub `schedule`은 장기 미활동 시 비활성화될 수 있다. §10.4의 확인 명령으로 주기 점검.)

### 워크플로 B — `build-app.yml` (P4 구현 완료)

```yaml
on:
  workflow_dispatch:
  workflow_call:            # release.yml이 재사용
    inputs:
      upload_artifacts: { default: true, type: boolean }
permissions: { contents: read }
strategy:
  fail-fast: false
  matrix:
    include:
      - { name: linux-x86_64,   os: ubuntu-latest,  target: x86_64-unknown-linux-gnu }
      - { name: macos-arm64,    os: macos-14,       target: aarch64-apple-darwin }
      - { name: windows-x86_64, os: windows-latest, target: x86_64-pc-windows-msvc }
steps:
  checkout
  dtolnay/rust-toolchain@1.96.1 (targets + rustfmt + clippy)   # rust-toolchain.toml과 동일
  Swatinem/rust-cache@v2 (key: target)
  [Linux만] apt: libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo build --release --target <target>
  cargo test  --target <target>
  tools/smoke-test.sh target/<target>/release/goose-doc     # 실제 바이너리 실행 검증
  [Linux만] 디스플레이 없이 패널 실행 → "Serving goose docs at" + 패널 안내 확인
  패키징: dist/goose-doc-<name>[.exe] + .sha256
  actions/upload-artifact@v7.0.1
```

- **크로스 컴파일 없음** → 각 러너에서 네이티브 빌드. 실패가 진짜 플랫폼 실패가 된다.
- `fail-fast: false`로 3개 플랫폼 결과를 모두 확인한다.
- 스모크 테스트는 `tools/smoke-test.sh`이며 **Git Bash(Windows)에서도 동작**하도록 작성했다: `curl.exe` 폴백, `seq` 미사용, `sha256sum`/`shasum` 양쪽 지원, `disown`으로 종료 시 "Terminated" 잡 알림 억제.
- **macOS는 `aarch64`만**(P0 결정). 자산 3종.
- `--smoke-gui` 플래그는 도입하지 않았다. GUI 창 검증은 로컬 macOS에서 수동으로 확인했고(§8 P3), CI에서는 **디스플레이 없을 때 헤드리스로 폴백하는지**만 검사한다(xvfb 불필요 → CI 최소화).

---

## 7. 사용 흐름

### 7.1 서버 운영자 (Ubuntu 서버, 예)

```bash
# 1) 릴리스 자산 다운로드 후 해제
tar xzf goose-doc-0.1.0-x86_64-unknown-linux-gnu.tar.gz
# 2) 서버 모드로 실행 (GUI 없음)
./goose-doc --headless --port 10650        # 기본이 모든 인터페이스
# 3) 영구 등록
sudo ./goose-doc service install
```

Windows/macOS는 더블클릭 → 패널에서 IP/포트 설정 → 시작.

### 7.2 goose 사용자 (문서 소비자)

```bash
# 방법 1) HTTP 호스팅 사용
export GOOSE_DOCS_ROOT=http://docs.internal:10650

# 방법 2) 같은 호스트에서 로컬 경로로 사용 (네트워크 불필요)
export GOOSE_DOCS_ROOT=/opt/goose-docs

# 이후 goose 실행 → goose-doc-guide 스킬이 해당 root에서 문서를 읽음
```

(config.yaml에 `GOOSE_DOCS_ROOT` 기입도 동일하게 동작 — F2, F3)

---

## 8. 단계별 계획

| 단계 | 산출물 | 완료 기준(Acceptance) | 예상 |
|---|---|---|---|
| **P0** 결정 | 결정 기록 | GUI 방식(egui), 기본 포트 `10650`, 번들 변형(`site`만), 릴리즈 자산 위치 확정 | 완료 |
| **P1** 번들 파이프라인 | `tools/build-docs-bundle.sh`, `tools/verify-docs-root.sh`, `docs-bundle.yml` | **✅ 완료** — v1.52.0으로 번들 생성(344.4MB, 맵 61항목), manifest 발행, 추출 경로에서 `GOOSE_DOCS_ROOT` 지정 시 스킬이 오프라인으로 문서 읽음(검증 통과) | 완료 |
| **P2** 코어 + 헤드리스 서버 | `src/{main,cli,config,docs,server}.rs`, `tests/serve_test.rs`, `fixtures/docs-root/`, `tools/smoke-test.sh` | **✅ 완료** — 테스트 37개 통과, fmt/clippy clean. 실제 번들(61페이지) 서빙·`GOOSE_DOCS_ROOT=http://…`로 스킬 동작·404/경로이탈/원격바인드/포트해제 검증 | 완료 |
| **P3** 패널 UI (egui) | `panel/mod.rs`, `panel/app.rs`, `config.rs`, `fonts.rs` | 3개 OS에서 더블클릭 → 창 → 설정 → 시작/중지/상태/URL 복사 동작. Linux는 `xvfb-run --smoke-gui` 통과 | 2~3일 |
| **P4** 크로스 OS CI | `build-app.yml`, `release.yml`, `smoke-test.sh` | **✅ 완료** — `soolmuk/goose-doc`에서 **3개 플랫폼(x86_64 linux/win, macos arm64) green**. 각 플랫폼에서 빌드·테스트 71개·스모크 테스트 실측 통과, 아티팩트 3종 업로드 | 완료 |
| **P5** 릴리스/서비스 | `release.yml`, `src/service.rs`, `tools/check-linux.sh` | **✅ 완료** — 3 OS 자산 릴리스 발행(`app-v0.1.0`), launchd 서비스 실등록·서빙 검증, 시스템/사용자 systemd·launchd·Windows 정의를 **플랫폼 무관 테스트**. 테스트 89개 | 완료 |
| **P6** 선택 | Basic 인증, 검색 | 필요 시 | — |

---

## 9. 리스크와 대응

| 리스크 | 영향 | 대응 |
|---|---|---|
| Linux 서버에 디스플레이 없음(헤드리스) | GUI 실행 실패 | `DISPLAY`/`WAYLAND_DISPLAY` 미검출 시 **자동 헤드리스 폴백**, systemd 기본 실행도 `--headless`, `doctor`에서 사전 진단 (§5.3.1) |
| Linux 빌드에 GUI 시스템 패키지 필요 | CI 빌드 실패 | 워크플로 B에 `libxcb-*-dev`/`libxkbcommon-dev` 설치 스텝 1줄 (§6) |
| `onBrokenLinks: throw`로 문서 빌드 실패 | 번들 생성 중단 | goose CI와 **동일 순서**(ACP 문서 생성 → npm ci → build → verify) 재현, 실패 시 이전 정상 버전 유지 |
| goose 버전과 문서 버전 불일치 | 오답 유발 | 릴리즈 태그로 고정(F6). 매니페스트에 `goose_version` 기록, 패널에 표시 |
| 344MB 번들 다운로드(에어갭) | 배포 불편 | `fetch` 명령으로 사전 반입, `--docs-dir` 지원, 릴리스 자산으로 반입 (lean 변형은 미채택) |
| 릴리즈가 커짐(번들 344MB + 실행파일 3종) | 다운로드 부담 | 릴리즈 하나에 모아 태그 관리 단순화. 필요한 자산만 `gh release download -p`로 선택 다운로드 |
| 문서 재배포 라이선스 | 법적 | Apache-2.0 고지(NOTICE, README, UI에 upstream 출처·버전·라이선스 표기) (F11) |
| 포트 충돌 | 시작 실패 | `--port 0`(임의 포트) 지원, 사용 중 포트는 UI에서 오류 표시 |
| 네트워크 노출 | 사내 열람 | **기본이 모든 인터페이스인 것은 의도된 동작**(서버 호스팅이 목적). 제한은 `--local-only`, 선택적 Basic 인증(P6). 노출되는 내용은 공개 문서(Apache-2.0)뿐 |
| Windows SmartScreen / macOS Gatekeeper | 실행 차단 | README에 안내(`xattr -d com.apple.quarantine`, "추가 정보 → 실행"), 코드 서명은 P6 |
| 관리 API 노출 | 보안 | 문서 포트와 분리된 로컬 전용 관리 포트, 원격 바인드 시 관리 API 비활성 |

---

## 10. 확정 사항 (P0 — 전 항목 결정 완료)

| # | 항목 | 결정 | 근거 |
|---|---|---|---|
| 1 | 패널 방식 | **egui/eframe 네이티브 창** (단일 바이너리 + `--headless`) | 사용자 결정. §5.3 |
| 2 | 렌더러 | **`glow`** (기본 `wgpu` 대신), `default-features = false` | Linux 소프트웨어 GL/원격 데스크톱 예측성, 의존성 절감. §5.6 |
| 3 | Rust 툴체인 | **1.95 이상** 고정 (eframe 0.36 MSRV) | crates.io eframe 0.36.2 |
| 4 | 기본 포트 | **10650** (0이면 임의 포트) | 사용자 결정 |
| 5 | 기본 바인드 | **`0.0.0.0` (모든 인터페이스)** — 다른 PC가 접속할 수 있어야 서버 호스팅이 성립 | 요구사항 5(서버 실행). 제한은 `--local-only` |
| 6 | 번들 변형 | **`site` 전체만** (HTML + md + map). `lean` 미채택 | 배포 경로 단일화. §4.2 |
| 7 | 번들 배포 위치 | **goose-doc 저장소 릴리즈 자산, goose 버전 태그 하나에 번들+실행파일 통합** | 사용자 결정(변경). §6-A |
| 8 | goose 신버전 감지 | **1일 6회 cron** (4시간 간격). `repository_dispatch` 미사용 | 사용자 결정 + 요구사항 4. §6-A |
| 9 | macOS 아키텍처 | **`aarch64`만** (Intel은 범위 제외) | 권장안. `x86_64`는 `rustup target add` 한 줄로 추가 가능. §6-B |
| 10 | 패널 UI 언어 | **영어** | egui 기본 폰트에 CJK 없음. §5.8 |
| 11 | 워크플로 수 | **2개** (릴리스 / 앱 빌드 재사용) | 요구사항 4. 통합으로 3→2 |

미결정 항목 없음. P1부터 구현 착수 가능.

---

## 10.1 변경 이력

- **2026-09-26**: P0 확정(포트 10650, 번들 `site` 단일, 릴리즈 자산 배포, cron 1일 6회, macOS aarch64만, 영어 UI). 패널 방식을 egui 네이티브 창으로 확정하고 관련 절(§1, §3.1, §5.1, §5.3, §5.6~5.9, §6, §9, §11) 갱신.
- **2026-09-26 (P1 완료)**: `tools/build-docs-bundle.sh`, `tools/verify-docs-root.sh`, `.github/workflows/docs-bundle.yml`, `README.md` 작성. v1.52.0 실측으로 사실 F15~F21 추가, §4.1/§4.1.1/§4.1.2 갱신, 신규 리스크 G1~G3 등록. **site 번들이 344MB(스킬 필수분 1.8MB)임을 확인** — 대안 크기 측정치를 §4.1.2에 기록.
- **2026-09-26 (Windows 단일 exe 실패 → 패널 내장 폴백 누락 수정)**: 사용자가 배포된 `goose-doc-windows-x86_64.exe`를 실행해 **Start를 눌렀으나 실패**했다: `no docs bundles in C:\Users\...\AppData\Local\goose-doc\bundles. Run \`goose-doc fetch <version>\` or pass --docs-dir.` **원인은 환경이 아니라 코드다.** P2에서 `docs::resolve(..., allow_embedded)`를 도입하며 `main.rs`의 서브 경로(`--headless`, `doctor`)만 내장 폴백을 쓰도록 바꾸고, **패널은 옛 `resolve_cached` 직접 호출을 그대로 두었다**(`panel/mod.rs::resolve_docs`). 그 결과 **다운로드한 exe 하나로는 Start가 절대 성공할 수 없었다** — `--docs-dir`도 없고 캐시도 비어 있고, 내장 사본은 조회되지 않았다. `--headless`만 동작했기 때문에 CI(§10.3의 "빈 캐시 서빙" 스텝은 `--headless` 사용)와 로컬 검증이 이 경로를 **전부 비껴갔다**. 수정: 패널이 `docs::resolve(.., true)`를 호출하도록 통일하고, 패널 상태 모델에 **GUI 없이 검증 가능한 회귀 테스트 2건**을 추가 — (1) 빈 캐시·`--docs-dir` 없음에서 Start가 내장 문서로 서빙, (2) Start 후 **맵과 맵이 나열한 61개 페이지를 전부 HTTP로 조회해 200 확인 + 없는 경로는 404**. 부수 수정: 패널 Docs root 라벨이 실제와 무관하게 "cached bundle"로 표시되던 것을 `describe_docs_location`으로 교체(빈 캐시에서 "embedded in the binary (1.52.0)"), `doctor`가 내장 사본일 때 `GOOSE_DOCS_ROOT=`(빈 값)을 출력하던 것을 안내 문구로 교체. **교훈: 대체 해석 경로를 추가할 때는 그 경로를 쓰는 모든 진입점을 함께 점검한다. 사용자가 클릭하는 경로(패널)가 CI가 검증하는 경로(--headless)와 다르면 CI는 아무것도 보증하지 않는다.** CI 스모크 테스트가 `--docs-dir`만 써서 **Windows에서 "exe 단독 서빙" 경로가 미검증**이었다. `build-app.yml`에 "번들·`--docs-dir` 없이 빈 캐시로 서빙" 스텝을 추가 → **3개 플랫폼 모두 `unique page links: 61`** 확인. (스텝 구현 중 맵 링크 정규식이 줄 시작을 요구해 카운트가 0으로 나오던 버그를 발견해 수정.) README에 플랫폼별 시작 안내(Windows: 콘솔 창 동반, `--headless`, `service install`, 방화벽, SmartScreen) 추가.
- **2026-09-26 (단일 파일 배포)**: 사용자 요구("exe 하나면 되는 것")에 따라 문서를 **바이너리에 내장**했다. `src/embedded`(760KB, 61페이지)를 `include_dir!`로 컴파일 시 포함 → 다운로드 없이 서빙. 해석 순서는 `--docs-dir` > `--docs-version` > 캐시 > **내장**. `fetch --variant lean|site`(기본 lean 190KB) 추가. 릴리즈에 lean(190KB) + site(344MB) + 실행파일 3종을 함께 발행. **주의: site 344MB는 HTML 브라우징(블로그 이미지·동영상) 전용이며 스킬 기능에는 불필요.**
- **2026-09-26 (내장 크기 사고와 가드)**: CI의 embed 단계가 `find ... | head -1`로 번들을 골라, **Linux/macOS는 site(360MB)를 내장해 330MB 바이너리**가 나왔다(Windows는 lean을 골라 정상). 재현 후 수정: `*-lean.tar.gz`를 명시 선택, `embed-docs.sh`가 32MB 초과 번들을 **거부**, 단위 테스트가 내장 문서 4MB 상한을 검사, CI가 **바이너리 64MB 상한**을 검사. 수정 후 24/17/15MB.
- **2026-09-26 (릴리즈 구조 변경)**: 릴리즈를 **goose 버전 기준 하나로 통합**했다. `release.yml`이 `docs-bundle.yml`을 흡수해 번들과 3개 실행 파일을 같은 태그(`v<goose_version>`)에 발행한다. `resolve` 잡이 기존 릴리즈를 확인해 **skip**하므로 1일 6회 실행에도 번들을 재빌드하지 않는다. 워크플로 3→2개. `fetch`의 릴리즈 태그도 `docs-v<ver>`→`v<ver>`로 변경. 실측: `v1.52.0` 릴리즈에 번들+실행파일 3종+SHA256SUMS, fetch/서빙/크롤링 통과, 재실행 시 `already exists; nothing to do.`
- **2026-09-26 (P5 검증)**: 스케줄/자동 감지/멱등성 실측 완료(§10.4). 입력 없는 실행이 `v1.52.0`을 자동 감지했고, 재실행은 멱등하게 스킵했으며, v1.51.0으로 신규 번들 발행 → fetch → 서빙까지 버전별로 다른 문서가 제공되는 것을 확인했다. **cron 자체의 자동 실행은 아직 시각이 지나지 않아 미확인**(§10.4).
- **2026-09-26 (P5 완료)**: `src/service.rs`(systemd 사용자/시스템, launchd, Windows `sc.exe`), `fetch`의 **비공개 저장소 지원**(GitHub API로 asset 해석) 과 **스트리밍 다운로드**(344MB를 메모리에 올리지 않음), `tools/check-linux.sh`(Linux 컨테이너에서 CI 동일 검증). 릴리스 `app-v0.1.0` 발행(3 OS + SHA256SUMS), 번들 `docs-v1.52.0` 발행. CI가 **컴파일 결함 2건**(Linux/Windows에서 각각 cfg로 인한 미사용 경고)을 잡아냈고, 이를 계기로 서비스 정의를 순수 함수로 분리해 **단일 테스트로 3플랫폼을 검증**하도록 구조를 바꿨다. **launchd 서비스를 실제 등록해 서빙까지 확인.**
- **2026-09-26 (P4 완료)**: `build-app.yml`(3 OS 매트릭스), `release.yml` 추가, `tools/smoke-test.sh`를 Windows Git Bash 대응으로 재작성(disown, seq 제거, curl/체크섬 폴백). 비공개 저장소 **`soolmuk/goose-doc`** 생성 후 CI 3회 실행 → **3개 플랫폼 green**(run 36206440329), 아티팩트 3종. CI가 로컬에서 놓친 결함 **2건(D1 fmt, D2 아티팩트 입력)** 을 잡아냄 → §10.3. CI가 빌드한 Linux 바이너리로 실제 344MB 번들 서빙 확인.
- **2026-09-26 (P3 완료)**: `panel/mod.rs`(GUI 비의존 상태 모델), `panel/app.rs`(eframe), `settings.rs`(영속화), `addr.rs` 추가. **실제 macOS GUI 창을 띄워 Start/Stop/창닫기를 클릭으로 검증**(스크린샷). eframe 0.36 API 변경(`App::ui`, `egui::Panel::top`)을 소스에서 확인해 반영. `--smoke-gui`는 도입하지 않음(xvfb 불필요).
- **2026-09-26 (P2 완료)**: Rust 크레이트 구현(`src/`, 약 1,300줄, 테스트 37개), `fixtures/docs-root/`, `tools/smoke-test.sh`, `tests/serve_test.rs` 추가. **HTTP 관리 API를 두지 않는 것으로 결정**(패널이 프로세스 내에서 직접 제어) → §5.4 갱신. `--docs-dir`/`--docs-version`/`fetch`/`doctor` 구현. G1은 사용자 결정으로 **그대로 유지**.

## 10.2 미해결 검토 항목

| ID | 항목 | 상태 |
|---|---|---|
| G1 | site 번들 344MB 축소 여부 | **결정: 그대로 유지** (사용자 결정, 2026-09-26). 대안 크기는 §4.1.2에 기록해 두었고 전환은 `tar --exclude` 한 줄 |
| — | 패널에 문서 버전/페이지 수 표시 | 완료 (P3: `RunningServer`가 `docs_version`·`docs_entries` 제공) |
| — | 번들 다운로드가 비공개 저장소에 토큰을 요구 | 완료 (P5: `--token`/`GH_TOKEN`, GitHub API로 asset 해석) |

---

## 10.3 P4 검증 한계 (반드시 인지)

| 항목 | 상태 |
|---|---|
| `actionlint` 3개 워크플로 | ✅ 0 errors (shellcheck 규칙 포함) |
| `shellcheck -S warning` `tools/*.sh` | ✅ 0 issues |
| **macOS**: 빌드 + 테스트 71개 + 스모크 테스트 | ✅ 실측 통과 |
| **Linux(Ubuntu 24.04 컨테이너)**: 빌드 + 테스트 71개 + 스모크 테스트 + 디스플레이 없을 때 폴백 | ✅ 실측 통과 |
| **Windows**: 빌드 + 테스트 + 스모크 테스트 | ✅ **실측 통과** (GitHub Actions, Git Bash) |
| **GitHub Actions 실제 실행** | ✅ **3개 플랫폼 green** (`soolmuk/goose-doc` run 36206440329) |
| CI 산출물 3종 아티팩트 업로드 + 체크섬 검증 | ✅ linux-x86_64 / macos-arm64 / windows-x86_64 |
| CI가 빌드한 Linux 바이너리로 **실제 344MB 번들 서빙** | ✅ map/index/CSS 모두 200 |

`actionlint`는 문법·표현식·액션 입력을 검사하지만 실행 결과를 보장하지 않는다. 실제 러너에서 아래가 확인되었고, **CI가 로컬 검증에서 놓친 결함 2건을 잡아냈다.**

### CI가 잡아낸 결함 (로컬 검증 누락)

**D1. `cargo fmt --check` 실패 (3개 플랫폼 전부)**
- `src/cli.rs`의 `DEFAULT_FETCH_BASE_URL` 상수를 fmt 적용 후 커밋했다. 로컬에서 fmt를 돌린 **뒤에** 수정해 재확인을 빠뜨렸다.
- 교훈: 커밋 전 `cargo fmt --check`를 **마지막** 단계로 둔다.

**D2. `upload_artifacts` 입력이 `workflow_dispatch`에 선언되지 않아 아티팩트가 업로드되지 않음**
- `workflow_call`에만 선언된 입력은 dispatch 실행 시 **빈 문자열**이 되고, `if: inputs.upload_artifacts != false`는 빈 문자열을 `false`와 같다고 보지 않아 조건이 거짓이 됐다.
- 아티팩트 목록이 비어 있는 것을 보고 발견했다.
- 수정: 입력을 양쪽 트리거에 선언하고 조건을 `inputs.upload_artifacts == true || inputs.upload_artifacts == ''`로 명시.

### P5에서 CI가 잡아낸 결함 (플랫폼별 빌드)

**D3. Linux에서 `cargo clippy` 실패 — macOS 전용 함수가 미사용**
`escape_xml`(launchd 전용)이 Linux에서 `dead_code`로 걸렸다. macOS에서만 빌드했기 때문에 로컬에서 보이지 않았다.

**D4. Windows에서 `cargo clippy` 실패 — 같은 부류 2건**
`hostname`, `config_home`이 Windows에서 미사용으로 걸렸다. `#[cfg]`로 변수를 조건부 선언하던 구조가 원인이었다.

**대응 (구조 변경)**
플랫폼별 서비스 정의를 **순수 함수**(`systemd_plan`, `launchd_plan`, `windows_plan`)로 분리하고 **모든 플랫폼에서 컴파일**되게 했다. 결과: 한 번의 테스트 실행으로 3개 정의를 모두 검증하므로, cfg 실수를 3개 러너에 의존하지 않고 즉시 찾는다. 부수적으로 `tools/check-linux.sh`를 추가해 Linux 검증을 로컬에서 재현할 수 있게 했다(D3이 이 스크립트로 즉시 재현됐다).

**교훈**: `#[cfg]`로 변수를 조건부 선언하지 말고, 조건부 로직을 분리해 전 플랫폼에서 컴파일·테스트한다.

### 사용자 실행에서 발견된 결함 (CI가 검증하지 않는 경로)

**D5. 패널의 Start가 내장 문서를 찾지 못함 — 배포된 exe 단독 실행이 불가능**
- 배포된 `goose-doc-windows-x86_64.exe`에서 **Start 클릭 시 실패**: `no docs bundles in ...\bundles. Run \`goose-doc fetch <version>\` or pass --docs-dir.`
- 원인: 내장 폴백(`docs::resolve(.., true)`)을 `--headless`/`doctor` 경로에만 연결하고 **패널은 캐시 전용 해석을 그대로 유지**했다. 즉 **exe 하나만 받은 사용자가 누르는 버튼이 유일하게 실패하는 경로**였다.
- **왜 CI가 놓쳤나**: §10.3의 "빈 캐시 서빙" 스텝과 `--headless` 검증은 모두 **패널을 거치지 않는다**. 게다가 로컬 P3 GUI 검증은 `--docs-dir`(픽스처)로 했기 때문에 캐시·내장 해석 자체를 타지 않았다. **사용자의 클릭 경로와 CI의 검증 경로가 달랐던 것이 근본 원인**이다.
- 수정: 패널도 `docs::resolve(.., true)`를 쓰도록 통일. 회귀 테스트 2건을 **GUI 없이** 추가(빈 캐시에서 Start → 내장 서빙 / Start 후 맵 + 맵이 나열한 61개 페이지 전부 HTTP 200, 없는 경로 404). CI의 "빈 캐시" 스텝도 **패널 상태 기계를 직접 구동**하도록 추가해 같은 부류가 다시 나오지 않게 했다.
- **교훈**: 사용자가 실제로 누르는 경로가 CI가 두드리는 경로와 같아야 한다. 대체 해석 경로(내장/캐시/명시)를 추가하면 **모든 진입점**(패널·`--headless`·`doctor`·`service`)을 함께 점검한다.

**다음 단계**: Windows/macOS의 컴파일 문제는 이 구조 변경으로 해소됐고, 3개 플랫폼이 green이다.

---

## 10.4 스케줄·자동 감지·멱등성 검증 (P5)

| 항목 | 결과 |
|---|---|
| 워크플로 3개 `state` | ✅ 모두 `active` (`gh api .../actions/workflows`) |
| cron 등록 | ✅ `0 0,4,8,12,16,20 * * *` (1일 6회, 4시간 간격) |
| **입력 없이 실행 → 자동 감지** | ✅ 로그 `Resolved goose release: v1.52.0` (입력 생략 시 `releases/latest` 폴링) |
| **멱등성** | ✅ 재실행 로그 `Asset already published for v1.52.0; nothing to do.`, 릴리즈 중복 없음 |
| **새 버전 감지 → 번들 발행** | ✅ v1.51.0으로 동일 경로 실행 → 신규 릴리즈 `docs-v1.51.0` 생성(344MB + manifest) |
| 발행된 번들의 **fetch → 서빙** | ✅ v1.51.0/v1.52.0 각각 다운로드·검증 후 서빙 |
| **버전 선택 로직** | ✅ 미지정 → 최신(1.52.0), `--docs-version 1.51.0` → 1.51.0, `v1.51.0` 접두사 허용 |
| **버전별 문서 차이 반영** | ✅ 서빙된 map sha가 버전별로 다름(1.51.0 `4e1a6b95…` / 1.52.0 `0df3aada…`). 1.52.0에 `Z.AI Coding Plan` 항목 추가 확인 |
| 미출시 버전 요청 | ✅ `release docs-v1.99.0 not found … set GH_TOKEN` (명확한 안내) |
| **cron 실제 실행 이력** | ✅ **자동 실행 확인** — `2026-09-26T04:20:11Z` `event=schedule` 실행이 `success` (예정 04:00 UTC / 13:00 KST). `total_count: 1` |

**cron 잔여 확인 방법**: `gh api "repos/soolmuk/goose-doc/actions/workflows/release.yml/runs?event=schedule" --jq '.total_count'` 가 1 이상이면 자동 실행이 동작한 것이다. GitHub은 저장소 활동이 장기간 없으면 scheduled 워크플로를 비활성화하므로, 주기적으로 이 값과 워크플로 `state`를 확인한다.

---

## 11. 검증 체크리스트 (P1~P5 종료 시)

**P1 (완료분 — 실측 통과)**

- [x] goose 릴리즈 태그(v1.52.0)에서 번들 생성 성공: `goose-docs-1.52.0.tar.gz` 344.4MB + manifest
- [x] 맵 항목 61개 전부 실제 파일로 존재 (누락 0)
- [x] 추출 경로를 `GOOSE_DOCS_ROOT`로 지정 시 `goose-doc-guide`가 **네트워크 없이** 파일 도구만으로 문서를 읽음
- [x] `manifest.json`에 `goose_version`/`tag`/`commit`/`sha256`/`map_sha256`/`entries` 기록

**P2 (완료분 — 실측 통과)**

- [x] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` 통과
- [x] 테스트 37개 통과(단위 32 + 통합 5), 실제 서버를 띄우는 HTTP 테스트 포함
- [x] 실제 번들 61페이지 전부 200
- [x] `.md`가 `text/plain`으로 서빙됨(스킬이 원문을 읽음)
- [x] 없는 경로 404, HTML 폴백 없음
- [x] 경로 이탈(`/../`, `/docs/../../`, `%2e%2e`) 403
- [x] `--port 0` → 실제 포트를 로그·URL에 반영
- [x] 플래그 없이 기본 바인드로 기동 → LAN IP에서 200 응답, 광고 URL이 wildcard 아님

**P2.5 (추가 검증 — 실제 문서 사이트)**

- [x] 실제 344MB 번들의 **HTML 사이트가 브라우저에서 정상 렌더**(홈/문서/설치 페이지 스크린샷)
- [x] Docusaurus 절대경로 자산(`/assets/**`) 전부 200, `text/css`·`text/javascript` content-type 정상
- [x] 사이트 전체 크롤링: **페이지 346개 / 자산 301개, 404 0건** (`tools/crawl-site.py`)
- [x] 콘솔 에러 0건. React #418은 **공식 goose-docs.ai에서도 동일 발생**(업스트림 문제, 우리 서버 무관)
- [x] 같은 서버로 `goose-doc-guide`가 문서를 읽음(61페이지 확인)

**P3 (완료분 — 실제 GUI 클릭 검증)**

- [x] macOS에서 패널 창이 뜨고 렌더링됨 (780×612, 스크린샷)
- [x] **Start 클릭 → `*:10650 (LISTEN)`, LAN IP에서 200**
- [x] 상태 표시: URL·Listening·Reach·Pages·Uptime·Requests 갱신
- [x] **Stop 클릭 → 포트 해제, 연결 거부**
- [x] **창 닫기 → 프로세스 종료 + 포트 해제**(고아 없음)
- [x] 설정 저장(비밀값 없음), 실패 시 자동 헤드리스 폴백

**P4 (구현 완료 — 실행 검증 일부 대기)**

- [x] `actionlint`(shellcheck 규칙 포함) 3개 워크플로 0 errors
- [x] `shellcheck -S warning tools/*.sh` 0 issues
- [x] macOS 네이티브 빌드 + 테스트 71개 + 스모크 테스트 통과
- [x] Linux(Ubuntu 24.04 컨테이너) 빌드 + 테스트 71개 + 스모크 테스트 + 디스플레이 없을 때 헤드리스 폴백 통과
- [x] 워크플로 단계를 로컬에서 그대로 재현(fmt/clippy/build/test/스모크/패키징/체크섬 검증)
- [x] 릴리즈 노트 렌더링 실제 실행 확인
- [ ] **Windows 빌드·실행** — 미검증 (§10.3)
- [ ] **GitHub Actions 실제 실행** — 원격 저장소 필요 (§10.3)
- [x] 정지 시 포트 해제(재바인드 가능)
- [x] `fetch`가 sha256 불일치 시 거부(구현), `doctor`가 잘못된 docs root에서 종료코드 1

**P2~P5 (남은 항목)**

- [ ] 패널(egui)에서 시작/중지/재시작, 잘못된 IP·포트 입력 차단이 동작한다
- [ ] 디스플레이 없는 Linux에서 패널 없이 실행하면 헤드리스로 동작하고 로그를 남긴다
- [ ] 기본 포트가 **10650**이며, 이미 사용 중이면 시작 전에 오류로 차단된다
- [ ] 패널 UI 문구가 영어이고, 폰트가 없어도 크래시하지 않는다
- [ ] 창을 닫으면 서버도 함께 정지한다(고아 프로세스 없음)
- [ ] 3개 타깃(win/linux/macos-arm64)에서 스모크 테스트가 실제 실행으로 통과한다 (Linux는 `xvfb-run --smoke-gui` 포함)
- [ ] 워크플로는 3개이며, 그 이상의 자동화가 없다
- [ ] `docs-bundle.yml`이 같은 goose 버전으로 재실행돼도 릴리즈를 중복 생성하지 않는다(멱등성)
- [ ] GitHub `schedule`이 비활성화되지 않았는지 1회 확인(장기 미활동 시 자동 비활성화 가능)
- [ ] goose 저장소에 추가된 워크플로/트리거가 **없다**(git diff로 확인)
