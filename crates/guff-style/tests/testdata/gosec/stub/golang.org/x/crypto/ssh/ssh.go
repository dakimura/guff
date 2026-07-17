package ssh

type HostKeyCallback func(hostname string, remote string, key []byte) error

func InsecureIgnoreHostKey() HostKeyCallback { return nil }
