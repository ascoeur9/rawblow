# RawBlow — Privacy Policy / 개인정보 처리방침

**Last updated / 최종 수정: 2026-07-20**

RawBlow is a desktop application for viewing and culling (rating, labeling, and organizing) photos. It runs entirely on your own device. We do not operate any server that collects, stores, or processes your personal information. RawBlow has **no user accounts, no sign-in, no advertising, no analytics, and no telemetry.**

RawBlow은 사진을 보고 컬링(평가·라벨링·정리)하는 데스크톱 앱으로, **전적으로 사용자 기기 안에서** 동작합니다. 개인정보를 수집·저장·처리하는 서버를 운영하지 않으며, **계정·로그인·광고·분석·텔레메트리가 전혀 없습니다.**

---

## English

### Data the app handles (locally, on your device)
- **Your photos and RAW files**, and their embedded metadata (EXIF — camera/lens, capture time, and GPS location if present). These are read locally to display and organize your images.
- **Your ratings, labels, and color tags** — saved as small "sidecar" text files next to your photos, and in the app's configuration folder (`%APPDATA%\RawBlow`).
- **A thumbnail cache** (`%LOCALAPPDATA%\RawBlow`) to make browsing faster.
- **A crash log** (`rawblow_crash.log`) written to your Desktop if the app crashes. It is stored locally and is **never transmitted anywhere**.

None of the above is sent to us or to any third party by the app itself.

### Network connections (only for these optional features)
RawBlow connects to the internet **only** when you use one of these features:

1. **Update check** — *optional; can be turned off in Settings.* The app queries GitHub's public API (`api.github.com`) to check whether a newer version exists. This sends only a standard web request (the app version in the User-Agent header, and — as with any internet request — your IP address). No file or personal data is uploaded.
2. **Photo location map** — *optional; only when you open the map for a photo that contains GPS data.* The app requests map tiles from OpenStreetMap (`tile.openstreetmap.org`) for the area around that photo's location. The map area, derived from your photo's coordinates, is sent to OpenStreetMap's servers as part of the tile request.
3. **AI model download** — *optional; only if you enable the AI culling feature.* The app downloads a model file from the project's GitHub releases. No personal data is uploaded.

These requests go to third-party services operated by others; their handling of the request is governed by **their own** privacy policies:
- GitHub (Microsoft): https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement
- OpenStreetMap Foundation: https://osmfoundation.org/wiki/Privacy_Policy

### What we do NOT do
- We do **not** collect, store, sell, share, or transmit your photos, your edits (ratings/labels/tags), or any personal information.
- **No** analytics, tracking, advertising, or telemetry.
- **No** user accounts or sign-in.

### Children
RawBlow is a general-purpose photo tool and does not knowingly collect any personal information from anyone, including children.

### Changes to this policy
We may update this policy from time to time; the "Last updated" date above will change accordingly.

### Contact
Questions or requests: please open an issue at https://github.com/ascoeur9/rawblow/issues
(or email: hare.rinko@gmail.com).

---

## 한국어

### 앱이 다루는 데이터 (사용자 기기 내부, 로컬)
- **사진·RAW 파일**과 그 안의 메타데이터(EXIF — 카메라/렌즈, 촬영 시각, GPS 위치가 있으면 위치). 이미지를 표시·정리하기 위해 로컬에서만 읽습니다.
- **평가·라벨·컬러 태그** — 사진 옆의 작은 "사이드카" 텍스트 파일과 앱 설정 폴더(`%APPDATA%\RawBlow`)에 저장됩니다.
- **썸네일 캐시**(`%LOCALAPPDATA%\RawBlow`) — 탐색 속도를 위해 저장됩니다.
- **크래시 로그**(`rawblow_crash.log`) — 앱이 비정상 종료되면 바탕화면에 기록됩니다. 로컬에만 저장되며 **어디로도 전송되지 않습니다.**

위 데이터는 앱에 의해 우리나 제3자에게 전송되지 않습니다.

### 네트워크 연결 (아래 선택 기능을 쓸 때만)
RawBlow은 다음 기능을 사용할 때**만** 인터넷에 연결합니다:

1. **업데이트 확인** — *선택 사항, 설정에서 끌 수 있음.* 새 버전이 있는지 GitHub 공개 API(`api.github.com`)에 조회합니다. 표준 웹 요청(User-Agent의 앱 버전, 그리고 모든 인터넷 요청과 마찬가지로 IP 주소)만 전송하며, 파일이나 개인정보는 올리지 않습니다.
2. **사진 위치 지도** — *선택 사항, GPS가 있는 사진의 지도를 열 때만.* 해당 사진 위치 주변의 지도 타일을 OpenStreetMap(`tile.openstreetmap.org`)에 요청합니다. 사진 좌표에서 도출된 지도 영역이 타일 요청의 일부로 OpenStreetMap 서버에 전송됩니다.
3. **AI 모델 다운로드** — *선택 사항, AI 컬링 기능을 켤 때만.* 프로젝트 GitHub 릴리즈에서 모델 파일을 내려받습니다. 개인정보는 올리지 않습니다.

이 요청들은 타사가 운영하는 서비스로 전달되며, 해당 요청의 처리는 **각 서비스의** 개인정보 처리방침을 따릅니다:
- GitHub(Microsoft): https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement
- OpenStreetMap Foundation: https://osmfoundation.org/wiki/Privacy_Policy

### 하지 않는 것
- 사진, 편집 내용(평가/라벨/태그), 개인정보를 **수집·저장·판매·공유·전송하지 않습니다.**
- 분석·추적·광고·텔레메트리가 **없습니다.**
- 계정·로그인이 **없습니다.**

### 아동
RawBlow은 범용 사진 도구이며, 아동을 포함해 누구로부터도 개인정보를 고의로 수집하지 않습니다.

### 방침 변경
본 방침은 수시로 업데이트될 수 있으며, 그때마다 위 "최종 수정" 날짜가 변경됩니다.

### 문의
문의·요청: https://github.com/ascoeur9/rawblow/issues 에 이슈를 남겨 주세요
(또는 이메일: hare.rinko@gmail.com).
