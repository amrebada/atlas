// Atlas - builds the initial prompt that invokes a Claude Code skill.

import type { ClaudeSkill } from "../../types";

// Prose invocation ("Use the <name> skill.") rather than a slash command:
// slash-as-initial-arg is not documented as reliable, prose is - and it
// matches the Pilot precedent ("Use the atlas skill ..."). Works for
// namespaced plugin skills too ("Use the code-review:code-review skill.").
export function buildSkillPrompt(skill: ClaudeSkill, input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return `Use the ${skill.name} skill.`;
  return `Use the ${skill.name} skill.\n\n${trimmed}`;
}
