# Gate mutation evidence separately from CRAP scores

Line coverage can be complete while tests still miss a change in behavior.
An opted-in mutation scan fails the owning function for each survived or
uncovered mutant, independently of the coverage-weighted CRAP score and its
regression baseline. Folding the kill ratio into CRAP would let complexity,
the score threshold, or a previous baseline suppress the finding and would
change the meaning of existing scores.

Only reviewed, equivalent survivors can be acknowledged by function identity
and exact edit. Partial mutator runs can still identify failures. They provide
evidence only for the mutations that ran.

See [mutation evidence](../MUTATION_EVIDENCE.md) for report behavior and source
binding.
