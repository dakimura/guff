package ginkgo

import (
	"io"
	"github.com/go-logr/logr"
	"github.com/onsi/ginkgo/v2/internal"
	"github.com/onsi/ginkgo/v2/internal/parallel_support"
	"github.com/onsi/ginkgo/v2/types"
)

const GINKGO_VERSION = types.VERSION

var flagSet types.GinkgoFlagSet
var deprecationTracker = types.NewDeprecationTracker()
var suiteConfig = types.NewDefaultSuiteConfig()
var reporterConfig = types.NewDefaultReporterConfig()
var outputInterceptor internal.OutputInterceptor
var client parallel_support.Client

func init() {
	panic("stub")
}
func exitIfErr(err error) {
	panic("stub")
}
func exitIfErrors(errors []error) {
	panic("stub")
}

type GinkgoWriterInterface interface {
	io.Writer
	Print(a ...any)
	Printf(format string, a ...any)
	Println(a ...any)
	TeeTo(writer io.Writer)
	ClearTeeWriters()
}
type SpecContext = internal.SpecContext

var GinkgoWriter GinkgoWriterInterface
var GinkgoLogr logr.Logger

type GinkgoTestingT interface{ Fail() }

func GinkgoConfiguration() (types.SuiteConfig, types.ReporterConfig) {
	panic("stub")
}
func GinkgoRandomSeed() int64 {
	panic("stub")
}
func GinkgoParallelProcess() int {
	panic("stub")
}
func GinkgoHelper() {
	panic("stub")
}
func GinkgoLabelFilter() string {
	panic("stub")
}
func GinkgoSemVerFilter() string {
	panic("stub")
}
func PauseOutputInterception() {
	panic("stub")
}
func ResumeOutputInterception() {
	panic("stub")
}
func RunSpecs(t GinkgoTestingT, description string, args ...any) bool {
	panic("stub")
}
func extractSuiteConfiguration(args []any) (Labels, SemVerConstraints, ComponentSemVerConstraints, types.AroundNodes) {
	panic("stub")
}
func getwd() (string, error) {
	panic("stub")
}
func PreviewSpecs(description string, args ...any) Report {
	panic("stub")
}
func Skip(message string, callerSkip ...int) {
	panic("stub")
}
func Fail(message string, callerSkip ...int) {
	panic("stub")
}
func AbortSuite(message string, callerSkip ...int) {
	panic("stub")
}

type ignorablePanic interface{ GinkgoRecoverShouldIgnoreThisPanic() }

func GinkgoRecover() {
	panic("stub")
}
func pushNode(node internal.Node, errors []error) bool {
	panic("stub")
}

type NodeArgsTransformer func(nodeType types.NodeType, offset Offset, text string, args []any) (string, []any, []error)

func AddTreeConstructionNodeArgsTransformer(transformer NodeArgsTransformer) func() {
	panic("stub")
}
func Describe(text string, args ...any) bool {
	panic("stub")
}
func FDescribe(text string, args ...any) bool {
	panic("stub")
}
func PDescribe(text string, args ...any) bool {
	panic("stub")
}

var XDescribe = PDescribe
var Context, FContext, PContext, XContext = Describe, FDescribe, PDescribe, XDescribe

func When(text string, args ...any) bool {
	panic("stub")
}
func FWhen(text string, args ...any) bool {
	panic("stub")
}
func PWhen(text string, args ...any) bool {
	panic("stub")
}

var XWhen = PWhen

func It(text string, args ...any) bool {
	panic("stub")
}
func FIt(text string, args ...any) bool {
	panic("stub")
}
func PIt(text string, args ...any) bool {
	panic("stub")
}

var XIt = PIt
var Specify, FSpecify, PSpecify, XSpecify = It, FIt, PIt, XIt

func By(text string, callback ...func()) {
	panic("stub")
}
func BeforeSuite(body any, args ...any) bool {
	panic("stub")
}
func AfterSuite(body any, args ...any) bool {
	panic("stub")
}
func SynchronizedBeforeSuite(process1Body any, allProcessBody any, args ...any) bool {
	panic("stub")
}
func SynchronizedAfterSuite(allProcessBody any, process1Body any, args ...any) bool {
	panic("stub")
}
func BeforeEach(args ...any) bool {
	panic("stub")
}
func JustBeforeEach(args ...any) bool {
	panic("stub")
}
func AfterEach(args ...any) bool {
	panic("stub")
}
func JustAfterEach(args ...any) bool {
	panic("stub")
}
func BeforeAll(args ...any) bool {
	panic("stub")
}
func AfterAll(args ...any) bool {
	panic("stub")
}
func DeferCleanup(args ...any) {
	panic("stub")
}
func AttachProgressReporter(reporter func() string) func() {
	panic("stub")
}
