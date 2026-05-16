# GORM 익스포터 완성 태스크

## 완료
- [x] `crates/vespertide-exporter/src/gorm/mod.rs` 구현
- [x] `Orm::Gorm` variant 등록 (orm.rs, lib.rs)
- [x] CLI `--orm gorm` 등록 (export.rs, `.go` 확장자)
- [x] 스냅샷 테스트 356개 통과

## 남은 단계

### 1. Go 설치 및 go build 검증
- Go 설치: https://go.dev/dl/
- 아래 명령으로 생성 코드 컴파일 검증
```bash
cd C:\vespertide
cargo run -p vespertide-cli -- export --orm gorm
cd <export 출력 디렉토리>
go mod init example.com/test
go get gorm.io/gorm
go get gorm.io/datatypes
go get github.com/google/uuid
go get github.com/shopspring/decimal
go build ./...
```


### 2. README 업데이트
- 지원 ORM 목록에 GORM 추가
- `--orm gorm` 사용 예시 추가

### 3. CHANGELOG 업데이트
- GORM 익스포터 추가 내용 기록
