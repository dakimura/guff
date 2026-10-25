package internal

type TreeNode struct {
	Node     Node
	Parent   *TreeNode
	Children TreeNodes
}

func (tn *TreeNode) AppendChild(child *TreeNode) {
	panic("stub")
}
func (tn *TreeNode) AncestorNodeChain() Nodes {
	panic("stub")
}

type TreeNodes []*TreeNode

func (tn TreeNodes) Nodes() Nodes {
	panic("stub")
}
func (tn TreeNodes) WithID(id uint) *TreeNode {
	panic("stub")
}
func GenerateSpecsFromTreeRoot(tree *TreeNode) Specs {
	panic("stub")
}
