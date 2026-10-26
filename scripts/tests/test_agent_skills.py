"""Agent skills live once in `.agents/skills/`; `.claude/skills` is a symlink to
it so Claude Code and other agents read the same files. Each skill must carry
the frontmatter that makes it discoverable.
"""

import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
AGENTS = REPO / ".agents" / "skills"
CLAUDE = REPO / ".claude" / "skills"


class AgentSkillsTest(unittest.TestCase):
    def test_claude_skills_is_symlink_to_agents_skills(self):
        self.assertTrue(CLAUDE.is_symlink())
        self.assertEqual(CLAUDE.resolve(), AGENTS.resolve())

    def test_every_skill_has_frontmatter(self):
        skills = sorted(AGENTS.glob("*/SKILL.md"))
        self.assertTrue(skills)
        for skill in skills:
            text = skill.read_text()
            with self.subTest(skill=skill.parent.name):
                self.assertTrue(text.startswith("---\n"), "missing frontmatter")
                head = text.split("\n---\n", 1)[0]
                self.assertIn(f"name: {skill.parent.name}\n", head)
                self.assertIn("description: ", head)

    def test_maintain_skill_exists(self):
        self.assertTrue((AGENTS / "maintain" / "SKILL.md").is_file())


if __name__ == "__main__":
    unittest.main()
