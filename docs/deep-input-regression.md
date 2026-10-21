# Deep-input regression

The pre-0.1.1 debug CLI exited with SIGSEGV (subprocess return code -11) for
`export function f(){return ` followed by 20,000 `(`, `1`, 20,000 `)`, and `;}`.
The reproduction ran in a disposable directory with core dumps disabled.

A standalone probe using the same Tree-sitter dependency and source successfully:

1. Parsed the input with `root_node().has_error() == false`.
2. Traversed all 60,016 nodes using `TreeCursor` without application recursion.
3. Dropped the cursor and tree and exited successfully.

This isolates the failure to the application's recursive analysis rather than
Tree-sitter parsing, iterative cursor traversal, or tree destruction for this
fixture. Version 0.1.1 replaces recursive collection, metrics traversal and
qualified-name construction with iterative operations. Existing metrics tests
preserve function ownership and type-only exclusions. CLI subprocess regressions
exercise parentheses at depths 1, 64, 512, 4,096 and 20,000, and nested function
names. The 20,000-parenthesis case now exits normally with complexity 1.

These checks address stack growth in our walkers; they do not assert unlimited
parser time or memory for every possible TypeScript input.
