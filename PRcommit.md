# PR 파일 정리

## 등록해야 할 파일 (PR에 포함)

### 핵심 기능 구현
| 파일 | 설명 |
|------|------|
| `crates/vespertide-exporter/src/gorm/mod.rs` | GORM 익스포터 구현 + Go build 검증 중 발견된 버그 3개 수정 |
| `crates/vespertide-exporter/src/lib.rs` | `GormExporter` 공개 re-export 등록 |
| `crates/vespertide-exporter/src/orm.rs` | `Orm::Gorm` variant 추가 |
| `crates/vespertide-cli/src/commands/export.rs` | CLI `--orm gorm` 옵션 및 `.go` 확장자 추가 |

### 스냅샷 테스트
| 파일 | 설명 |
|------|------|
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__all_simple_types.snap` | 전체 단순 타입 스냅샷 |
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__basic_table.snap` | 기본 테이블 스냅샷 |
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__composite_pk_nullable.snap` | 복합 PK 스냅샷 |
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__table_with_foreign_key.snap` | FK 스냅샷 |
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__table_with_integer_enum.snap` | 정수 Enum 스냅샷 |
| `crates/vespertide-exporter/src/gorm/snapshots/vespertide_exporter__gorm__tests__table_with_string_enum.snap` | 문자열 Enum 스냅샷 |
| `crates/vespertide-exporter/src/snapshots/vespertide_exporter__orm__tests__render_entity_snapshots@gorm.snap` | ORM 디스패치 스냅샷 |
| `crates/vespertide-exporter/src/snapshots/vespertide_exporter__orm__tests__render_entity_with_schema_snapshots@gorm.snap` | ORM 스키마 포함 스냅샷 |

### 문서
| 파일 | 설명 |
|------|------|
| `README.md` | ORM Export 섹션에 GORM 추가 |
| `CHANGELOG.md` | `[Unreleased]` 섹션에 GORM 익스포터 추가 기록 |

---

## 제거해야 할 파일 (PR에 포함하면 안 됨)

### 개인 프로젝트 설정
| 파일 | 이유 |
|------|------|
| `CLAUDE.md` | AI 코딩 지침 파일 — 개인 워크플로우 설정 |
| `GORMtask.md` | 개인 태스크 관리 문서 |
| `vespertide.json` | 이 프로젝트 전용 설정 (`vespera::Schema` 등 외부 의존 포함) |

### 생성된 Go 파일 (src/models/)
`cargo run -- export --orm gorm` 으로 생성된 출력물이며, **이 저장소의 예제 스키마**에 종속된 파일들.
라이브러리 코드가 아니므로 PR 대상이 아님.

| 파일 | 이유 |
|------|------|
| `src/models/go.mod` | 로컬 Go build 검증용 임시 파일 (`module example.com/test`) |
| `src/models/go.sum` | 로컬 Go build 검증용 임시 파일 |
| `src/models/article.go` | 이 프로젝트 스키마에서 생성된 파일 |
| `src/models/article_user.go` | 동일 |
| `src/models/dual.go` | 동일 |
| `src/models/dual_rel.go` | 동일 |
| `src/models/media.go` | 동일 |
| `src/models/single.go` | 동일 |
| `src/models/single_rel.go` | 동일 |
| `src/models/triple.go` | 동일 |
| `src/models/triple_rel.go` | 동일 |
| `src/models/user.go` | 동일 |
| `src/models/user_media_role.go` | 동일 |

---

## PR 브랜치 구성 순서

1. `git restore CLAUDE.md GORMtask.md vespertide.json` — 개인 파일 되돌리기
2. `git rm --cached src/models/` — 생성된 Go 파일 스테이징에서 제거
3. 등록 파일만 커밋 후 fork → PR 오픈
