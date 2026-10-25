package cmp

import (
	"reflect"
	"regexp"
)

type Option interface {
	filter(s *state, t reflect.Type, vx, vy reflect.Value) applicableOption
}
type applicableOption interface {
	Option
	apply(s *state, vx, vy reflect.Value)
}
type coreOption interface {
	Option
	isCore()
}
type core struct{}

func (core) isCore() {
	panic("stub")
}

type Options []Option

func (opts Options) filter(s *state, t reflect.Type, vx, vy reflect.Value) (out applicableOption) {
	panic("stub")
}
func (opts Options) apply(s *state, _, _ reflect.Value) {
	panic("stub")
}
func (opts Options) String() string {
	panic("stub")
}
func FilterPath(f func(Path) bool, opt Option) Option {
	panic("stub")
}

type pathFilter struct {
	core
	fnc func(Path) bool
	opt Option
}

func (f pathFilter) filter(s *state, t reflect.Type, vx, vy reflect.Value) applicableOption {
	panic("stub")
}
func (f pathFilter) String() string {
	panic("stub")
}
func FilterValues(f interface{}, opt Option) Option {
	panic("stub")
}

type valuesFilter struct {
	core
	typ reflect.Type
	fnc reflect.Value
	opt Option
}

func (f valuesFilter) filter(s *state, t reflect.Type, vx, vy reflect.Value) applicableOption {
	panic("stub")
}
func (f valuesFilter) String() string {
	panic("stub")
}
func Ignore() Option {
	panic("stub")
}

type ignore struct{ core }

func (ignore) isFiltered() bool {
	panic("stub")
}
func (ignore) filter(_ *state, _ reflect.Type, _, _ reflect.Value) applicableOption {
	panic("stub")
}
func (ignore) apply(s *state, _, _ reflect.Value) {
	panic("stub")
}
func (ignore) String() string {
	panic("stub")
}

type validator struct{ core }

func (validator) filter(_ *state, _ reflect.Type, vx, vy reflect.Value) applicableOption {
	panic("stub")
}
func (validator) apply(s *state, vx, vy reflect.Value) {
	panic("stub")
}

const identRx = `[_\p{L}][_\p{L}\p{N}]*`

var identsRx = regexp.MustCompile(`^` + identRx + `(\.` + identRx + `)*$`)

func Transformer(name string, f interface{}) Option {
	panic("stub")
}

type transformer struct {
	core
	name string
	typ  reflect.Type
	fnc  reflect.Value
}

func (tr *transformer) isFiltered() bool {
	panic("stub")
}
func (tr *transformer) filter(s *state, t reflect.Type, _, _ reflect.Value) applicableOption {
	panic("stub")
}
func (tr *transformer) apply(s *state, vx, vy reflect.Value) {
	panic("stub")
}
func (tr transformer) String() string {
	panic("stub")
}
func Comparer(f interface{}) Option {
	panic("stub")
}

type comparer struct {
	core
	typ reflect.Type
	fnc reflect.Value
}

func (cm *comparer) isFiltered() bool {
	panic("stub")
}
func (cm *comparer) filter(_ *state, t reflect.Type, _, _ reflect.Value) applicableOption {
	panic("stub")
}
func (cm *comparer) apply(s *state, vx, vy reflect.Value) {
	panic("stub")
}
func (cm comparer) String() string {
	panic("stub")
}
func Exporter(f func(reflect.Type) bool) Option {
	panic("stub")
}

type exporter func(reflect.Type) bool

func (exporter) filter(_ *state, _ reflect.Type, _, _ reflect.Value) applicableOption {
	panic("stub")
}
func AllowUnexported(types ...interface{}) Option {
	panic("stub")
}

type Result struct {
	_     [0]func()
	flags resultFlags
}

func (r Result) Equal() bool {
	panic("stub")
}
func (r Result) ByIgnore() bool {
	panic("stub")
}
func (r Result) ByMethod() bool {
	panic("stub")
}
func (r Result) ByFunc() bool {
	panic("stub")
}
func (r Result) ByCycle() bool {
	panic("stub")
}

type resultFlags uint

const (
	_ resultFlags = (1 << iota) / 2
	reportEqual
	reportUnequal
	reportByIgnore
	reportByMethod
	reportByFunc
	reportByCycle
)

func Reporter(r interface {
	PushStep(PathStep)
	Report(Result)
	PopStep()
}) Option {
	panic("stub")
}

type reporter struct{ reporterIface }
type reporterIface interface {
	PushStep(PathStep)
	Report(Result)
	PopStep()
}

func (reporter) filter(_ *state, _ reflect.Type, _, _ reflect.Value) applicableOption {
	panic("stub")
}
func normalizeOption(src Option) Option {
	panic("stub")
}
func flattenOptions(dst, src Options) Options {
	panic("stub")
}
