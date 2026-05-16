package models

import (
    "time"

    "github.com/google/uuid"
)

type Role string

const (
    RoleLead Role = "lead"
    RoleContributor Role = "contributor"
)

type ArticleUser struct {
    MediaID uuid.UUID `gorm:"column:media_id;primaryKey;type:uuid" json:"media_id"`
    ArticleID int64 `gorm:"column:article_id;primaryKey" json:"article_id"`
    UserID uuid.UUID `gorm:"column:user_id;primaryKey;type:uuid;index" json:"user_id"`
    User User `gorm:"foreignKey:UserID;constraint:OnDelete:CASCADE" json:"-"`
    AuthorOrder int32 `gorm:"column:author_order;not null;default:1" json:"author_order"`
    Role Role `gorm:"column:role;not null;default:'contributor'" json:"role"`
    CreatedAt time.Time `gorm:"column:created_at;not null" json:"created_at"`
}

func (ArticleUser) TableName() string { return "article_user" }
