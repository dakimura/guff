package p

import (
	"crypto/rand"
	"encoding/json"
	"strings"
	"uuid"
)

var (
	_ = uuid.New()
	_ = strings.Lines("a")
	_ = rand.Int
	_ = json.Marshal
)
