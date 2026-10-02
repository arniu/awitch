# Blind review

1. Send the full change set to two independent subagents; give each the goal,
   intended behaviour, acceptance criteria, and constraints — never the
   approach, prior fixes, or prior findings.
2. Keep the reviews independent — validate every finding against the code, fix
   confirmed issues, then run a fresh two-reviewer round.
3. When a fresh two-reviewer round finds nothing new, reduce to one fresh
   reviewer for local refinement.
4. If the cycle does not converge, stop patching and step back to the design:
   boundaries, invariants, state flow, ownership. If it still cannot settle,
   report the open issues, tradeoffs, and evidence to the user.
