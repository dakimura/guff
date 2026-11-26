package types

import (
	"regexp"
)

func ParseFileFilters(filters []string) (FileFilters, error) {
	panic("stub")
}

type FileFilter struct {
	Filename    *regexp.Regexp
	LineFilters LineFilters
}

func (f FileFilter) Matches(locations []CodeLocation) bool {
	panic("stub")
}

type FileFilters []FileFilter

func (ffs FileFilters) Matches(locations []CodeLocation) bool {
	panic("stub")
}

type LineFilter struct {
	Min int
	Max int
}

func (lf LineFilter) Matches(line int) bool {
	panic("stub")
}

type LineFilters []LineFilter

func (lfs LineFilters) Matches(line int) bool {
	panic("stub")
}
