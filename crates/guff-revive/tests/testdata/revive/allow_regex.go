// Package allowregex exercises the `allowRegex` argument shared by
// unused-parameter and unused-receiver.
package allowregex

// T is a receiver holder.
type T struct{}

// Underscored is reported only when allowRegex does not accept `_ctx`.
func Underscored(_ctx int) {}

// Plain is reported under every configuration: no regex here accepts `ctx`.
func Plain(ctx int) {}

// Used is never reported: the body references the parameter.
func Used(n int) int { return n }

// Blank is never reported: upstream drops `_` before the regex is consulted.
func Blank(_ int) {}

// UnderscoredRecv is reported only when allowRegex does not accept `_t`.
func (_t T) UnderscoredRecv() {}

// PlainRecv is reported under every configuration.
func (t T) PlainRecv() {}

// UsedRecv is never reported.
func (t T) UsedRecv() T { return t }
