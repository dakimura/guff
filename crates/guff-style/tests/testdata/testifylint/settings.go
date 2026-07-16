package testifylint

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestSettings(t *testing.T) {
	var result bool
	assert.Equal(t, false, result)
	assert.Equal(t, 0, len([]int{}))
}
