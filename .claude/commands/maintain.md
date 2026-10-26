Analyze, fix, and ship Markkit maintenance. "Run maintenance" (including
"maintainace" and "maintaiance") requests the complete shipped outcome.

Read `.agents/skills/maintain/SKILL.md`, execute the checklist in
`knowledge/operations/maintenance.md`, then run `.agents/skills/ship/SKILL.md`
through green CI and squash-merge. `$ARGUMENTS` may scope the maintenance area;
an explicit analysis-only request is the only exception to fixing and shipping.

Do not replace fixes, audits, or validation with deferral issues or stop at a
local commit. Preserve security contracts, fix CI failures, never merge red checks.
