package types

import (
	"time"
)

type SuiteConfig struct {
	RandomSeed            int64
	RandomizeAllSpecs     bool
	FocusStrings          []string
	SkipStrings           []string
	FocusFiles            []string
	SkipFiles             []string
	LabelFilter           string
	SemVerFilter          string
	FailOnPending         bool
	FailOnEmpty           bool
	FailFast              bool
	FlakeAttempts         int
	MustPassRepeatedly    int
	DryRun                bool
	PollProgressAfter     time.Duration
	PollProgressInterval  time.Duration
	Timeout               time.Duration
	EmitSpecProgress      bool
	OutputInterceptorMode string
	SourceRoots           []string
	GracePeriod           time.Duration
	SleepOnFailure        time.Duration
	ParallelProcess       int
	ParallelTotal         int
	ParallelHost          string
}

func NewDefaultSuiteConfig() SuiteConfig {
	panic("stub")
}

type VerbosityLevel uint

const (
	VerbosityLevelSuccinct VerbosityLevel = iota
	VerbosityLevelNormal
	VerbosityLevelVerbose
	VerbosityLevelVeryVerbose
)

func (vl VerbosityLevel) GT(comp VerbosityLevel) bool {
	panic("stub")
}
func (vl VerbosityLevel) GTE(comp VerbosityLevel) bool {
	panic("stub")
}
func (vl VerbosityLevel) Is(comp VerbosityLevel) bool {
	panic("stub")
}
func (vl VerbosityLevel) LTE(comp VerbosityLevel) bool {
	panic("stub")
}
func (vl VerbosityLevel) LT(comp VerbosityLevel) bool {
	panic("stub")
}

type ReporterConfig struct {
	NoColor        bool
	Succinct       bool
	Verbose        bool
	VeryVerbose    bool
	FullTrace      bool
	ShowNodeEvents bool
	GithubOutput   bool
	SilenceSkips   bool
	ForceNewlines  bool
	FdOutput       bool
	JSONReport     string
	GoJSONReport   string
	JUnitReport    string
	TeamcityReport string
}

func (rc ReporterConfig) Verbosity() VerbosityLevel {
	panic("stub")
}
func (rc ReporterConfig) WillGenerateReport() bool {
	panic("stub")
}
func NewDefaultReporterConfig() ReporterConfig {
	panic("stub")
}

type CLIConfig struct {
	Recurse                   bool
	SkipPackage               string
	RequireSuite              bool
	NumCompilers              int
	Procs                     int
	Parallel                  bool
	AfterRunHook              string
	OutputDir                 string
	KeepSeparateCoverprofiles bool
	KeepSeparateReports       bool
	KeepGoing                 bool
	UntilItFails              bool
	Repeat                    int
	RandomizeSuites           bool
	Depth                     int
	WatchRegExp               string
}

func NewDefaultCLIConfig() CLIConfig {
	panic("stub")
}
func (g CLIConfig) ComputedProcs() int {
	panic("stub")
}
func (g CLIConfig) ComputedNumCompilers() int {
	panic("stub")
}

type GoFlagsConfig struct {
	Race                 bool
	Cover                bool
	CoverMode            string
	CoverPkg             string
	Vet                  string
	BlockProfile         string
	BlockProfileRate     int
	CoverProfile         string
	CPUProfile           string
	MemProfile           string
	MemProfileRate       int
	MutexProfile         string
	MutexProfileFraction int
	Trace                string
	A                    bool
	ASMFlags             string
	BuildMode            string
	BuildVCS             bool
	Compiler             string
	GCCGoFlags           string
	GCFlags              string
	InstallSuffix        string
	LDFlags              string
	LinkShared           bool
	Mod                  string
	N                    bool
	ModFile              string
	ModCacheRW           bool
	ASan                 bool
	MSan                 bool
	PkgDir               string
	Tags                 string
	TrimPath             bool
	ToolExec             string
	Work                 bool
	X                    bool
	O                    string
}

func NewDefaultGoFlagsConfig() GoFlagsConfig {
	panic("stub")
}
func (g GoFlagsConfig) BinaryMustBePreserved() bool {
	panic("stub")
}
func (g GoFlagsConfig) NeedsSymbols() bool {
	panic("stub")
}

type deprecatedConfig struct {
	DebugParallel                   bool
	NoisySkippings                  bool
	NoisyPendings                   bool
	RegexScansFilePath              bool
	SlowSpecThresholdWithFLoatUnits float64
	Stream                          bool
	Notify                          bool
	EmitSpecProgress                bool
	SlowSpecThreshold               time.Duration
	AlwaysEmitGinkgoWriter          bool
}

var FlagSections = GinkgoFlagSections{{Key: "multiple-suites", Style: "{{dark-green}}", Heading: "Running Multiple Test Suites"}, {Key: "order", Style: "{{green}}", Heading: "Controlling Test Order"}, {Key: "parallel", Style: "{{yellow}}", Heading: "Controlling Test Parallelism"}, {Key: "low-level-parallel", Style: "{{yellow}}", Heading: "Controlling Test Parallelism", Description: "These are set by the Ginkgo CLI, {{red}}{{bold}}do not set them manually{{/}} via go test.\nUse ginkgo -p or ginkgo -procs=N instead."}, {Key: "filter", Style: "{{cyan}}", Heading: "Filtering Tests"}, {Key: "failure", Style: "{{red}}", Heading: "Failure Handling"}, {Key: "output", Style: "{{magenta}}", Heading: "Controlling Output Formatting"}, {Key: "code-and-coverage-analysis", Style: "{{orange}}", Heading: "Code and Coverage Analysis", Description: "When generating a cover files, please pass a filename {{bold}}not{{/}} a path.  To specify a different directory use {{magenta}}--output-dir{{/}}."}, {Key: "performance-analysis", Style: "{{coral}}", Heading: "Performance Analysis", Description: "When generating profile files, please pass filenames {{bold}}not{{/}} a path.  Ginkgo will generate a profile file with the given name in the package's directory.  To specify a different directory use {{magenta}}--output-dir{{/}}."}, {Key: "debug", Style: "{{blue}}", Heading: "Debugging Tests", Description: "In addition to these flags, Ginkgo supports a few debugging environment variables.  To change the parallel server protocol set {{blue}}GINKGO_PARALLEL_PROTOCOL{{/}} to {{bold}}HTTP{{/}}.  To avoid pruning callstacks set {{blue}}GINKGO_PRUNE_STACK{{/}} to {{bold}}FALSE{{/}}."}, {Key: "watch", Style: "{{light-yellow}}", Heading: "Controlling Ginkgo Watch"}, {Key: "misc", Style: "{{light-gray}}", Heading: "Miscellaneous"}, {Key: "go-build", Style: "{{light-gray}}", Heading: "Go Build Flags", Succinct: true, Description: "These flags are inherited from go build.  Run {{bold}}ginkgo help build{{/}} for more detailed flag documentation."}}
var SuiteConfigFlags = GinkgoFlags{{KeyPath: "S.RandomSeed", Name: "seed", SectionKey: "order", UsageDefaultValue: "randomly generated by Ginkgo", Usage: "The seed used to randomize the spec suite.", AlwaysExport: true}, {KeyPath: "S.RandomizeAllSpecs", Name: "randomize-all", SectionKey: "order", DeprecatedName: "randomizeAllSpecs", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will randomize all specs together.  By default, ginkgo only randomizes the top level Describe, Context and When containers."}, {KeyPath: "S.FailOnPending", Name: "fail-on-pending", SectionKey: "failure", DeprecatedName: "failOnPending", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will mark the test suite as failed if any specs are pending."}, {KeyPath: "S.FailFast", Name: "fail-fast", SectionKey: "failure", DeprecatedName: "failFast", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will stop running a test suite after a failure occurs."}, {KeyPath: "S.FlakeAttempts", Name: "flake-attempts", SectionKey: "failure", UsageDefaultValue: "0 - failed tests are not retried", DeprecatedName: "flakeAttempts", DeprecatedDocLink: "changed-command-line-flags", Usage: "Make up to this many attempts to run each spec. If any of the attempts succeed, the suite will not be failed."}, {KeyPath: "S.FailOnEmpty", Name: "fail-on-empty", SectionKey: "failure", Usage: "If set, ginkgo will mark the test suite as failed if no specs are run."}, {KeyPath: "S.SleepOnFailure", Name: "sleep-on-failure", SectionKey: "failure", UsageDefaultValue: "0 - disabled", Usage: "If set, ginkgo will pause for this duration after a spec fails - before its teardown (AfterEach/JustAfterEach/DeferCleanup) runs - so you can inspect the live system. Press ^C to end the pause early and proceed to cleanup. Serial only: cannot be combined with -p/--procs."}, {KeyPath: "S.DryRun", Name: "dry-run", SectionKey: "debug", DeprecatedName: "dryRun", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will walk the test hierarchy without actually running anything.  Best paired with -v."}, {KeyPath: "S.PollProgressAfter", Name: "poll-progress-after", SectionKey: "debug", UsageDefaultValue: "0", Usage: "Emit node progress reports periodically if node hasn't completed after this duration."}, {KeyPath: "S.PollProgressInterval", Name: "poll-progress-interval", SectionKey: "debug", UsageDefaultValue: "10s", Usage: "The rate at which to emit node progress reports after poll-progress-after has elapsed."}, {KeyPath: "S.SourceRoots", Name: "source-root", SectionKey: "debug", Usage: "The location to look for source code when generating progress reports.  You can pass multiple --source-root flags."}, {KeyPath: "S.Timeout", Name: "timeout", SectionKey: "debug", UsageDefaultValue: "1h", Usage: "Test suite fails if it does not complete within the specified timeout."}, {KeyPath: "S.GracePeriod", Name: "grace-period", SectionKey: "debug", UsageDefaultValue: "30s", Usage: "When interrupted, Ginkgo will wait for GracePeriod for the current running node to exit before moving on to the next one."}, {KeyPath: "S.OutputInterceptorMode", Name: "output-interceptor-mode", SectionKey: "debug", UsageArgument: "dup, swap, or none", Usage: "If set, ginkgo will use the specified output interception strategy when running in parallel.  Defaults to dup on unix and swap on windows."}, {KeyPath: "S.LabelFilter", Name: "label-filter", SectionKey: "filter", UsageArgument: "expression", Usage: "If set, ginkgo will only run specs with labels that match the label-filter.  The passed-in expression can include boolean operations (!, &&, ||, ','), groupings via '()', and regular expressions '/regexp/'.  e.g. '(cat || dog) && !fruit'"}, {KeyPath: "S.SemVerFilter", Name: "sem-ver-filter", SectionKey: "filter", UsageArgument: "version", Usage: "If set, ginkgo will only run specs with semantic version constraints that are satisfied by the provided version. e.g. '2.1.0'"}, {KeyPath: "S.FocusStrings", Name: "focus", SectionKey: "filter", Usage: "If set, ginkgo will only run specs that match this regular expression. Can be specified multiple times, values are ORed."}, {KeyPath: "S.SkipStrings", Name: "skip", SectionKey: "filter", Usage: "If set, ginkgo will only run specs that do not match this regular expression. Can be specified multiple times, values are ORed."}, {KeyPath: "S.FocusFiles", Name: "focus-file", SectionKey: "filter", UsageArgument: "file (regexp) | file:line | file:lineA-lineB | file:line,line,line", Usage: "If set, ginkgo will only run specs in matching files. Can be specified multiple times, values are ORed."}, {KeyPath: "S.SkipFiles", Name: "skip-file", SectionKey: "filter", UsageArgument: "file (regexp) | file:line | file:lineA-lineB | file:line,line,line", Usage: "If set, ginkgo will skip specs in matching files. Can be specified multiple times, values are ORed."}, {KeyPath: "D.RegexScansFilePath", DeprecatedName: "regexScansFilePath", DeprecatedDocLink: "removed--regexscansfilepath", DeprecatedVersion: "2.0.0"}, {KeyPath: "D.DebugParallel", DeprecatedName: "debug", DeprecatedDocLink: "removed--debug", DeprecatedVersion: "2.0.0"}, {KeyPath: "D.EmitSpecProgress", DeprecatedName: "progress", SectionKey: "debug", DeprecatedVersion: "2.5.0", Usage: ".  The functionality provided by --progress was confusing and is no longer needed.  Use --show-node-events instead to see node entry and exit events included in the timeline of failed and verbose specs.  Or you can run with -vv to always see all node events.  Lastly, --poll-progress-after and the PollProgressAfter decorator now provide a better mechanism for debugging specs that tend to get stuck."}}
var ParallelConfigFlags = GinkgoFlags{{KeyPath: "S.ParallelProcess", Name: "parallel.process", SectionKey: "low-level-parallel", UsageDefaultValue: "1", Usage: "This worker process's (one-indexed) process number.  For running specs in parallel."}, {KeyPath: "S.ParallelTotal", Name: "parallel.total", SectionKey: "low-level-parallel", UsageDefaultValue: "1", Usage: "The total number of worker processes.  For running specs in parallel."}, {KeyPath: "S.ParallelHost", Name: "parallel.host", SectionKey: "low-level-parallel", UsageDefaultValue: "set by Ginkgo CLI", Usage: "The address for the server that will synchronize the processes."}}
var ReporterConfigFlags = GinkgoFlags{{KeyPath: "R.NoColor", Name: "no-color", SectionKey: "output", DeprecatedName: "noColor", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, suppress color output in default reporter.  You can also set the environment variable GINKGO_NO_COLOR=TRUE"}, {KeyPath: "R.Verbose", Name: "v", SectionKey: "output", Usage: "If set, emits more output including GinkgoWriter contents."}, {KeyPath: "R.VeryVerbose", Name: "vv", SectionKey: "output", Usage: "If set, emits with maximal verbosity - includes skipped and pending tests."}, {KeyPath: "R.Succinct", Name: "succinct", SectionKey: "output", Usage: "If set, default reporter prints out a very succinct report"}, {KeyPath: "R.FullTrace", Name: "trace", SectionKey: "output", Usage: "If set, default reporter prints out the full stack trace when a failure occurs"}, {KeyPath: "R.ShowNodeEvents", Name: "show-node-events", SectionKey: "output", Usage: "If set, default reporter prints node > Enter and < Exit events when specs fail"}, {KeyPath: "R.GithubOutput", Name: "github-output", SectionKey: "output", Usage: "If set, default reporter prints easier to manage output in Github Actions."}, {KeyPath: "R.SilenceSkips", Name: "silence-skips", SectionKey: "output", Usage: "If set, default reporter will not print out skipped tests."}, {KeyPath: "R.ForceNewlines", Name: "force-newlines", SectionKey: "output", Usage: "If set, default reporter will ensure a newline appears after each test."}, {KeyPath: "R.FdOutput", Name: "fd", SectionKey: "output", Usage: "If set, emits RSpec-style 'format documentation' output instead of Ginkgo's default output.  --fd is exclusive: it overrides -p/-procs and -randomize-all, forcing specs to run serially in declaration order, since fd's hierarchical output can't be rendered sensibly when specs are parallelized or randomized."}, {KeyPath: "R.JSONReport", Name: "json-report", UsageArgument: "filename.json", SectionKey: "output", Usage: "If set, Ginkgo will generate a JSON-formatted test report at the specified location."}, {KeyPath: "R.GoJSONReport", Name: "gojson-report", UsageArgument: "filename.json", SectionKey: "output", Usage: "If set, Ginkgo will generate a Go JSON-formatted test report at the specified location."}, {KeyPath: "R.JUnitReport", Name: "junit-report", UsageArgument: "filename.xml", SectionKey: "output", DeprecatedName: "reportFile", DeprecatedDocLink: "improved-reporting-infrastructure", Usage: "If set, Ginkgo will generate a conformant junit test report in the specified file."}, {KeyPath: "R.TeamcityReport", Name: "teamcity-report", UsageArgument: "filename", SectionKey: "output", Usage: "If set, Ginkgo will generate a Teamcity-formatted test report at the specified location."}, {KeyPath: "D.SlowSpecThresholdWithFLoatUnits", DeprecatedName: "slowSpecThreshold", DeprecatedDocLink: "changed--slowspecthreshold", Usage: "use --slow-spec-threshold instead and pass in a duration string (e.g. '5s', not '5.0')"}, {KeyPath: "D.NoisyPendings", DeprecatedName: "noisyPendings", DeprecatedDocLink: "removed--noisypendings-and--noisyskippings", DeprecatedVersion: "2.0.0"}, {KeyPath: "D.NoisySkippings", DeprecatedName: "noisySkippings", DeprecatedDocLink: "removed--noisypendings-and--noisyskippings", DeprecatedVersion: "2.0.0"}, {KeyPath: "D.SlowSpecThreshold", DeprecatedName: "slow-spec-threshold", SectionKey: "output", Usage: "--slow-spec-threshold has been deprecated and will be removed in a future version of Ginkgo.  This feature has proved to be more noisy than useful.  You can use --poll-progress-after, instead, to get more actionable feedback about potentially slow specs and understand where they might be getting stuck.", DeprecatedVersion: "2.5.0"}, {KeyPath: "D.AlwaysEmitGinkgoWriter", DeprecatedName: "always-emit-ginkgo-writer", SectionKey: "output", Usage: " - use -v instead, or one of Ginkgo's machine-readable report formats to get GinkgoWriter output for passing specs."}}

func BuildTestSuiteFlagSet(suiteConfig *SuiteConfig, reporterConfig *ReporterConfig) (GinkgoFlagSet, error) {
	panic("stub")
}
func VetConfig(flagSet GinkgoFlagSet, suiteConfig SuiteConfig, reporterConfig ReporterConfig) []error {
	panic("stub")
}
func ReconcileFdOutputConfiguration(reporterConfig ReporterConfig, suiteConfig *SuiteConfig, cliConfig *CLIConfig) bool {
	panic("stub")
}

var GinkgoCLISharedFlags = GinkgoFlags{{KeyPath: "C.Recurse", Name: "r", SectionKey: "multiple-suites", Usage: "If set, ginkgo finds and runs test suites under the current directory recursively."}, {KeyPath: "C.SkipPackage", Name: "skip-package", SectionKey: "multiple-suites", DeprecatedName: "skipPackage", DeprecatedDocLink: "changed-command-line-flags", UsageArgument: "comma-separated list of packages", Usage: "A comma-separated list of package names to be skipped.  If any part of the package's path matches, that package is ignored."}, {KeyPath: "C.RequireSuite", Name: "require-suite", SectionKey: "failure", DeprecatedName: "requireSuite", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, Ginkgo fails if there are ginkgo tests in a directory but no invocation of RunSpecs."}, {KeyPath: "C.NumCompilers", Name: "compilers", SectionKey: "multiple-suites", UsageDefaultValue: "0 (will autodetect)", Usage: "When running multiple packages, the number of concurrent compilations to perform."}}
var GinkgoCLIRunAndWatchFlags = GinkgoFlags{{KeyPath: "C.Procs", Name: "procs", SectionKey: "parallel", UsageDefaultValue: "1 (run in series)", Usage: "The number of parallel test nodes to run."}, {KeyPath: "C.Procs", Name: "nodes", SectionKey: "parallel", UsageDefaultValue: "1 (run in series)", Usage: "--nodes is an alias for --procs"}, {KeyPath: "C.Parallel", Name: "p", SectionKey: "parallel", Usage: "If set, ginkgo will run in parallel with an auto-detected number of nodes."}, {KeyPath: "C.AfterRunHook", Name: "after-run-hook", SectionKey: "misc", DeprecatedName: "afterSuiteHook", DeprecatedDocLink: "changed-command-line-flags", Usage: "Command to run when a test suite completes."}, {KeyPath: "C.OutputDir", Name: "output-dir", SectionKey: "output", UsageArgument: "directory", DeprecatedName: "outputdir", DeprecatedDocLink: "improved-profiling-support", Usage: "A location to place all generated profiles and reports."}, {KeyPath: "C.KeepSeparateCoverprofiles", Name: "keep-separate-coverprofiles", SectionKey: "code-and-coverage-analysis", Usage: "If set, Ginkgo does not merge coverprofiles into one monolithic coverprofile.  The coverprofiles will remain in their respective package directories or in -output-dir if set."}, {KeyPath: "C.KeepSeparateReports", Name: "keep-separate-reports", SectionKey: "output", Usage: "If set, Ginkgo does not merge per-suite reports (e.g. -json-report) into one monolithic report for the entire testrun.  The reports will remain in their respective package directories or in -output-dir if set."}, {KeyPath: "D.Stream", DeprecatedName: "stream", DeprecatedDocLink: "removed--stream", DeprecatedVersion: "2.0.0"}, {KeyPath: "D.Notify", DeprecatedName: "notify", DeprecatedDocLink: "removed--notify", DeprecatedVersion: "2.0.0"}}
var GinkgoCLIRunFlags = GinkgoFlags{{KeyPath: "C.KeepGoing", Name: "keep-going", SectionKey: "multiple-suites", DeprecatedName: "keepGoing", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, failures from earlier test suites do not prevent later test suites from running."}, {KeyPath: "C.UntilItFails", Name: "until-it-fails", SectionKey: "debug", DeprecatedName: "untilItFails", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will keep rerunning test suites until a failure occurs."}, {KeyPath: "C.Repeat", Name: "repeat", SectionKey: "debug", UsageArgument: "n", UsageDefaultValue: "0 - i.e. no repetition, run only once", Usage: "The number of times to re-run a test-suite.  Useful for debugging flaky tests.  If set to N the suite will be run N+1 times and will be required to pass each time."}, {KeyPath: "C.RandomizeSuites", Name: "randomize-suites", SectionKey: "order", DeprecatedName: "randomizeSuites", DeprecatedDocLink: "changed-command-line-flags", Usage: "If set, ginkgo will randomize the order in which test suites run."}}
var GinkgoCLIWatchFlags = GinkgoFlags{{KeyPath: "C.Depth", Name: "depth", SectionKey: "watch", Usage: "Ginkgo will watch dependencies down to this depth in the dependency tree."}, {KeyPath: "C.WatchRegExp", Name: "watch-regexp", SectionKey: "watch", DeprecatedName: "watchRegExp", DeprecatedDocLink: "changed-command-line-flags", UsageArgument: "Regular Expression", UsageDefaultValue: `\.go$`, Usage: "Only files matching this regular expression will be watched for changes."}}
var GoBuildFlags = GinkgoFlags{{KeyPath: "Go.Race", Name: "race", SectionKey: "code-and-coverage-analysis", Usage: "enable data race detection. Supported on linux/amd64, linux/ppc64le, linux/arm64, linux/s390x, freebsd/amd64, netbsd/amd64, darwin/amd64, darwin/arm64, and windows/amd64."}, {KeyPath: "Go.Vet", Name: "vet", UsageArgument: "list", SectionKey: "code-and-coverage-analysis", Usage: `Configure the invocation of "go vet" during "go test" to use the comma-separated list of vet checks.  If list is empty (by explicitly passing --vet=""), "go test" runs "go vet" with a curated list of checks believed to be always worth addressing.  If list is "off", "go test" does not run "go vet" at all.  Available checks can be found by running 'go doc cmd/vet'`}, {KeyPath: "Go.Cover", Name: "cover", SectionKey: "code-and-coverage-analysis", Usage: "Enable coverage analysis.	Note that because coverage works by annotating the source code before compilation, compilation and test failures with coverage enabled may report line numbers that don't correspond to the original sources."}, {KeyPath: "Go.CoverMode", Name: "covermode", UsageArgument: "set,count,atomic", SectionKey: "code-and-coverage-analysis", Usage: `Set the mode for coverage analysis for the package[s] being tested. 'set': does this statement run? 'count': how many times does this statement run? 'atomic': like count, but correct in multithreaded tests and more expensive (must use atomic with -race). Sets -cover`}, {KeyPath: "Go.CoverPkg", Name: "coverpkg", UsageArgument: "pattern1,pattern2,pattern3", SectionKey: "code-and-coverage-analysis", Usage: "Apply coverage analysis in each test to packages matching the patterns. 	The default is for each test to analyze only the package being tested. See 'go help packages' for a description of package patterns. Sets -cover."}, {KeyPath: "Go.A", Name: "a", SectionKey: "go-build", Usage: "force rebuilding of packages that are already up-to-date."}, {KeyPath: "Go.ASMFlags", Name: "asmflags", UsageArgument: "'[pattern=]arg list'", SectionKey: "go-build", Usage: "arguments to pass on each go tool asm invocation."}, {KeyPath: "Go.BuildMode", Name: "buildmode", UsageArgument: "mode", SectionKey: "go-build", Usage: "build mode to use. See 'go help buildmode' for more."}, {KeyPath: "Go.BuildVCS", Name: "buildvcs", SectionKey: "go-build", Usage: "adds version control information."}, {KeyPath: "Go.Compiler", Name: "compiler", UsageArgument: "name", SectionKey: "go-build", Usage: "name of compiler to use, as in runtime.Compiler (gccgo or gc)."}, {KeyPath: "Go.GCCGoFlags", Name: "gccgoflags", UsageArgument: "'[pattern=]arg list'", SectionKey: "go-build", Usage: "arguments to pass on each gccgo compiler/linker invocation."}, {KeyPath: "Go.GCFlags", Name: "gcflags", UsageArgument: "'[pattern=]arg list'", SectionKey: "go-build", Usage: "arguments to pass on each go tool compile invocation."}, {KeyPath: "Go.InstallSuffix", Name: "installsuffix", SectionKey: "go-build", Usage: "a suffix to use in the name of the package installation directory, in order to keep output separate from default builds. If using the -race flag, the install suffix is automatically set to raceor, if set explicitly, has _race appended to it. Likewise for the -msan flag.  Using a -buildmode option that requires non-default compile flags has a similar effect."}, {KeyPath: "Go.LDFlags", Name: "ldflags", UsageArgument: "'[pattern=]arg list'", SectionKey: "go-build", Usage: "arguments to pass on each go tool link invocation."}, {KeyPath: "Go.LinkShared", Name: "linkshared", SectionKey: "go-build", Usage: "build code that will be linked against shared libraries previously created with -buildmode=shared."}, {KeyPath: "Go.Mod", Name: "mod", UsageArgument: "mode (readonly, vendor, or mod)", SectionKey: "go-build", Usage: "module download mode to use: readonly, vendor, or mod.  See 'go help modules' for more."}, {KeyPath: "Go.ModCacheRW", Name: "modcacherw", SectionKey: "go-build", Usage: "leave newly-created directories in the module cache read-write instead of making them read-only."}, {KeyPath: "Go.ModFile", Name: "modfile", UsageArgument: "file", SectionKey: "go-build", Usage: `in module aware mode, read (and possibly write) an alternate go.mod file instead of the one in the module root directory. A file named go.mod must still be present in order to determine the module root directory, but it is not accessed. When -modfile is specified, an alternate go.sum file is also used: its path is derived from the -modfile flag by trimming the ".mod" extension and appending ".sum".`}, {KeyPath: "Go.ASan", Name: "asan", SectionKey: "go-build", Usage: "enable interoperation with address sanitizer."}, {KeyPath: "Go.MSan", Name: "msan", SectionKey: "go-build", Usage: "enable interoperation with memory sanitizer. Supported only on linux/amd64, linux/arm64 and only with Clang/LLVM as the host C compiler. On linux/arm64, pie build mode will be used."}, {KeyPath: "Go.N", Name: "n", SectionKey: "go-build", Usage: "print the commands but do not run them."}, {KeyPath: "Go.PkgDir", Name: "pkgdir", UsageArgument: "dir", SectionKey: "go-build", Usage: "install and load all packages from dir instead of the usual locations. For example, when building with a non-standard configuration, use -pkgdir to keep generated packages in a separate location."}, {KeyPath: "Go.Tags", Name: "tags", UsageArgument: "tag,list", SectionKey: "go-build", Usage: "a comma-separated list of build tags to consider satisfied during the build. For more information about build tags, see the description of build constraints in the documentation for the go/build package. (Earlier versions of Go used a space-separated list, and that form is deprecated but still recognized.)"}, {KeyPath: "Go.TrimPath", Name: "trimpath", SectionKey: "go-build", Usage: `remove all file system paths from the resulting executable. Instead of absolute file system paths, the recorded file names will begin with either "go" (for the standard library), or a module path@version (when using modules), or a plain import path (when using GOPATH).`}, {KeyPath: "Go.ToolExec", Name: "toolexec", UsageArgument: "'cmd args'", SectionKey: "go-build", Usage: "a program to use to invoke toolchain programs like vet and asm. For example, instead of running asm, the go command will run cmd args /path/to/asm <arguments for asm>'."}, {KeyPath: "Go.Work", Name: "work", SectionKey: "go-build", Usage: "print the name of the temporary work directory and do not delete it when exiting."}, {KeyPath: "Go.X", Name: "x", SectionKey: "go-build", Usage: "print the commands."}}
var GoBuildOFlags = GinkgoFlags{{KeyPath: "Go.O", Name: "o", SectionKey: "go-build", Usage: "output binary path (including name)."}}
var GoRunFlags = GinkgoFlags{{KeyPath: "Go.CoverProfile", Name: "coverprofile", UsageArgument: "file", SectionKey: "code-and-coverage-analysis", Usage: `Write a coverage profile to the file after all tests have passed. Sets -cover.  Must be passed a filename, not a path.  Use output-dir to control the location of the output.`}, {KeyPath: "Go.BlockProfile", Name: "blockprofile", UsageArgument: "file", SectionKey: "performance-analysis", Usage: `Write a goroutine blocking profile to the specified file when all tests are complete. Preserves test binary.`}, {KeyPath: "Go.BlockProfileRate", Name: "blockprofilerate", UsageArgument: "rate", SectionKey: "performance-analysis", Usage: `Control the detail provided in goroutine blocking profiles by calling runtime.SetBlockProfileRate with rate. See 'go doc runtime.SetBlockProfileRate'. The profiler aims to sample, on average, one blocking event every n nanoseconds the program spends blocked. By default, if -test.blockprofile is set without this flag, all blocking events are recorded, equivalent to -test.blockprofilerate=1.`}, {KeyPath: "Go.CPUProfile", Name: "cpuprofile", UsageArgument: "file", SectionKey: "performance-analysis", Usage: `Write a CPU profile to the specified file before exiting. Preserves test binary.`}, {KeyPath: "Go.MemProfile", Name: "memprofile", UsageArgument: "file", SectionKey: "performance-analysis", Usage: `Write an allocation profile to the file after all tests have passed. Preserves test binary.`}, {KeyPath: "Go.MemProfileRate", Name: "memprofilerate", UsageArgument: "rate", SectionKey: "performance-analysis", Usage: `Enable more precise (and expensive) memory allocation profiles by setting runtime.MemProfileRate. See 'go doc runtime.MemProfileRate'. To profile all memory allocations, use -test.memprofilerate=1.`}, {KeyPath: "Go.MutexProfile", Name: "mutexprofile", UsageArgument: "file", SectionKey: "performance-analysis", Usage: `Write a mutex contention profile to the specified file when all tests are complete. Preserves test binary.`}, {KeyPath: "Go.MutexProfileFraction", Name: "mutexprofilefraction", UsageArgument: "n", SectionKey: "performance-analysis", Usage: `if >= 0, calls runtime.SetMutexProfileFraction()	Sample 1 in n stack traces of goroutines holding a contended mutex.`}, {KeyPath: "Go.Trace", Name: "execution-trace", UsageArgument: "file", ExportAs: "trace", SectionKey: "performance-analysis", Usage: `Write an execution trace to the specified file before exiting.`}}

func VetAndInitializeCLIAndGoConfig(cliConfig CLIConfig, goFlagsConfig GoFlagsConfig) (CLIConfig, GoFlagsConfig, []error) {
	panic("stub")
}
func GenerateGoTestCompileArgs(goFlagsConfig GoFlagsConfig, packageToBuild string, pathToInvocationPath string, preserveSymbols bool) ([]string, error) {
	panic("stub")
}
func GenerateGinkgoTestRunArgs(suiteConfig SuiteConfig, reporterConfig ReporterConfig, goFlagsConfig GoFlagsConfig) ([]string, error) {
	panic("stub")
}
func GenerateGoTestRunArgs(goFlagsConfig GoFlagsConfig) ([]string, error) {
	panic("stub")
}
func BuildRunCommandFlagSet(suiteConfig *SuiteConfig, reporterConfig *ReporterConfig, cliConfig *CLIConfig, goFlagsConfig *GoFlagsConfig) (GinkgoFlagSet, error) {
	panic("stub")
}
func BuildWatchCommandFlagSet(suiteConfig *SuiteConfig, reporterConfig *ReporterConfig, cliConfig *CLIConfig, goFlagsConfig *GoFlagsConfig) (GinkgoFlagSet, error) {
	panic("stub")
}
func BuildBuildCommandFlagSet(cliConfig *CLIConfig, goFlagsConfig *GoFlagsConfig) (GinkgoFlagSet, error) {
	panic("stub")
}
func BuildLabelsCommandFlagSet(cliConfig *CLIConfig) (GinkgoFlagSet, error) {
	panic("stub")
}
