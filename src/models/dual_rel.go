package models

type DualRel struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    Username Dual `gorm:"foreignKey:Username;constraint:OnDelete:CASCADE" json:"-"`
    CheckerUsername string `gorm:"column:checker_username;primaryKey;size:32" json:"checker_username"`
    CheckerUsername Dual `gorm:"foreignKey:CheckerUsername;constraint:OnDelete:CASCADE" json:"-"`
}

func (DualRel) TableName() string { return "dual_rel" }
