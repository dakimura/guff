package types

import (
	"sync"
)

type CodeLocation struct {
	FileName       string `json:",omitempty"`
	LineNumber     int    `json:",omitempty"`
	FullStackTrace string `json:",omitempty"`
	CustomMessage  string `json:",omitempty"`
}

func (codeLocation CodeLocation) String() string {
	panic("stub")
}
func (codeLocation CodeLocation) ContentsOfLine() string {
	panic("stub")
}

type codeLocationLocator struct {
	pcs     map[uintptr]bool
	helpers map[string]bool
	lock    *sync.Mutex
}

func (c *codeLocationLocator) addHelper(pc uintptr) {
	panic("stub")
}
func (c *codeLocationLocator) hasHelper(name string) bool {
	panic("stub")
}
func (c *codeLocationLocator) getCodeLocation(skip int) CodeLocation {
	panic("stub")
}

var clLocator = &codeLocationLocator{pcs: map[uintptr]bool{}, helpers: map[string]bool{}, lock: &sync.Mutex{}}

func MarkAsHelper(optionalSkip ...int) {
	panic("stub")
}
func NewCustomCodeLocation(message string) CodeLocation {
	panic("stub")
}
func NewCodeLocation(skip int) CodeLocation {
	panic("stub")
}
func NewCodeLocationWithStackTrace(skip int) CodeLocation {
	panic("stub")
}
func PruneStack(fullStackTrace string, skip int) string {
	panic("stub")
}
