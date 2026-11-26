package main

import "os/exec"

func main() {
	exec.Command("ls")
	exec.Command("/bin/ls", "arg1")
}
