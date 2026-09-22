// Package siblings isolates one question: when two responses share a name, is
// a close written on one of them a close on the other?
//
// The walk here is an AST walk, so the only handle it has on a close is the
// name in `resp.Body.Close()`. Upstream works on `ssa.Value`s, where every
// `:=` is its own `Alloc` and a `Phi` joins only values that one path can
// carry together — so two `resp`s in blocks neither of which encloses the
// other are never joined, and a close on the second says nothing about the
// first.
//
// beats writes exactly that: `authenticator_test.go` binds one `resp` from
// `rt.RoundTrip` and another from `client.Get` in two `if` bodies of the same
// `t.Run` closure, and closes only the second. The first leak was silent.
//
// The last four functions are the other direction: one variable really
// assigned twice, which upstream *does* join, and a name reused for something
// that is not a response at all.
package siblings

import "net/http"

// Reported at the first: two `:=`, neither block encloses the other.
func siblingFirstOpen(url string, open bool) {
	if open {
		resp, err := http.Get(url)
		_ = err
		_ = resp
	}
	if url != "" {
		resp, err := http.Get(url + "/x")
		if err == nil {
			_ = resp.Body.Close()
		}
	}
}

// Reported at the second. The order matters: before this fix only this
// direction worked, because the close was seen before the second binding
// replaced the entry.
func siblingSecondOpen(url string, open bool) {
	if url != "" {
		resp, err := http.Get(url)
		if err == nil {
			_ = resp.Body.Close()
		}
	}
	if open {
		resp, err := http.Get(url + "/x")
		_ = err
		_ = resp
	}
}

// Reported twice.
func siblingBothOpen(url string, open bool) {
	if open {
		resp, _ := http.Get(url)
		_ = resp
	}
	if url != "" {
		resp, _ := http.Get(url + "/x")
		_ = resp
	}
}

// Silent: two bindings, two closes.
func siblingBothClosed(url string, alt bool) {
	if alt {
		resp, _ := http.Get(url)
		_ = resp.Body.Close()
	}
	if !alt {
		resp, _ := http.Get(url + "/x")
		_ = resp.Body.Close()
	}
}

// Reported at the `if` arm: the two arms of one `if`/`else` are still two
// bindings, and the `else` arm's close is its own.
func ifElseDistinct(url string, alt bool) {
	if alt {
		resp, _ := http.Get(url)
		_ = resp
	} else {
		resp, _ := http.Get(url + "/x")
		_ = resp.Body.Close()
	}
}

// Reported at the first: the same pair inside a loop body.
func siblingInLoop(urls []string) {
	for _, u := range urls {
		if u != "" {
			resp, _ := http.Get(u)
			_ = resp
		}
		if u != "x" {
			resp, _ := http.Get(u)
			_ = resp.Body.Close()
		}
	}
}

// Reported at the outer: an inner block's `:=` shadows it, and the inner
// close belongs to the inner binding.
func blockShadowOuterOpen(url string) {
	resp, _ := http.Get(url)
	{
		resp, _ := http.Get(url + "/x")
		_ = resp.Body.Close()
	}
	_ = resp
}

// Reported at the inner, for the same reason read the other way.
func blockShadowInnerOpen(url string) {
	resp, _ := http.Get(url)
	_ = resp.Body.Close()
	{
		resp, _ := http.Get(url + "/x")
		_ = resp
	}
}

// Reported at the outer: a shadowing `var` is a binding too.
func varShadow(url string) {
	resp, _ := http.Get(url)
	_ = resp
	{
		var resp, _ = http.Get(url + "/x")
		_ = resp.Body.Close()
	}
}

// Reported at the `if` arm: the name is reused for a string, which binds
// nothing the close could reach — and the response still leaks.
func reusedForNonResponse(url string, alt bool) {
	if alt {
		resp, _ := http.Get(url)
		_ = resp
	}
	if !alt {
		resp := url
		_ = resp
	}
}

// -- the other direction: one variable, really assigned twice.

// Silent: one `resp`, assigned in both arms of an `if`/`else`, closed after.
// Upstream joins the two values in one `Phi` and the close settles both.
func oneVarIfElse(url string, alt bool) error {
	var resp *http.Response
	var err error
	if alt {
		resp, err = http.Get(url)
	} else {
		resp, err = http.Get(url + "/x")
	}
	if err != nil {
		return err
	}
	return resp.Body.Close()
}

// Silent: `:=` then `=` on the same variable — still one variable.
func oneVarRedefined(url string, alt bool) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	if alt {
		resp, err = http.Get(url + "/x")
		if err != nil {
			return err
		}
	}
	return resp.Body.Close()
}

// Silent: one variable rebound each time round a loop and closed each time.
func oneVarInLoop(urls []string) error {
	var resp *http.Response
	var err error
	for _, u := range urls {
		resp, err = http.Get(u)
		if err != nil {
			return err
		}
		if err := resp.Body.Close(); err != nil {
			return err
		}
	}
	return nil
}
