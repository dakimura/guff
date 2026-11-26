package types

import (
	"sync"
)

type Deprecation struct {
	Message string
	DocLink string
	Version string
}
type deprecations struct{}

var Deprecations = deprecations{}

func (d deprecations) CustomReporter() Deprecation {
	panic("stub")
}
func (d deprecations) Async() Deprecation {
	panic("stub")
}
func (d deprecations) Measure() Deprecation {
	panic("stub")
}
func (d deprecations) ParallelNode() Deprecation {
	panic("stub")
}
func (d deprecations) CurrentGinkgoTestDescription() Deprecation {
	panic("stub")
}
func (d deprecations) Convert() Deprecation {
	panic("stub")
}
func (d deprecations) Blur() Deprecation {
	panic("stub")
}
func (d deprecations) Nodot() Deprecation {
	panic("stub")
}
func (d deprecations) SuppressProgressReporting() Deprecation {
	panic("stub")
}

type DeprecationTracker struct {
	deprecations map[Deprecation][]CodeLocation
	lock         *sync.Mutex
}

func NewDeprecationTracker() *DeprecationTracker {
	panic("stub")
}
func (d *DeprecationTracker) TrackDeprecation(deprecation Deprecation, cl ...CodeLocation) {
	panic("stub")
}
func (d *DeprecationTracker) DidTrackDeprecations() bool {
	panic("stub")
}
func (d *DeprecationTracker) DeprecationsReport() string {
	panic("stub")
}

type SemVer struct {
	Major int
	Minor int
	Patch int
}

func (s SemVer) GreaterThanOrEqualTo(o SemVer) bool {
	panic("stub")
}
func ParseSemVer(semver string) SemVer {
	panic("stub")
}
