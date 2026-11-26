package config

type GinkgoConfigType = DeprecatedGinkgoConfigType
type DeprecatedGinkgoConfigType struct {
	RandomSeed         int64
	RandomizeAllSpecs  bool
	RegexScansFilePath bool
	FocusStrings       []string
	SkipStrings        []string
	SkipMeasurements   bool
	FailOnPending      bool
	FailFast           bool
	FlakeAttempts      int
	EmitSpecProgress   bool
	DryRun             bool
	DebugParallel      bool
	ParallelNode       int
	ParallelTotal      int
	SyncHost           string
	StreamHost         string
}
type DefaultReporterConfigType = DeprecatedDefaultReporterConfigType
type DeprecatedDefaultReporterConfigType struct {
	NoColor           bool
	SlowSpecThreshold float64
	NoisyPendings     bool
	NoisySkippings    bool
	Succinct          bool
	Verbose           bool
	FullTrace         bool
	ReportPassed      bool
	ReportFile        string
}
type GinkgoConfigIsNoLongerAccessibleFromTheConfigPackageUseTheDSLsGinkgoConfigurationFunctionInstead struct{}

var GinkgoConfig = GinkgoConfigIsNoLongerAccessibleFromTheConfigPackageUseTheDSLsGinkgoConfigurationFunctionInstead{}

type DefaultReporterConfigIsNoLongerAccessibleFromTheConfigPackageUseTheDSLsGinkgoConfigurationFunctionInstead struct{}

var DefaultReporterConfig = DefaultReporterConfigIsNoLongerAccessibleFromTheConfigPackageUseTheDSLsGinkgoConfigurationFunctionInstead{}
