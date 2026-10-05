// revive v1.17.0 `handleConfig`: a `disable-line` / `disable-next-line` that
// lands inside a range already disabled changes nothing, so it no longer
// closes the range. v1.15.0 recorded the redundant disable and then the
// matching enable, which re-enabled the rule from that line on: `c_d` and
// `g_h` were reported. Upstream's own directive testdata has no such shape.
package insiderange

//revive:disable:var-naming
var a_b = 1 //revive:disable-line:var-naming
var c_d = 2

//revive:enable:var-naming
var e_f = 3

//revive:disable:var-naming
//revive:disable-next-line:var-naming
var g_h = 4
var i_j = 5

//revive:enable:var-naming
var k_l = 6
