package gexec

import (
	"sync"
)

var (
	mu     sync.Mutex
	tmpDir string
)

func Build(packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func BuildWithEnvironment(packagePath string, env []string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func BuildIn(gopath string, packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func doBuild(gopath, packagePath string, env []string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func CompileTest(packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func GetAndCompileTest(packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func CompileTestWithEnvironment(packagePath string, env []string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func GetAndCompileTestWithEnvironment(packagePath string, env []string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func CompileTestIn(gopath string, packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func GetAndCompileTestIn(gopath string, packagePath string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func isLocalPackage(packagePath string) bool {
	panic("stub")
}
func getForTest(gopath, packagePath string, env []string) error {
	panic("stub")
}
func doGet(gopath, packagePath string, env []string, args ...string) error {
	panic("stub")
}
func doCompileTest(gopath, packagePath string, env []string, args ...string) (compiledPath string, err error) {
	panic("stub")
}
func replaceGoPath(environ []string, newGoPath string) []string {
	panic("stub")
}
func newExecutablePath(gopath, packagePath string, suffixes ...string) (string, error) {
	panic("stub")
}
func CleanupBuildArtifacts() {
	panic("stub")
}
func temporaryDirectory() (string, error) {
	panic("stub")
}
