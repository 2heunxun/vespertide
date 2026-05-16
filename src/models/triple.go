package models

type Triple struct {
    Username string `gorm:"column:username;primaryKey;size:32" json:"username"`
    TripleRels []TripleRel `gorm:"foreignKey:Username" json:"-"`
    TripleRels []TripleRel `gorm:"foreignKey:CheckerUsername" json:"-"`
    TripleRels []TripleRel `gorm:"foreignKey:OtherUsername" json:"-"`
}

func (Triple) TableName() string { return "triple" }
