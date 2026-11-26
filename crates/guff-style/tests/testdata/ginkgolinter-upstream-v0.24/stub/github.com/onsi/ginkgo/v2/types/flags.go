package types

import (
	"flag"
	"reflect"
)

type GinkgoFlag struct {
	Name              string
	KeyPath           string
	SectionKey        string
	Usage             string
	UsageArgument     string
	UsageDefaultValue string
	DeprecatedName    string
	DeprecatedDocLink string
	DeprecatedVersion string
	ExportAs          string
	AlwaysExport      bool
}
type GinkgoFlags []GinkgoFlag

func (f GinkgoFlags) CopyAppend(flags ...GinkgoFlag) GinkgoFlags {
	panic("stub")
}
func (f GinkgoFlags) WithPrefix(prefix string) GinkgoFlags {
	panic("stub")
}
func (f GinkgoFlags) SubsetWithNames(names ...string) GinkgoFlags {
	panic("stub")
}

type GinkgoFlagSection struct {
	Key         string
	Style       string
	Succinct    bool
	Heading     string
	Description string
}
type GinkgoFlagSections []GinkgoFlagSection

func (gfs GinkgoFlagSections) Lookup(key string) (GinkgoFlagSection, bool) {
	panic("stub")
}

type GinkgoFlagSet struct {
	flags               GinkgoFlags
	bindings            any
	sections            GinkgoFlagSections
	extraGoFlagsSection GinkgoFlagSection
	flagSet             *flag.FlagSet
}

func NewGinkgoFlagSet(flags GinkgoFlags, bindings any, sections GinkgoFlagSections) (GinkgoFlagSet, error) {
	panic("stub")
}
func NewAttachedGinkgoFlagSet(flagSet *flag.FlagSet, flags GinkgoFlags, bindings any, sections GinkgoFlagSections, extraGoFlagsSection GinkgoFlagSection) (GinkgoFlagSet, error) {
	panic("stub")
}
func bindFlagSet(f GinkgoFlagSet, flagSet *flag.FlagSet) (GinkgoFlagSet, error) {
	panic("stub")
}
func (f GinkgoFlagSet) IsZero() bool {
	panic("stub")
}
func (f GinkgoFlagSet) Completion(arg string) map[string]string {
	panic("stub")
}
func (f GinkgoFlagSet) WasSet(name string) bool {
	panic("stub")
}
func (f GinkgoFlagSet) Lookup(name string) *flag.Flag {
	panic("stub")
}
func (f GinkgoFlagSet) Parse(args []string) ([]string, error) {
	panic("stub")
}
func (f GinkgoFlagSet) ValidateDeprecations(deprecationTracker *DeprecationTracker) {
	panic("stub")
}
func (f GinkgoFlagSet) Usage() string {
	panic("stub")
}
func (f GinkgoFlagSet) substituteUsage() {
	panic("stub")
}
func valueAtKeyPath(root any, keyPath string) (reflect.Value, bool) {
	panic("stub")
}
func (f GinkgoFlagSet) usageForSection(section GinkgoFlagSection) string {
	panic("stub")
}
func (f GinkgoFlagSet) usageForFlag(flag GinkgoFlag, style string) string {
	panic("stub")
}
func (f GinkgoFlagSet) usageForGoFlag(goFlag *flag.Flag) string {
	panic("stub")
}

type stringSliceVar struct{ slice reflect.Value }

func (ssv stringSliceVar) String() string {
	panic("stub")
}
func (ssv stringSliceVar) Set(s string) error {
	panic("stub")
}
func GenerateFlagArgs(flags GinkgoFlags, bindings any) ([]string, error) {
	panic("stub")
}
