package models

import (
    "time"

    "github.com/google/uuid"
)

type Media struct {
    // hello
    ID uuid.UUID `gorm:"column:id;primaryKey;type:uuid" json:"id"`
    Name string `gorm:"column:name;not null;size:100" json:"name"`
    Description *string `gorm:"column:description;type:text" json:"description"`
    Logo *string `gorm:"column:logo;type:text" json:"logo"`
    OwnerID uuid.UUID `gorm:"column:owner_id;not null;type:uuid;index" json:"owner_id"`
    Owner User `gorm:"foreignKey:OwnerID;constraint:OnDelete:CASCADE" json:"-"`
    CreatedAt time.Time `gorm:"column:created_at;not null" json:"created_at"`
    UpdatedAt *time.Time `gorm:"column:updated_at" json:"updated_at"`
    Articles []Article `gorm:"foreignKey:MediaID" json:"-"`
    UserMediaRoles []UserMediaRole `gorm:"foreignKey:MediaID" json:"-"`
}

func (Media) TableName() string { return "media" }
