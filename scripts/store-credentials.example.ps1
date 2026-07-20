# MS 스토어 제출 자격증명 — 예시.
#
# 사용법:
#   1) 이 파일을 store-credentials.ps1 로 복사 (그 이름은 .gitignore 로 커밋 제외됨)
#   2) 아래 실제 값 채우기
#   3) 릴리즈 전에 dot-source:   . .\scripts\store-credentials.ps1
#      그 다음   .\scripts\release-all.ps1   또는   .\scripts\submit-msix-store.ps1
#
# 값 출처:
#   STORE_APP_ID        Partner Center → RawBlow → 제품 ID → 스토어 ID (9로 시작하는 12자)
#   STORE_IDENTITY_NAME / STORE_PUBLISHER / STORE_PUBLISHER_DISPLAY
#                       Partner Center → RawBlow → 제품 관리 → 제품 ID 페이지
#                       (Package/Identity/Name, Publisher(CN=...GUID), Publisher display name)
#   STORE_TENANT_ID / STORE_CLIENT_ID / STORE_CLIENT_SECRET
#                       Azure Portal 에서 앱 등록(App registration) 생성 후,
#                       Partner Center → 계정 설정 → 사용자 관리 → Azure AD 애플리케이션에 그 앱 연결.
#                       Secret 은 만료 있으니 갱신 주의.

$env:STORE_APP_ID           = "9NXXXXXXXXXX"
$env:STORE_TENANT_ID        = "00000000-0000-0000-0000-000000000000"
$env:STORE_CLIENT_ID        = "00000000-0000-0000-0000-000000000000"
$env:STORE_CLIENT_SECRET    = "여기에-시크릿"
$env:STORE_IDENTITY_NAME    = "1234Hare.RawBlow"
$env:STORE_PUBLISHER        = "CN=ABCD1234-0000-0000-0000-배정된GUID"
$env:STORE_PUBLISHER_DISPLAY = "Hare"
