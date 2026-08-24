package p

import (
	"fmt"
	"os/exec"
	"strings"
)

// depguard says two different things: one for an import that a rule denies, and
// one for an import that is simply not on a rule's allow list. Only the first
// carries the rule's `desc`.

func Denied() {
	fmt.Println("x")
}

func NotAllowed() {
	_ = strings.Contains("a", "b")
	_, _ = exec.LookPath("ls")
}
