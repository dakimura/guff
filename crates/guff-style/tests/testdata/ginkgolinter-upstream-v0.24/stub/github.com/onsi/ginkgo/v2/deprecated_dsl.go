package ginkgo

import (
	"time"
	"github.com/onsi/ginkgo/v2/internal"
	"github.com/onsi/ginkgo/v2/reporters"
)

type Done = internal.Done
type Reporter = reporters.DeprecatedReporter

func RunSpecsWithDefaultAndCustomReporters(t GinkgoTestingT, description string, _ []Reporter) bool {
	panic("stub")
}
func RunSpecsWithCustomReporters(t GinkgoTestingT, description string, _ []Reporter) bool {
	panic("stub")
}

type DeprecatedGinkgoTestDescription struct {
	FullTestText   string
	ComponentTexts []string
	TestText       string
	FileName       string
	LineNumber     int
	Failed         bool
	Duration       time.Duration
}
type GinkgoTestDescription = DeprecatedGinkgoTestDescription

func CurrentGinkgoTestDescription() DeprecatedGinkgoTestDescription {
	panic("stub")
}
func GinkgoParallelNode() int {
	panic("stub")
}

type Benchmarker interface {
	Time(name string, body func(), info ...any) (elapsedTime time.Duration)
	RecordValue(name string, value float64, info ...any)
	RecordValueWithPrecision(name string, value float64, units string, precision int, info ...any)
}

func Measure(_ ...any) bool {
	panic("stub")
}
