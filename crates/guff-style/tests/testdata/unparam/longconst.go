// Package longconst is unparam's "always receives" over long string constants.
//
// The comparison is `constant.Compare(a, token.EQL, b)`, on the values. It is
// *not* `Value.String() == Value.String()`: that method is documented as "a
// short, quoted (human-readable) form … for String values the result may be a
// shortened string", and it shortens at 72 characters. Four different scripts
// that open with the same long line then compare equal and the parameter looks
// constant — k6 `internal/js/runner_test.go:385` passes four distinct
// JavaScript sources sharing their first sixty-odd characters.
//
// The shortened form is still what the message quotes, which is upstream's
// `constValueString`.
//
// unparam wants at least four call sites before it will call a value constant,
// so each function below has exactly four.
package longconst

func sink(string) {}

// silent — four distinct scripts that share a long opening line.
func sharedPrefix(data string) { sink(data) }

func useSharedPrefix() {
	sharedPrefix(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { return 42; }
	`)
	sharedPrefix(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.default = function(data) { throw new Error("a") }
	`)
	sharedPrefix(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { }
	`)
	sharedPrefix(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = async function() { return 1 }
	`)
}

// fires — the same long script four times, so the value really is constant.
func sameLongScript(data string) { sink(data) }

func useSameLongScript() {
	sameLongScript(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { return 42; }
	`)
	sameLongScript(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { return 42; }
	`)
	sameLongScript(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { return 42; }
	`)
	sameLongScript(`
	exports.options = { setupTimeout: "1s", teardownTimeout: "1s" };
	exports.setup = function() { return 42; }
	`)
}

// silent — short literals that differ, the case the shortening never reached.
func shortDistinct(data string) { sink(data) }

func useShortDistinct() {
	shortDistinct("a")
	shortDistinct("b")
	shortDistinct("c")
	shortDistinct("d")
}
