package models

import (
    "time"

    "github.com/google/uuid"
)

type Role string

const (
    RoleOwner Role = "owner"
    RoleEditor Role = "editor"
    RoleReporter Role = "reporter"
)

// hello media role
type UserMediaRole struct {
    // hello
    UserID uuid.UUID `gorm:"column:user_id;primaryKey;type:uuid;index" json:"user_id"`
    User User `gorm:"foreignKey:UserID;constraint:OnDelete:CASCADE" json:"-"`
    MediaID uuid.UUID `gorm:"column:media_id;primaryKey;type:uuid;index" json:"media_id"`
    Media Media `gorm:"foreignKey:MediaID;constraint:OnDelete:CASCADE" json:"-"`
    Role Role `gorm:"column:role;not null;index" json:"role"`
    CreatedAt time.Time `gorm:"column:created_at;not null" json:"created_at"`
}

func (UserMediaRole) TableName() string { return "user_media_role" }
