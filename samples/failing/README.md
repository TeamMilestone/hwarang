# 파싱 실패 샘플

나라장터(g2b.go.kr) 입찰공고 첨부에서 hwarang 이 열지 못한 실제 문서들이다.
2026-08-11 ~ 08-14 나흘치를 전수 조사해 추린 것으로, 출처는 공개된 정부 입찰공고다.

## 모집단

| 구분 | 개수 |
|---|---|
| HWP/HWPX 전체 | 3,602 |
| OLE (HWP 5.x) | 2,511 |
| PK (HWPX) | 1,074 |
| XML (HWPML) | 14 |
| **HWP 3.0** | **0** |
| 파싱 실패 (고유) | 5 |

**HWP 3.0 은 한 건도 나오지 않았다.** 나흘 내내 0건이다. `src/hwp30/` 경로가
실전에서 밟히는지는 이 표본으로는 확인되지 않는다.

## 파일

| 파일 | 매직 | 크기 | 실패 사유 |
|---|---|---|---|
| `drm-drmone.hwp` | `9B 20 44 52` | 70KB | `UnsupportedFormat` |
| `drm-drmone.hwpx` | `9B 20 44 52` | 68KB | `UnsupportedFormat` |
| `drm-softcamp-scds.hwpx` | `53 43 44 53` | 130KB | `UnsupportedFormat` |
| `cfb-malformed-minifat.hwp` | `D0 CF 11 E0` | 116KB | `Malformed MiniFAT` |
| `cfb-password-protected.hwp` | `D0 CF 11 E0` | 20KB | `Password-protected document` |

### DRM 3건 — 고칠 수 없다

확장자만 `.hwp`/`.hwpx` 일 뿐 내용은 DRM 컨테이너다. 헤더에 평문으로 이렇게 적혀 있다:

```
9B 20 44 52 4D 4F 4E 45  ".  D  R  M  O  N  E"
"This Document is encrypted and protected"
```

`53 43 44 53` 은 SoftCamp 계열(`SCDSA004`). 복호화 키가 없으면 어떤 파서로도 못 연다.
**개선 여지는 파싱이 아니라 오류 메시지에 있다** — 지금은 "알 수 없는 형식" 이라
호출자가 자기 버그인지 문서 문제인지 구분하지 못한다. 매직을 알아보고
"DRM 보호 문서" 로 답하면 상위 계층이 재시도·알림 여부를 정할 수 있다.

### CFB 2건 — 볼 만하다

- `cfb-malformed-minifat.hwp` — MiniFAT 항목이 157개인데 미니섹터 103 이 157 을 가리킨다.
  범위를 벗어난 체인이다. 한글에서는 열린다면 관대한 복구가 가능하다는 뜻이고,
  한글에서도 안 열린다면 애초에 깨진 파일이다. 먼저 확인이 필요하다.
- `cfb-password-protected.hwp` — 암호가 걸려 있다. 이건 정상 동작이며,
  현재 오류 메시지도 정확하다.

## 재현

```bash
cargo run --release --example check_failing -- samples/failing/*.hwp samples/failing/*.hwpx
```

```
cfb-malformed-minifat.hwp        ERR: I/O error: Malformed MiniFAT (MiniFAT has 157 entries, but mini sector 103 points to 157)
cfb-password-protected.hwp       ERR: Password-protected document
drm-drmone.hwp                   ERR: Unsupported file format
drm-drmone.hwpx                  ERR: Unsupported file format
drm-softcamp-scds.hwpx           ERR: Unsupported file format
```

## 주의

실제 공고 첨부라 내용이 들어 있다. 공개 자료지만 저장소에 커밋할지는 판단이 필요하다.
