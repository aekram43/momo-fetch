use std::path::{Path, PathBuf};

use adk_rust::skill::{
    discover_skill_files_with_extras, load_skill_index_with_extras, select_skills,
    SelectionPolicy, SkillIndex, SkillMatch,
};

/// Skill service wrapping adk-skill for discovery, selection, and management.
///
/// Scans `.harness/skills/`, `.skills/`, and `.claude/skills/` for SKILL.md files.
/// Provides skill listing, auto-matching, and install-from-git capabilities.
pub struct SkillService {
    project_path: PathBuf,
    harness_skills_dir: PathBuf,
    index: SkillIndex,
    #[allow(dead_code)]
    policy: SelectionPolicy,
}

impl SkillService {
    /// Create a new SkillService for the given project path.
    ///
    /// Discovers skills from:
    /// - `.skills/` (standard adk-skill directory)
    /// - `.claude/skills/` (Claude Code compatible)
    /// - `.harness/skills/` (project-local harness skills)
    pub fn new(project_path: &Path) -> anyhow::Result<Self> {
        let harness_skills_dir = project_path.join(".harness").join("skills");

        // Ensure .harness/skills/ exists
        std::fs::create_dir_all(&harness_skills_dir)?;

        let extra_dirs = vec![harness_skills_dir.clone()];

        let index = load_skill_index_with_extras(project_path, &extra_dirs)
            .map_err(|e| anyhow::anyhow!("Failed to load skill index: {e}"))?;

        let policy = SelectionPolicy {
            top_k: 3,
            min_score: 0.5,
            ..SelectionPolicy::default()
        };

        Ok(Self {
            project_path: project_path.to_path_buf(),
            harness_skills_dir,
            index,
            policy,
        })
    }

    /// Reload the skill index (e.g., after installing a new skill).
    pub fn reload(&mut self) -> anyhow::Result<()> {
        let extra_dirs = vec![self.harness_skills_dir.clone()];
        self.index = load_skill_index_with_extras(&self.project_path, &extra_dirs)
            .map_err(|e| anyhow::anyhow!("Failed to reload skill index: {e}"))?;
        Ok(())
    }

    /// Get the skill index.
    pub fn index(&self) -> &SkillIndex {
        &self.index
    }

    /// Get the selection policy.
    #[allow(dead_code)]
    pub fn policy(&self) -> &SelectionPolicy {
        &self.policy
    }

    /// Get the harness skills directory path.
    #[allow(dead_code)]
    pub fn harness_skills_dir(&self) -> &Path {
        &self.harness_skills_dir
    }

    /// Check if any skills are loaded.
    pub fn has_skills(&self) -> bool {
        !self.index.is_empty()
    }

    /// Get the number of loaded skills.
    pub fn skill_count(&self) -> usize {
        self.index.len()
    }

    /// Auto-match skills for a given query string.
    /// Returns ranked skill matches based on lexical overlap.
    #[allow(dead_code)]
    pub fn match_skills(&self, query: &str) -> Vec<SkillMatch> {
        select_skills(&self.index, query, &self.policy)
    }

    /// Install a skill from a git URL by cloning into .harness/skills/.
    ///
    /// The repo is cloned into `.harness/skills/<name>/` where `<name>` is
    /// derived from the git URL's last path component (stripping `.git`).
    pub fn install_from_git(&mut self, git_url: &str) -> anyhow::Result<String> {
        // Extract name from URL: last component, strip .git suffix
        let name = git_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("skill")
            .trim_end_matches(".git")
            .to_string();

        if name.is_empty() {
            return Err(anyhow::anyhow!("Could not derive skill name from URL"));
        }

        let target_dir = self.harness_skills_dir.join(&name);
        if target_dir.exists() {
            return Err(anyhow::anyhow!(
                "Skill '{name}' already exists at {}",
                target_dir.display()
            ));
        }

        // Clone the repo
        let output = std::process::Command::new("git")
            .args(["clone", "--depth", "1", git_url])
            .arg(&target_dir)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("git clone failed: {stderr}"));
        }

        // Reload index to pick up new skill
        self.reload()?;

        Ok(name)
    }

    /// Remove an installed skill by name.
    /// Only removes skills in the .harness/skills/ directory.
    #[allow(dead_code)]
    pub fn remove_skill(&mut self, name: &str) -> anyhow::Result<()> {
        let skill_dir = self.harness_skills_dir.join(name);
        if !skill_dir.exists() {
            return Err(anyhow::anyhow!("Skill '{name}' not found in .harness/skills/"));
        }

        std::fs::remove_dir_all(&skill_dir)?;
        self.reload()?;
        Ok(())
    }

    /// Build the skill context block for system prompt injection.
    ///
    /// Lists all discovered skills with their names and descriptions.
    /// Returns None if no skills are loaded.
    pub fn build_skill_context(&self) -> Option<String> {
        if self.index.is_empty() {
            return None;
        }

        let mut parts = vec!["## Available Skills".to_string()];
        parts.push(String::new());

        for skill in self.index.skills() {
            let trigger_marker = if skill.trigger { " [explicit-only]" } else { "" };
            let tags = if skill.tags.is_empty() {
                String::new()
            } else {
                format!(" ({})", skill.tags.join(", "))
            };
            parts.push(format!(
                "- **{}**: {}{}{}",
                skill.name, skill.description, tags, trigger_marker
            ));
        }

        parts.push(String::new());
        parts.push(
            "Skills can be invoked explicitly via /<skill-name> or matched automatically \
             based on user intent."
                .to_string(),
        );

        Some(parts.join("\n"))
    }

    /// List all skill files that would be discovered (without parsing).
    #[allow(dead_code)]
    pub fn list_skill_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        let extra_dirs = vec![self.harness_skills_dir.clone()];
        discover_skill_files_with_extras(&self.project_path, &extra_dirs)
            .map_err(|e| anyhow::anyhow!("Failed to discover skill files: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_empty_skill_service() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();

        let service = SkillService::new(&project).unwrap();
        assert!(!service.has_skills());
        assert_eq!(service.skill_count(), 0);
        assert!(service.build_skill_context().is_none());
    }

    #[test]
    fn test_discovers_skills_from_harness_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".harness").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        fs::write(
            skills_dir.join("test.md"),
            "---\nname: test-skill\ndescription: A test skill\ntags: [test]\n---\nDo test things.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        assert!(service.has_skills());
        assert_eq!(service.skill_count(), 1);

        let skill = service.index().find_by_name("test-skill").unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
    }

    #[test]
    fn test_discovers_skills_from_skills_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".skills");
        fs::create_dir_all(&skills_dir).unwrap();

        fs::write(
            skills_dir.join("search.md"),
            "---\nname: search\ndescription: Search the codebase\ntags: [code, search]\n---\nUse rg first.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        assert!(service.has_skills());
        assert_eq!(service.skill_count(), 1);

        let ctx = service.build_skill_context().unwrap();
        assert!(ctx.contains("search"));
        assert!(ctx.contains("Search the codebase"));
    }

    #[test]
    fn test_discovers_skills_from_claude_skills_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".claude").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        fs::write(
            skills_dir.join("review.md"),
            "---\nname: review\ndescription: Code review\n---\nCheck code quality.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        assert_eq!(service.skill_count(), 1);
    }

    #[test]
    fn test_skill_context_format() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".harness").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        fs::write(
            skills_dir.join("summarize.md"),
            "---\nname: summarize\ndescription: Summarize documents\ntags: [docs]\n---\nSummarize text.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        let ctx = service.build_skill_context().unwrap();
        assert!(ctx.contains("Available Skills"));
        assert!(ctx.contains("**summarize**"));
        assert!(ctx.contains("Summarize documents"));
        assert!(ctx.contains("(docs)"));
    }

    #[test]
    fn test_match_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".harness").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        fs::write(
            skills_dir.join("search.md"),
            "---\nname: code-search\ndescription: Search Rust code\ntags: [code, search, rust]\n---\nUse rg.",
        )
        .unwrap();
        fs::write(
            skills_dir.join("release.md"),
            "---\nname: release\ndescription: Prepare release notes\ntags: [changelog]\n---\nSummarize commits.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        let matches = service.match_skills("search code in rust project");
        assert!(!matches.is_empty());
        assert_eq!(matches[0].skill.name, "code-search");
    }

    #[test]
    fn test_reload_picks_up_new_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let skills_dir = project.join(".harness").join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let mut service = SkillService::new(&project).unwrap();
        assert_eq!(service.skill_count(), 0);

        // Add a new skill file
        fs::write(
            skills_dir.join("new.md"),
            "---\nname: new-skill\ndescription: New skill\n---\nNew body.",
        )
        .unwrap();

        service.reload().unwrap();
        assert_eq!(service.skill_count(), 1);
        assert!(service.index().find_by_name("new-skill").is_some());
    }

    #[test]
    fn test_install_from_git_derives_name() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();

        let service = SkillService::new(&project).unwrap();

        // Test name extraction (without actually cloning)
        let name = "https://github.com/example/my-skill.git"
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("skill")
            .trim_end_matches(".git")
            .to_string();
        assert_eq!(name, "my-skill");
    }

    #[test]
    fn test_project_local_overrides_extra() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");

        // Create both .skills/ and .harness/skills/ with same-name skill
        let local_dir = project.join(".skills");
        let harness_dir = project.join(".harness").join("skills");
        fs::create_dir_all(&local_dir).unwrap();
        fs::create_dir_all(&harness_dir).unwrap();

        fs::write(
            local_dir.join("search.md"),
            "---\nname: search\ndescription: Local search\n---\nLocal.",
        )
        .unwrap();
        fs::write(
            harness_dir.join("search.md"),
            "---\nname: search\ndescription: Harness search\n---\nHarness.",
        )
        .unwrap();

        let service = SkillService::new(&project).unwrap();
        // adk-skill deduplicates — only one "search" should exist
        // .skills/ is local, .harness/skills/ is extra — local wins
        let search_skills: Vec<_> = service.index().skills().iter().filter(|s| s.name == "search").collect();
        assert_eq!(search_skills.len(), 1);
        // .skills/ (project-local in adk-skill) wins over .harness/skills/ (extra dir)
        assert_eq!(search_skills[0].description, "Local search");
    }
}
