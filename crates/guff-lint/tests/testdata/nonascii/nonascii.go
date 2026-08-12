// Package nonascii has non-ASCII identifiers, comments and
// literals. No gated OSS target has any — measured by
// corpus/shapes.py — so the shape lives here instead.
//
// Two different unit systems meet in this file:
//
//   - a finding's **column** is a 1-based *byte* offset
//     (go/token), so every multi-byte character before a
//     finding shifts it;
//   - lll's line length is a *rune* count
//     (utf8.RuneCountInString), so the same characters do
//     not shift that.
//
// A tool that picks one unit for both is wrong twice. This
// is the split that made godox panic on caddy, slicing a
// comment at a non-boundary byte.
//
// Every line below is under 60 runes, so the only lll
// findings are the ones this file asks for on purpose.
package nonascii

import (
	"fmt"
	"os"
)

// 日本語の識別子。Columns after this are past 21 bytes.
func 挨拶(名前 string) string {
	return fmt.Sprintf("こんにちは、%s", 名前)
}

// An unchecked error whose column follows multi-byte text.
func Ошибка() {
	日本語 := "テキスト"
	os.Chdir(日本語)
}

// Two findings on one line, both past a multi-byte prefix.
func Σχόλιο() {
	αβγ := 1
	αβγ = 2
	_ = αβγ
	os.Chdir("каталог")
}

// TODO: godox keyword sitting after ✓ a 3-byte character.
func Emoji() {
	// 🎌 is 4 bytes and one rune.
	fmt.Println("絵文字🎌")
}

// A wrong printf operand, on a line starting multi-byte.
func Printf() {
	名前 := "ok"
	fmt.Printf("%d", 名前)
}

// The discriminator: the kana line below is 34 runes but
// 94 bytes. lll must stay silent on it — counting bytes
// would put it over the limit of 60.
func Runes() {
	// あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほ
	_ = 0
}

// The control: 61 runes of ASCII, over the limit either way.
func Control() { _ = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
