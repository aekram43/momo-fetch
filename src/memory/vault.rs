use std::path::{Path, PathBuf};

use crate::memory::parser;
use crate::memory::types::{ExtractionResult, MemoryQuery, MemoryResult, VaultConfig, VaultCounters, VaultStats};

/// Obsidian-compatible memory vault engine.
///
/// Manages all vault I/O with vault_path, config, and counters.
/// All methods are synchronous to avoid holding locks across await points.
pub struct ObsidianVault {
    vault_path: PathBuf,
    config: VaultConfig,
}

impl ObsidianVault {
    /// Open an existing vault or create a new one.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let config_path = path.join(".vault-config.json");
        let config: VaultConfig = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content)?
        } else {
            let mut config = VaultConfig::default();
            config.vault_path = path.to_string_lossy().to_string();
            config
        };

        // Ensure directory structure exists
        for dir in &[
            "1-memcells",
            "2-events",
            "3-foresights",
            "4-episodes",
            "5-profile",
            "6-reflections/weekly",
            "6-reflections/monthly",
            "clusters",
            "templates",
        ] {
            std::fs::create_dir_all(path.join(dir))?;
        }

        Ok(Self {
            vault_path: path.to_path_buf(),
            config,
        })
    }

    /// Get the vault root path.
    pub fn path(&self) -> &Path {
        &self.vault_path
    }

    /// Get the vault configuration.
    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Get vault statistics.
    pub fn stats(&self) -> &VaultStats {
        &self.config.stats
    }

    /// Get the vault counters.
    pub fn counters(&self) -> &VaultCounters {
        &self.config.counters
    }

    // ─── Write MemCell ───────────────────────────────────────────

    /// Write a MemCell to the daily log file.
    ///
    /// Appends a new MemCell section to `1-memcells/YYYY/MM/YYYY-MM-DD.md`.
    /// Creates the file with frontmatter if it doesn't exist.
    /// Updates config counters and regenerates index.
    pub fn write_memcell(
        &mut self,
        project: &str,
        topic: &str,
        context: &str,
        actions: &[crate::memory::types::ActionRecord],
        outcome: &str,
        keywords: &[&str],
    ) -> anyhow::Result<String> {
        let now = chrono::Local::now();
        let date = now.format("%Y-%m-%d").to_string();
        let time = now.format("%H:%M").to_string();

        let memcell_path = self.vault_path
            .join("1-memcells")
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(format!("{date}.md"));

        // Determine next MemCell number
        let memcell_num = self.count_memcells_in_file(&memcell_path)? + 1;
        let memcell_ref = format!("{date}#MemCell {memcell_num:03}");

        let actions_text = actions
            .iter()
            .enumerate()
            .map(|(i, a)| format!("{}. {} → {}", i + 1, a.description, a.result))
            .collect::<Vec<_>>()
            .join("\n");

        let keywords_str = keywords.join(", ");
        let tags_yaml = format!(
            "[memcell, {}]",
            keywords.iter()
                .chain(std::iter::once(&project))
                .map(|k| k.replace(' ', "-"))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let section = format!(

            "\n## MemCell {memcell_num:03} — {time}\n\n\
             **Topic**: {topic}\n\n\
             **Context**: {context}\n\n\
             **Actions**:\n{actions_text}\n\n\
             **Outcome**: {outcome}\n\n\
             **Keywords**: {keywords_str}\n",
        );

        // Ensure parent directories exist
        if let Some(parent) = memcell_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write or append
        if memcell_path.exists() {
            let existing = std::fs::read_to_string(&memcell_path)?;
            // Update memcell_count in frontmatter
            let updated = self.update_memcell_count(&existing, memcell_num);
            let tmp = atomic_temp_path(&memcell_path);
            std::fs::write(&tmp, format!("{updated}{section}"))?;
            std::fs::rename(&tmp, &memcell_path)?;
        } else {
            let frontmatter = format!(
                "---\n\
                 type: memcell\n\
                 date: {date}\n\
                 project: {project}\n\
                 tags: {tags_yaml}\n\
                 memcell_count: {memcell_num}\n\
                 ---\n",
            );
            let tmp = atomic_temp_path(&memcell_path);
            std::fs::write(&tmp, format!("{frontmatter}{section}"))?;
            std::fs::rename(&tmp, &memcell_path)?;
        }

        // Update counters
        self.config.stats.total_memcells += 1;
        self.save_config()?;
        self.regenerate_index()?;

        Ok(memcell_ref)
    }

    // ─── Extract ─────────────────────────────────────────────────

    /// Extract events, foresights, and episode from a MemCell.
    ///
    /// Creates structured notes from the raw MemCell data using templates.
    /// Updates counters and regenerates index.
    pub fn extract_from_memcell(
        &mut self,
        memcell_ref: &str,
        project: &str,
        topic: &str,
        context: &str,
        _actions: &[crate::memory::types::ActionRecord],
        outcome: &str,
        keywords: &[&str],
    ) -> anyhow::Result<ExtractionResult> {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string();
        let mut events_created = Vec::new();
        let mut foresights_created = Vec::new();

        // Extract events: one per significant keyword/topic
        let event_facts = self.generate_event_facts(topic, context, outcome, keywords);
        for fact in &event_facts {
            self.config.counters.event += 1;
            let event_id = format!("fact-{:04}", self.config.counters.event);
            let tags = format!("[fact, {}]",
                keywords.iter()
                    .take(3)
                    .map(|k| k.replace(' ', "-"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let content = format!(
                "---\n\
                 type: event\n\
                 id: {event_id}\n\
                 created: {timestamp}\n\
                 confidence: 0.85\n\
                 status: active\n\
                 parent: \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: {tags}\n\
                 ---\n\n\
                 # {fact}\n\n\
                 {context}\n\n\
                 **Evidence**: {outcome}\n\n\
                 **Source**: [[{memcell_ref}]]\n",
            );

            let event_path = self.vault_path.join("2-events").join(format!("{event_id}.md"));
            let tmp = atomic_temp_path(&event_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &event_path)?;

            events_created.push(event_id);
            self.config.stats.total_events += 1;
        }

        // Extract foresight: one prediction based on patterns
        if let Some(prediction) = self.generate_foresight(topic, context, outcome, keywords) {
            self.config.counters.foresight += 1;
            let foresight_id = format!("pred-{:04}", self.config.counters.foresight);
            let duration_days = self.config.config.foresight_default_duration_days;
            let end_date = (now + chrono::Duration::days(duration_days as i64))
                .format("%Y-%m-%d")
                .to_string();

            let content = format!(
                "---\n\
                 type: foresight\n\
                 id: {foresight_id}\n\
                 created: {timestamp}\n\
                 status: pending\n\
                 confidence: 0.6\n\
                 start_time: {}\n\
                 end_time: {end_date}\n\
                 duration_days: {duration_days}\n\
                 parent: \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: [foresight, {}]\n\
                 validation_criteria: \"{prediction}\"\n\
                 ---\n\n\
                 # {prediction}\n\n\
                 Based on current work on \"{topic}\", this prediction tracks whether the approach holds.\n\n\
                 **Evidence**: {outcome}\n\n\
                 **Validation**: On {end_date}, check if the prediction holds.\n",
                now.format("%Y-%m-%d"),
                keywords.first().unwrap_or(&"general").replace(' ', "-"),
            );

            let foresight_path = self.vault_path
                .join("3-foresights")
                .join(format!("{foresight_id}.md"));
            let tmp = atomic_temp_path(&foresight_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &foresight_path)?;

            foresights_created.push(foresight_id);
            self.config.stats.total_foresights += 1;
            self.config.stats.pending_foresights += 1;
        }

        // Generate/update daily episode
        let episode_id = self.generate_episode(
            &now.format("%Y-%m-%d").to_string(),
            project,
            topic,
            context,
            outcome,
            memcell_ref,
            &events_created,
            &foresights_created,
            keywords,
        )?;

        // Save config and regenerate index
        self.save_config()?;
        self.regenerate_index()?;

        Ok(ExtractionResult {
            memcell_ref: memcell_ref.to_string(),
            events_created,
            foresights_created,
            episode_id: Some(episode_id),
        })
    }

    // ─── Read Note ───────────────────────────────────────────────

    /// Read a note by its ID (e.g., "fact-0001", "pred-0001").
    pub fn read_note(&self, id: &str) -> anyhow::Result<Option<(PathBuf, String)>> {
        let path = if id.starts_with("fact-") {
            self.vault_path.join("2-events").join(format!("{id}.md"))
        } else if id.starts_with("pred-") {
            self.vault_path.join("3-foresights").join(format!("{id}.md"))
        } else if id.starts_with("ep-") {
            self.vault_path.join("4-episodes").join(format!("{id}.md"))
        } else if id.starts_with("cluster-") {
            self.vault_path.join("clusters").join(format!("{id}.md"))
        } else {
            return Ok(None);
        };

        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(Some((path, content)))
        } else {
            Ok(None)
        }
    }

    /// Search using specified retrieval mode (placeholder for US-013).
    pub fn search(&self, _query: &MemoryQuery) -> anyhow::Result<Vec<MemoryResult>> {
        // TODO: Implement retrieval modes in US-013
        Ok(vec![])
    }

    // ─── Config Persistence ──────────────────────────────────────

    /// Save the vault configuration to .vault-config.json.
    fn save_config(&self) -> anyhow::Result<()> {
        let config_path = self.vault_path.join(".vault-config.json");
        let json = serde_json::to_string_pretty(&self.config)?;
        let tmp = atomic_temp_path(&config_path);
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &config_path)?;
        Ok(())
    }

    // ─── Index Regeneration ──────────────────────────────────────

    /// Regenerate the vault index.md file.
    ///
    /// Walks all vault directories and builds a master table of contents.
    fn regenerate_index(&self) -> anyhow::Result<()> {
        let mut sections = Vec::new();

        // Header
        sections.push(format!(
            "---\n\
             auto-generated: true\n\
             last_updated: {}\n\
             ---\n\n\
             # Memory Vault\n\n\
             > EverMemOS-compatible memory hierarchy, stored as Obsidian wiki.\n\
             > Levels 1-6 map to raw experiences through abstract reflections.\n",
            chrono::Local::now().format("%Y-%m-%d"),
        ));

        // Level 1: MemCells
        sections.push(self.index_memcells());

        // Level 2: Events
        sections.push(self.index_events());

        // Level 3: Foresights
        sections.push(self.index_foresights());

        // Level 4: Episodes
        sections.push(self.index_episodes());

        // Level 5: Profiles
        sections.push(self.index_profiles());

        // Level 6: Reflections
        sections.push(self.index_reflections());

        // Clusters
        sections.push(self.index_clusters());

        let content = sections.join("\n");
        let index_path = self.vault_path.join("index.md");
        let tmp = atomic_temp_path(&index_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &index_path)?;

        Ok(())
    }

    fn index_memcells(&self) -> String {
        let mut entries = Vec::new();
        let memcells_dir = self.vault_path.join("1-memcells");

        if memcells_dir.exists() {
            for entry in walkdir::WalkDir::new(&memcells_dir)
                .into_iter()
                .filter_map(|e: walkdir::Result<walkdir::DirEntry>| e.ok())
                .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext: &std::ffi::OsStr| ext == "md"))
            {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let (fm, _) = parser::parse_frontmatter(&content);
                    let date = fm
                        .as_ref()
                        .and_then(|v| v.get("date"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let count = fm
                        .as_ref()
                        .and_then(|v| v.get("memcell_count"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1);

                    let rel = entry
                        .path()
                        .file_stem()
                        .and_then(|s: &std::ffi::OsStr| s.to_str())
                        .unwrap_or(date);
                    entries.push(format!("- [[{rel}]] — {count} MemCell(s)"));
                }
            }
        }

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 1: MemCells (Raw Experiences)\n\n\
             > [!info] Daily logs of raw agent interactions\n\
             > Path: `1-memcells/YYYY/MM/YYYY-MM-DD.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_events(&self) -> String {
        let entries = self.index_level_dir("2-events", "fact-", "event");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 2: Events (Atomic Facts)\n\n\
             > [!info] One fact per note, linked back to source MemCell\n\
             > Path: `2-events/fact-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_foresights(&self) -> String {
        let entries = self.index_level_dir("3-foresights", "pred-", "foresight");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 3: Foresights (Predictions)\n\n\
             > [!info] Time-bounded predictions with validation tracking\n\
             > Path: `3-foresights/pred-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_episodes(&self) -> String {
        let entries = self.index_level_dir("4-episodes", "ep-", "episode");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 4: Episodes (Narrative Summaries)\n\n\
             > [!info] Clustered summaries of related MemCells\n\
             > Path: `4-episodes/ep-NNNN.md`\n\n\
             {entries_text}\n"
        )
    }

    fn index_profiles(&self) -> String {
        let profile_dir = self.vault_path.join("5-profile");
        let mut entries = Vec::new();

        if profile_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&profile_dir) {
                for entry in rd.flatten() {
                    if entry.path().extension().is_some_and(|ext| ext == "md") {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let stem = name.strip_suffix(".md").unwrap_or(&name);
                        entries.push(format!("- [[{stem}]] — Agent's own profile"));
                    }
                }
            }
        }

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Level 5: Profiles (Evolving Traits)\n\n\
             > [!info] Living profiles for agent and user\n\
             > Path: `5-profile/`\n\n\
             {entries_text}\n"
        )
    }

    fn index_reflections(&self) -> String {
        format!(
            "## Level 6: Reflections (Pattern Analysis)\n\n\
             > [!info] Weekly/monthly synthesis and decision weight adjustments\n\
             > Path: `6-reflections/`\n\n\
             _(no entries yet)_\n"
        )
    }

    fn index_clusters(&self) -> String {
        let entries = self.index_level_dir("clusters", "cluster-", "cluster");

        let entries_text = if entries.is_empty() {
            "_(no entries yet)_".to_string()
        } else {
            entries.join("\n")
        };

        format!(
            "## Clusters (Semantic Groupings)\n\n\
             > [!info] Virtual groupings of related MemCells for profile evolution\n\
             > Path: `clusters/`\n\n\
             {entries_text}\n"
        )
    }

    /// Index a level directory, extracting titles from frontmatter or first heading.
    fn index_level_dir(&self, dir: &str, prefix: &str, _level_name: &str) -> Vec<String> {
        let mut entries = Vec::new();
        let level_dir = self.vault_path.join(dir);

        if level_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&level_dir) {
                let mut files: Vec<_> = rd
                    .flatten()
                    .filter(|e| {
                        e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                            && e.path().extension().is_some_and(|ext| ext == "md")
                    })
                    .collect();
                files.sort_by_key(|e| e.file_name());

                for entry in files {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
                    if !stem.starts_with(prefix) {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        let title = extract_title(&content).unwrap_or_else(|| stem.clone());
                        let status = extract_status(&content);
                        let status_str = status
                            .map(|s| format!(" *({s})*"))
                            .unwrap_or_default();
                        entries.push(format!("- [[{stem}]] — {title}{status_str}"));
                    }
                }
            }
        }

        entries
    }

    // ─── Helpers ─────────────────────────────────────────────────

    /// Count MemCells in a daily log file.
    fn count_memcells_in_file(&self, path: &Path) -> anyhow::Result<u32> {
        if !path.exists() {
            return Ok(0);
        }
        let content = std::fs::read_to_string(path)?;
        let count = content
            .lines()
            .filter(|line| line.starts_with("## MemCell "))
            .count();
        Ok(count as u32)
    }

    /// Update the memcell_count in the YAML frontmatter.
    fn update_memcell_count(&self, content: &str, new_count: u32) -> String {
        let re = regex::Regex::new(r"memcell_count:\s*\d+").unwrap();
        re.replace(content, format!("memcell_count: {new_count}"))
            .to_string()
    }

    /// Generate event facts from MemCell data.
    fn generate_event_facts(
        &self,
        topic: &str,
        context: &str,
        outcome: &str,
        keywords: &[&str],
    ) -> Vec<String> {
        let mut facts = Vec::new();

        // Primary fact from the topic
        facts.push(format!("{topic}: {outcome}"));

        // Secondary facts from keywords (limit to 3 to avoid noise)
        for kw in keywords.iter().take(3) {
            if !topic.to_lowercase().contains(&kw.to_lowercase()) {
                facts.push(format!(
                    "{kw}: {} related to {topic}",
                    if context.len() > 60 {
                        format!("{}...", &context[..60])
                    } else {
                        context.to_string()
                    }
                ));
            }
        }

        facts
    }

    /// Generate a foresight prediction from MemCell data.
    fn generate_foresight(
        &self,
        topic: &str,
        context: &str,
        outcome: &str,
        keywords: &[&str],
    ) -> Option<String> {
        // Only generate foresight if the MemCell suggests a pattern or decision
        let has_predictive_signal = keywords.iter().any(|k| {
            let k = k.to_lowercase();
            k.contains("pattern")
                || k.contains("trend")
                || k.contains("decision")
                || k.contains("strategy")
                || k.contains("approach")
                || k.contains("architecture")
        }) || context.to_lowercase().contains("will")
            || context.to_lowercase().contains("should")
            || context.to_lowercase().contains("plan");

        if has_predictive_signal {
            Some(format!(
                "Approach for \"{topic}\" will remain effective: {outcome}"
            ))
        } else {
            None
        }
    }

    /// Generate or update a daily episode note.
    fn generate_episode(
        &mut self,
        date: &str,
        project: &str,
        topic: &str,
        context: &str,
        outcome: &str,
        memcell_ref: &str,
        events: &[String],
        foresights: &[String],
        keywords: &[&str],
    ) -> anyhow::Result<String> {
        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string();

        // Check if there's already an episode for today's date
        let existing_episode = self.find_episode_for_date(date);

        let episode_id = if let Some((id, _content)) = &existing_episode {
            // Update existing episode with new MemCell reference
            let id = id.clone();
            self.append_to_episode(&id, memcell_ref, events, foresights, topic)?;
            id
        } else {
            // Create new episode
            self.config.counters.episode += 1;
            let id = format!("ep-{:04}", self.config.counters.episode);

            let events_links: Vec<String> = events.iter().map(|e| format!("[[{e}]]")).collect();
            let foresights_links: Vec<String> =
                foresights.iter().map(|f| format!("[[{f}]]")).collect();
            let tags = format!("[episode, {}]",
                keywords.iter()
                    .take(2)
                    .map(|k| k.replace(' ', "-"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let content = format!(
                "---\n\
                 type: episode\n\
                 id: {id}\n\
                 created: {timestamp}\n\
                 subject: {topic}\n\
                 source_memcells:\n\
                   - \"[[{memcell_ref}]]\"\n\
                 project: {project}\n\
                 tags: {tags}\n\
                 ---\n\n\
                 ## Summary\n\n\
                 {context}\n\n\
                 **Outcome**: {outcome}\n\n\
                 ## Related\n\
                 **Events**: {}\n\
                 **Foresights**: {}\n",
                events_links.join(", "),
                if foresights_links.is_empty() {
                    "(none)".to_string()
                } else {
                    foresights_links.join(", ")
                },
            );

            let ep_path = self.vault_path.join("4-episodes").join(format!("{id}.md"));
            let tmp = atomic_temp_path(&ep_path);
            std::fs::write(&tmp, &content)?;
            std::fs::rename(&tmp, &ep_path)?;

            self.config.stats.total_episodes += 1;
            id
        };

        Ok(episode_id)
    }

    /// Find an episode that references the given date.
    fn find_episode_for_date(&self, date: &str) -> Option<(String, String)> {
        let episodes_dir = self.vault_path.join("4-episodes");
        if !episodes_dir.exists() {
            return None;
        }

        if let Ok(rd) = std::fs::read_dir(&episodes_dir) {
            for entry in rd.flatten() {
                if !entry.path().extension().is_some_and(|ext| ext == "md") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if content.contains(date) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let id = name.strip_suffix(".md").unwrap_or(&name).to_string();
                        return Some((id, content));
                    }
                }
            }
        }
        None
    }

    /// Append a new MemCell reference to an existing episode.
    fn append_to_episode(
        &self,
        episode_id: &str,
        memcell_ref: &str,
        events: &[String],
        foresights: &[String],
        topic: &str,
    ) -> anyhow::Result<()> {
        let ep_path = self.vault_path.join("4-episodes").join(format!("{episode_id}.md"));
        if !ep_path.exists() {
            return Ok(());
        }

        let mut content = std::fs::read_to_string(&ep_path)?;

        // Add new events and topic to the related section
        let new_events: Vec<String> = events.iter().map(|e| format!("[[{e}]]")).collect();
        let new_foresights: Vec<String> = foresights.iter().map(|f| format!("[[{f}]]")).collect();

        let addition = format!(
            "\n---\n\
             Additional from {memcell_ref}: {topic}\n\
             **New Events**: {}\n\
             **New Foresights**: {}\n",
            new_events.join(", "),
            if new_foresights.is_empty() {
                "(none)".to_string()
            } else {
                new_foresights.join(", ")
            },
        );

        content.push_str(&addition);

        let tmp = atomic_temp_path(&ep_path);
        std::fs::write(&tmp, &content)?;
        std::fs::rename(&tmp, &ep_path)?;

        Ok(())
    }
}

// ─── Utility Functions ───────────────────────────────────────────

/// Generate a temp file path for atomic writes.
fn atomic_temp_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(path);
    dir.join(format!("{stem}.harness-tmp{ext}"))
}

/// Extract the first markdown heading title from content.
fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed[2..].to_string());
        }
    }
    None
}

/// Extract the status field from YAML frontmatter.
fn extract_status(content: &str) -> Option<String> {
    let (fm, _) = parser::parse_frontmatter(content);
    fm.and_then(|v| v.get("status")?.as_str().map(String::from))
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::ActionRecord;

    #[test]
    fn test_vault_open_create() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = ObsidianVault::open(tmp.path()).unwrap();
        assert!(vault.path().exists());
        assert!(tmp.path().join("1-memcells").exists());
        assert!(tmp.path().join("2-events").exists());
        assert!(tmp.path().join("3-foresights").exists());
        assert!(tmp.path().join("4-episodes").exists());
        assert!(tmp.path().join("5-profile").exists());
        assert!(tmp.path().join("6-reflections/weekly").exists());
        assert!(tmp.path().join("6-reflections/monthly").exists());
        assert!(tmp.path().join("clusters").exists());
        assert!(tmp.path().join("templates").exists());
    }

    #[test]
    fn test_vault_open_existing_config() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a pre-existing config
        let config = VaultConfig::default();
        let config_path = tmp.path().join(".vault-config.json");
        std::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

        let vault = ObsidianVault::open(tmp.path()).unwrap();
        assert_eq!(vault.config().schema_version, "1.0.0");
    }

    #[test]
    fn test_write_memcell_first() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let memcell_ref = vault
            .write_memcell(
                "test-project",
                "Test Topic",
                "Testing vault write",
                &[ActionRecord {
                    description: "wrote test".into(),
                    result: "success".into(),
                }],
                "Works correctly",
                &["test", "vault"],
            )
            .unwrap();

        assert!(memcell_ref.contains("MemCell 001"));
        assert_eq!(vault.stats().total_memcells, 1);

        // Verify config was saved
        let config_path = tmp.path().join(".vault-config.json");
        assert!(config_path.exists());
        let saved: VaultConfig =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved.stats.total_memcells, 1);

        // Verify index was regenerated
        let index_path = tmp.path().join("index.md");
        assert!(index_path.exists());
    }

    #[test]
    fn test_write_memcell_multiple_same_day() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let ref1 = vault
            .write_memcell(
                "test-project",
                "First",
                "ctx1",
                &[],
                "ok1",
                &["test"],
            )
            .unwrap();

        let ref2 = vault
            .write_memcell(
                "test-project",
                "Second",
                "ctx2",
                &[],
                "ok2",
                &["test"],
            )
            .unwrap();

        assert!(ref1.contains("MemCell 001"));
        assert!(ref2.contains("MemCell 002"));
        assert_eq!(vault.stats().total_memcells, 2);
    }

    #[test]
    fn test_extract_from_memcell() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Architecture Decision",
                "Chose Obsidian wiki for vault",
                &[ActionRecord {
                    description: "researched options".into(),
                    result: "found good match".into(),
                }],
                "Obsidian wiki selected as vault backend",
                &["architecture", "decision", "vault"],
            )
            .unwrap();

        // Should have created events (one for topic + up to 2 keyword facts)
        assert!(!result.events_created.is_empty());
        assert_eq!(vault.stats().total_events, result.events_created.len() as u64);

        // Should have created a foresight (has "decision" keyword)
        assert!(!result.foresights_created.is_empty());
        assert_eq!(
            vault.stats().total_foresights,
            result.foresights_created.len() as u64
        );

        // Should have created an episode
        assert!(result.episode_id.is_some());
        assert_eq!(vault.stats().total_episodes, 1);

        // Verify event file exists
        let event_id = &result.events_created[0];
        let event_path = tmp.path().join("2-events").join(format!("{event_id}.md"));
        assert!(event_path.exists());

        // Verify foresight file exists
        let foresight_id = &result.foresights_created[0];
        let foresight_path = tmp
            .path()
            .join("3-foresights")
            .join(format!("{foresight_id}.md"));
        assert!(foresight_path.exists());

        // Verify episode file exists
        let episode_id = result.episode_id.as_ref().unwrap();
        let episode_path = tmp
            .path()
            .join("4-episodes")
            .join(format!("{episode_id}.md"));
        assert!(episode_path.exists());
    }

    #[test]
    fn test_read_note() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Create a note via extraction
        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Test Topic",
                "test context",
                &[],
                "test outcome",
                &["test"],
            )
            .unwrap();

        let event_id = &result.events_created[0];
        let note = vault.read_note(event_id).unwrap();
        assert!(note.is_some());
        let (_, content) = note.unwrap();
        assert!(content.contains("type: event"));
        assert!(content.contains(event_id));

        // Non-existent note
        let missing = vault.read_note("fact-9999").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_extract_no_foresight_without_predictive_signal() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        let result = vault
            .extract_from_memcell(
                "2026-05-07#MemCell 001",
                "test-project",
                "Fixed typo",
                "Fixed a typo in README",
                &[],
                "Typo fixed",
                &["typo", "readme"],
            )
            .unwrap();

        // Should have events but NO foresight (no predictive keywords)
        assert!(!result.events_created.is_empty());
        assert!(result.foresights_created.is_empty());
    }

    #[test]
    fn test_write_then_extract_full_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Write a MemCell
        let memcell_ref = vault
            .write_memcell(
                "agent-harness",
                "Memory vault architecture",
                "Designing the Obsidian-based memory system",
                &[
                    ActionRecord {
                        description: "analyzed EverMemOS paper".into(),
                        result: "identified 6 levels".into(),
                    },
                    ActionRecord {
                        description: "designed directory structure".into(),
                        result: "complete".into(),
                    },
                ],
                "Architecture approved and ready for implementation",
                &["memory", "architecture", "obsidian"],
            )
            .unwrap();

        // Extract from it
        let result = vault
            .extract_from_memcell(
                &memcell_ref,
                "agent-harness",
                "Memory vault architecture",
                "Designing the Obsidian-based memory system",
                &[
                    ActionRecord {
                        description: "analyzed EverMemOS paper".into(),
                        result: "identified 6 levels".into(),
                    },
                    ActionRecord {
                        description: "designed directory structure".into(),
                        result: "complete".into(),
                    },
                ],
                "Architecture approved and ready for implementation",
                &["memory", "architecture", "obsidian"],
            )
            .unwrap();

        // Verify full chain
        assert_eq!(vault.stats().total_memcells, 1);
        assert!(vault.stats().total_events >= 1);
        assert!(vault.stats().total_foresights >= 1); // "architecture" is predictive
        assert_eq!(vault.stats().total_episodes, 1);
    }

    #[test]
    fn test_config_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();

        // Create vault with default config
        let mut vault = ObsidianVault::open(tmp.path()).unwrap();

        // Modify and save
        vault.config.counters.event = 42;
        vault.config.stats.total_memcells = 100;
        vault.save_config().unwrap();

        // Re-open and verify
        let vault2 = ObsidianVault::open(tmp.path()).unwrap();
        assert_eq!(vault2.config.counters.event, 42);
        assert_eq!(vault2.config.stats.total_memcells, 100);
    }
}
