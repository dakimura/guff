// Upstream's reflectvaluecompare testdata (x/tools v0.52.0), plus aliases,
// defined types, parentheses, pointers and calls.
package a

import (
	"reflect"
)

func f() {
	var x, y reflect.Value
	var a, b interface{}
	_ = x == y // want `avoid using == with reflect.Value`
	_ = x == a // want `avoid using == with reflect.Value`
	_ = a == x // want `avoid using == with reflect.Value`
	_ = a == b

	// Comparing to reflect.Value{} is ok.
	_ = a == reflect.Value{}
	_ = reflect.Value{} == a
	_ = reflect.Value{} == reflect.Value{}
}
func g() {
	var x, y reflect.Value
	var a, b interface{}
	_ = x != y // want `avoid using != with reflect.Value`
	_ = x != a // want `avoid using != with reflect.Value`
	_ = a != x // want `avoid using != with reflect.Value`
	_ = a != b

	// Comparing to reflect.Value{} is ok.
	_ = a != reflect.Value{}
	_ = reflect.Value{} != a
	_ = reflect.Value{} != reflect.Value{}
}
func h() {
	var x, y reflect.Value
	var a, b interface{}
	reflect.DeepEqual(x, y) // want `avoid using reflect.DeepEqual with reflect.Value`
	reflect.DeepEqual(x, a) // want `avoid using reflect.DeepEqual with reflect.Value`
	reflect.DeepEqual(a, x) // want `avoid using reflect.DeepEqual with reflect.Value`
	reflect.DeepEqual(a, b)

	// Comparing to reflect.Value{} is ok.
	reflect.DeepEqual(reflect.Value{}, a)
	reflect.DeepEqual(a, reflect.Value{})
	reflect.DeepEqual(reflect.Value{}, reflect.Value{})
}

type alias = reflect.Value

type defined reflect.Value

func val() reflect.Value { return reflect.Value{} }

func extra(x reflect.Value, p *reflect.Value, al alias, d1, d2 defined, a any) bool {
	_ = al == al                // alias: reported
	_ = d1 == d2                // defined type: not reported
	_ = (x) == a                // parenthesised operand
	_ = (reflect.Value{}) == a  // parenthesised literal is not a CompositeLit
	_ = alias{} == a            // a literal spelled through the alias
	_ = p == nil                // pointer: not reported
	_ = *p == x                 // dereference
	_ = val() != x              // call result
	_ = x.Kind() == reflect.Int // the Kind, not the Value
	_ = x == x && al != x       // both operands of && are comparisons
	if x == (reflect.Value{}) {
		return true
	}
	eq := reflect.DeepEqual
	_ = eq(x, x)                    // a func value has no static callee: not reported
	_ = reflect.DeepEqual(&x, a)    // pointer
	return reflect.DeepEqual(al, a) // alias
}
