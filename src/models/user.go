package models

import (
    "time"

    "github.com/google/uuid"
)

type User struct {
    ID uuid.UUID `gorm:"column:id;primaryKey;type:uuid" json:"id"`
    Email string `gorm:"column:email;not null;unique;size:255;index" json:"email"`
    Password string `gorm:"column:password;not null;size:255" json:"password"`
    Name string `gorm:"column:name;not null;size:100" json:"name"`
    ProfileImage *string `gorm:"column:profile_image;type:text" json:"profile_image"`
    CreatedAt time.Time `gorm:"column:created_at;not null" json:"created_at"`
    UpdatedAt *time.Time `gorm:"column:updated_at" json:"updated_at"`
    ArticleUsers []ArticleUser `gorm:"foreignKey:UserID" json:"-"`
    Medias []Media `gorm:"foreignKey:OwnerID" json:"-"`
    UserMediaRoles []UserMediaRole `gorm:"foreignKey:UserID" json:"-"`
}

func (User) TableName() string { return "user" }
