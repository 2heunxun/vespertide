package models

import (
    "time"

    "github.com/google/uuid"
)

type Status string

const (
    StatusDraft Status = "draft"
    StatusReview Status = "review"
    StatusPublished Status = "published"
    StatusArchived Status = "archived"
)

type Article struct {
    MediaID uuid.UUID `gorm:"column:media_id;primaryKey;type:uuid" json:"media_id"`
    Media Media `gorm:"foreignKey:MediaID;constraint:OnDelete:CASCADE" json:"-"`
    ID int64 `gorm:"column:id;primaryKey" json:"id"`
    Title string `gorm:"column:title;not null;size:500" json:"title"`
    Content string `gorm:"column:content;not null;type:text" json:"content"`
    Summary *string `gorm:"column:summary;type:text" json:"summary"`
    Thumbnail *string `gorm:"column:thumbnail;type:text" json:"thumbnail"`
    Status Status `gorm:"column:status;not null;default:'draft';index" json:"status"`
    PublishedAt *time.Time `gorm:"column:published_at;index" json:"published_at"`
    CreatedAt time.Time `gorm:"column:created_at;not null" json:"created_at"`
    UpdatedAt *time.Time `gorm:"column:updated_at" json:"updated_at"`
}

func (Article) TableName() string { return "article" }
