package models

type SingleRel struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    Username Single `gorm:"foreignKey:Username;constraint:OnDelete:CASCADE" json:"-"`
}

func (SingleRel) TableName() string { return "single_rel" }
