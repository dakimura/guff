package gosec_ok

import (
	"crypto/sha256"
	"hash"
	"net"
	"os/exec"
)

func ok() hash.Hash {
	return sha256.New()
}

func okListen() {
	_, _ = net.Listen("tcp", "127.0.0.1:8080")
}

func okExec() {
	_ = exec.Command("ls", "-la")
}
