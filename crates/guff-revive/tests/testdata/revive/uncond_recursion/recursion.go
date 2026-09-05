// Package uncondrecursion pins which call counts as the function calling
// itself.
//
// Upstream builds a `funcDesc` of (receiver ident, function ident) for the
// declaration and compares it with one built from the call site. The receiver
// has **three** cases, not two:
//
//	case n.Recv == nil:                    rec = nil
//	case … len(n.Recv.List[0].Names) < 1:  rec = &ast.Ident{Name: "_"}
//	default:                               rec = n.Recv.List[0].Names[0]
//
// and `equal` treats nil and non-nil as different:
//
//	receiversAreEqual := (fd.receiverID == nil && other.receiverID == nil) ||
//	    fd.receiverID != nil && other.receiverID != nil &&
//	        fd.receiverID.Name == other.receiverID.Name
//
// guff collapsed the middle case into "no receiver", so a method with an
// **unnamed receiver** looked like a free function: telegraf's
// `func (*configurationOriginal) normalizeInputDatatype(…)` ends with
// `return normalizeInputDatatype(dataType)` — the package function, not
// itself — and guff called that unconditional recursion three times over.
package uncondrecursion

type recvT struct{}

type recvU struct{}

func normalize(s string) string { return s }

// Reported: a free function calling itself.
func loop(n int) int { return loop(n) }

// Reported: a method with a named receiver calling itself.
func (r recvT) selfNamed(n int) int { return r.selfNamed(n) }

// Silent: a named receiver, but the call is the package function.
func (recvT) normalizeNamed(s string) string { return normalize(s) }

// Silent: the telegraf shape — an unnamed receiver and a bare call to the
// package function of the same name.
func (recvT) normalizeUnnamed(s string) string { return normalizeUnnamed(s) }

func normalizeUnnamed(s string) string { return s }

// Silent: an unnamed receiver cannot match a selector, whatever it is named.
func (recvU) viaVar(n int) int {
	var v recvU
	return v.viaVar(n)
}

// Silent: a free function has no receiver, so a method call never matches.
func viaOther(n int) int {
	var v recvU
	return v.viaOther(n)
}

func (recvU) viaOther(n int) int { return n }
