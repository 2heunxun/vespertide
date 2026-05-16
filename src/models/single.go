package models

type Single struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    SingleRels []SingleRel `gorm:"foreignKey:Username" json:"-"`
}

func (Single) TableName() string { return "single" }
