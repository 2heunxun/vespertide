package models

type TripleRel struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    Username Triple `gorm:"foreignKey:Username;constraint:OnDelete:CASCADE" json:"-"`
    CheckerUsername string `gorm:"column:checker_username;primaryKey;size:32" json:"checker_username"`
    CheckerUsername Triple `gorm:"foreignKey:CheckerUsername;constraint:OnDelete:CASCADE" json:"-"`
    OtherUsername string `gorm:"column:other_username;primaryKey;size:32" json:"other_username"`
    OtherUsername Triple `gorm:"foreignKey:OtherUsername;constraint:OnDelete:CASCADE" json:"-"`
}

func (TripleRel) TableName() string { return "triple_rel" }
