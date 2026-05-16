# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- GORM exporter (`--orm gorm`): generates Go structs with GORM tags from schema definitions
  - Full type mapping: all SQL types to Go equivalents (`int32`, `int64`, `float32`, `float64`, `string`, `bool`, `time.Time`, `uuid.UUID`, `datatypes.JSON`, `decimal.Decimal`, etc.)
  - String and integer enum support with Go type aliases and `const` blocks
  - Foreign key relations with `foreignKey` and `constraint` tags
  - Reverse (HasMany) relations when schema context is provided
  - Composite primary keys, unique indexes, and named indexes
  - `TableName()` method generated when table name differs from GORM convention
  - Smart import generation (only includes `time`, `gorm.io/datatypes`, `github.com/google/uuid`, `github.com/shopspring/decimal` when needed)
