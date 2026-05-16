package models

type Dual struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    DualRels []DualRel `gorm:"foreignKey:Username" json:"-"`
    DualRels []DualRel `gorm:"foreignKey:CheckerUsername" json:"-"`
}

func (Dual) TableName() string { return "dual" }
